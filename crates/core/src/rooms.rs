//! Rooms: the NIP-29 groups inside a relay. Clients' moderation events stay
//! stored as history; each one's effect on its room is projected into tables in
//! the transaction that stores it, and the relay-signed events describing the room
//! are rewritten from those tables.

use std::collections::BTreeSet;

use coracle_lib::events::Event;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{PgConnection, PgExecutor};

use crate::access::{store, tag};
use crate::crypto::RelayKey;
use crate::error::Result;

pub mod kinds {
    pub const PUT_USER: u16 = 9000;
    pub const REMOVE_USER: u16 = 9001;
    pub const EDIT_METADATA: u16 = 9002;
    pub const DELETE_EVENT: u16 = 9005;
    pub const CREATE_GROUP: u16 = 9007;
    pub const DELETE_GROUP: u16 = 9008;
    pub const CREATE_INVITE: u16 = 9009;
    pub const PUT_PINS: u16 = 9010;
    pub const JOIN_REQUEST: u16 = 9021;
    pub const LEAVE_REQUEST: u16 = 9022;
    pub const METADATA: u16 = 39000;
    pub const ADMINS: u16 = 39001;
    pub const MEMBERS: u16 = 39002;
    pub const ROLES: u16 = 39003;
    pub const LIVEKIT_PARTICIPANTS: u16 = 39004;
    pub const PINS: u16 = 39005;
}

/// The role that lets a room member moderate the room.
pub const ADMIN_ROLE: &str = "admin";

/// Kinds the relay publishes about a room, addressed by its `d` tag.
pub fn is_metadata_kind(kind: u16) -> bool {
    (kinds::METADATA..=kinds::PINS).contains(&kind)
}

/// Kinds that change a room, which only its moderators may publish.
pub fn is_moderation_kind(kind: u16) -> bool {
    use kinds::*;
    matches!(
        kind,
        PUT_USER
            | REMOVE_USER
            | EDIT_METADATA
            | DELETE_EVENT
            | CREATE_GROUP
            | DELETE_GROUP
            | CREATE_INVITE
            | PUT_PINS
    )
}

/// Whether an event belongs to the room machinery: it is about a room, changes
/// one, asks to join or leave one, or is posted in one.
pub fn is_room_event(kind: u16, room: Option<&str>) -> bool {
    is_metadata_kind(kind)
        || is_moderation_kind(kind)
        || matches!(kind, kinds::JOIN_REQUEST | kinds::LEAVE_REQUEST)
        || room.is_some()
}

/// The room an event is about: its `d` tag for the relay's room events, and its
/// `h` tag otherwise.
pub fn room_id(event: &Event) -> Option<&str> {
    let name = if is_metadata_kind(event.kind) {
        "d"
    } else {
        "h"
    };
    event.tags.value(name).filter(|id| !id.is_empty())
}

/// A room's settings, as its creator and moderators set them.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Room {
    pub id: String,
    /// The metadata tags from the room's latest create or edit event, without its
    /// `h` tag: its name, picture and about, and the flags `hidden`, `private`
    /// and `closed`.
    pub tags: Json<Vec<Vec<String>>>,
    /// The `e` and `a` tags of the room's pinned events.
    pub pins: Json<Vec<Vec<String>>>,
    pub created_at: i64,
}

impl Room {
    pub fn has_flag(&self, name: &str) -> bool {
        self.tags
            .iter()
            .any(|tag| tag.first().map(String::as_str) == Some(name))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RoomMember {
    pub room_id: String,
    pub pubkey: String,
    pub roles: Vec<String>,
    pub created_at: i64,
}

pub async fn list_rooms(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<Room>> {
    Ok(sqlx::query_as::<_, Room>(
        "SELECT id, tags, pins, created_at FROM room WHERE relay_id = $1 ORDER BY id",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// Every member of every room on a relay.
pub async fn list_room_members(db: impl PgExecutor<'_>, relay_id: &str) -> Result<Vec<RoomMember>> {
    Ok(sqlx::query_as::<_, RoomMember>(
        "SELECT room_id, pubkey, roles, created_at FROM room_member
         WHERE relay_id = $1 ORDER BY room_id, created_at, pubkey",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// Every invite code of every room on a relay, as `(room_id, code)`.
pub async fn list_room_invites(
    db: impl PgExecutor<'_>,
    relay_id: &str,
) -> Result<Vec<(String, String)>> {
    Ok(sqlx::query_as::<_, (String, String)>(
        "SELECT room_id, code FROM room_invite WHERE relay_id = $1 ORDER BY room_id, code",
    )
    .bind(relay_id)
    .fetch_all(db)
    .await?)
}

/// Apply a room event a client just published to its room, returning the events
/// the relay signed to publish the change. The caller has already checked that the
/// author may publish it.
pub async fn apply_event(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    event: &Event,
    now: i64,
) -> Result<Vec<Event>> {
    let Some(room_id) = room_id(event) else {
        return Ok(Vec::new());
    };

    match event.kind {
        kinds::CREATE_GROUP | kinds::EDIT_METADATA => {
            let tags: Vec<Vec<String>> = event
                .tags
                .iter()
                .filter(|tag| tag.name() != "h")
                .map(|tag| tag.as_slice().to_vec())
                .collect();
            sqlx::query(
                "INSERT INTO room (relay_id, id, tags, created_at) VALUES ($1, $2, $3, $4)
                 ON CONFLICT (relay_id, id) DO UPDATE SET tags = EXCLUDED.tags",
            )
            .bind(relay_id)
            .bind(room_id)
            .bind(Json(&tags))
            .bind(event.created_at)
            .execute(&mut *conn)
            .await?;

            Ok(vec![
                publish_metadata(&mut *conn, key, relay_id, room_id, now).await?,
                publish_admins(conn, key, relay_id, room_id, now).await?,
            ])
        }
        kinds::PUT_USER => {
            for user in event.tags.find_all("p") {
                let roles = user.as_slice().get(2..).unwrap_or_default().to_vec();
                sqlx::query(
                    "INSERT INTO room_member (relay_id, room_id, pubkey, roles, created_at) VALUES ($1, $2, $3, $4, $5)
                     ON CONFLICT (relay_id, room_id, pubkey) DO UPDATE SET roles = EXCLUDED.roles",
                )
                .bind(relay_id)
                .bind(room_id)
                .bind(user.value())
                .bind(roles)
                .bind(now)
                .execute(&mut *conn)
                .await?;
            }
            publish_membership(conn, key, relay_id, room_id, now).await
        }
        kinds::REMOVE_USER => {
            let pubkeys: Vec<String> = event.tags.values("p").map(str::to_string).collect();
            sqlx::query(
                "DELETE FROM room_member WHERE relay_id = $1 AND room_id = $2 AND pubkey = ANY($3)",
            )
            .bind(relay_id)
            .bind(room_id)
            .bind(pubkeys)
            .execute(&mut *conn)
            .await?;
            publish_membership(conn, key, relay_id, room_id, now).await
        }
        kinds::JOIN_REQUEST => {
            let room = get_room(&mut *conn, relay_id, room_id).await?;
            let invited = is_invited(&mut *conn, relay_id, room_id, event).await?;
            if room.is_none_or(|room| room.has_flag("closed") && !invited) {
                return Ok(Vec::new());
            }
            set_member(
                conn,
                key,
                relay_id,
                room_id,
                &event.pubkey.to_hex(),
                true,
                now,
            )
            .await
        }
        kinds::LEAVE_REQUEST => {
            set_member(
                conn,
                key,
                relay_id,
                room_id,
                &event.pubkey.to_hex(),
                false,
                now,
            )
            .await
        }
        kinds::CREATE_INVITE => {
            for code in event.tags.values("code") {
                sqlx::query(
                    "INSERT INTO room_invite (relay_id, room_id, code, created_at) VALUES ($1, $2, $3, $4)
                     ON CONFLICT DO NOTHING",
                )
                .bind(relay_id)
                .bind(room_id)
                .bind(code)
                .bind(now)
                .execute(&mut *conn)
                .await?;
            }
            Ok(Vec::new())
        }
        kinds::PUT_PINS => {
            let pins: Vec<Vec<String>> = event
                .tags
                .iter()
                .filter(|tag| matches!(tag.name(), "e" | "a"))
                .map(|tag| tag.as_slice().to_vec())
                .collect();
            sqlx::query("UPDATE room SET pins = $3 WHERE relay_id = $1 AND id = $2")
                .bind(relay_id)
                .bind(room_id)
                .bind(Json(&pins))
                .execute(&mut *conn)
                .await?;
            Ok(vec![publish_pins(conn, key, relay_id, room_id, now).await?])
        }
        kinds::DELETE_EVENT => {
            // The room's own state events are off limits: deleting one would leave
            // the room describing itself wrongly.
            let ids: Vec<Vec<u8>> = event
                .tags
                .values("e")
                .filter_map(|id| hex::decode(id).ok().filter(|id| id.len() == 32))
                .collect();
            sqlx::query(
                "DELETE FROM event
                 WHERE relay_id = $1 AND id = ANY($2) AND tag_values @> ARRAY['h:' || $3]
                   AND NOT (kind BETWEEN 39000 AND 39005 OR kind IN (9000, 9001, 9002, 9005, 9007, 9008, 9009, 9010))",
            )
            .bind(relay_id)
            .bind(ids)
            .bind(room_id)
            .execute(conn)
            .await?;
            Ok(Vec::new())
        }
        kinds::DELETE_GROUP => {
            sqlx::query("DELETE FROM room WHERE relay_id = $1 AND id = $2")
                .bind(relay_id)
                .bind(room_id)
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                "DELETE FROM event
                 WHERE relay_id = $1 AND kind <> 9008
                   AND ((kind BETWEEN 39000 AND 39005 AND d = $2) OR tag_values @> ARRAY['h:' || $2])",
            )
            .bind(relay_id)
            .bind(room_id)
            .execute(conn)
            .await?;
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

async fn get_room(conn: &mut PgConnection, relay_id: &str, room_id: &str) -> Result<Option<Room>> {
    Ok(sqlx::query_as::<_, Room>(
        "SELECT id, tags, pins, created_at FROM room WHERE relay_id = $1 AND id = $2",
    )
    .bind(relay_id)
    .bind(room_id)
    .fetch_optional(conn)
    .await?)
}

/// Whether a join request carries one of the room's invite codes.
async fn is_invited(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
) -> Result<bool> {
    let Some(code) = event.tags.value("code") else {
        return Ok(false);
    };

    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM room_invite WHERE relay_id = $1 AND room_id = $2 AND code = $3)",
    )
    .bind(relay_id)
    .bind(room_id)
    .bind(code)
    .fetch_one(conn)
    .await?)
}

/// Add or remove one room member as the relay, publishing a put-user or
/// remove-user event and the new member list. Nothing is written if the pubkey is
/// already where it should be.
async fn set_member(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    pubkey: &str,
    present: bool,
    now: i64,
) -> Result<Vec<Event>> {
    let changed = if present {
        sqlx::query(
            "INSERT INTO room_member (relay_id, room_id, pubkey, created_at) VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(relay_id)
        .bind(room_id)
        .bind(pubkey)
        .bind(now)
        .execute(&mut *conn)
        .await?
        .rows_affected()
    } else {
        sqlx::query("DELETE FROM room_member WHERE relay_id = $1 AND room_id = $2 AND pubkey = $3")
            .bind(relay_id)
            .bind(room_id)
            .bind(pubkey)
            .execute(&mut *conn)
            .await?
            .rows_affected()
    };

    if changed == 0 {
        return Ok(Vec::new());
    }

    let kind = if present {
        kinds::PUT_USER
    } else {
        kinds::REMOVE_USER
    };
    let event = key.sign(
        kind,
        vec![tag(&["p", pubkey]), tag(&["h", room_id])],
        "",
        now,
    );
    store(&mut *conn, relay_id, &event, now).await?;

    let mut published = vec![event];
    published.extend(publish_membership(conn, key, relay_id, room_id, now).await?);
    Ok(published)
}

/// Rewrite a room's member and admin lists, which a membership change can alter.
async fn publish_membership(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    Ok(vec![
        publish_members(&mut *conn, key, relay_id, room_id, now).await?,
        publish_admins(conn, key, relay_id, room_id, now).await?,
    ])
}

/// Rewrite a room's metadata (kind 39000).
pub async fn publish_metadata(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let room = get_room(&mut *conn, relay_id, room_id).await?;
    let mut tags = vec![tag(&["d", room_id])];
    tags.extend(room.map(|room| room.tags.0).unwrap_or_default());

    let event = key.sign(kinds::METADATA, tags, "", now);
    store(conn, relay_id, &event, now).await?;
    Ok(event)
}

/// Rewrite a room's admin list (kind 39001): the relay's owner and the pubkeys
/// granted management methods, who administer every room, then the room's own
/// admins with their roles.
pub async fn publish_admins(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let relay_admins: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT tenant_pubkey FROM relay WHERE id = $1
         UNION ALL (SELECT DISTINCT pubkey FROM method_grant WHERE relay_id = $1 ORDER BY pubkey)",
    )
    .bind(relay_id)
    .fetch_all(&mut *conn)
    .await?;

    let room_admins = sqlx::query_as::<_, (String, Vec<String>)>(
        "SELECT pubkey, roles FROM room_member
         WHERE relay_id = $1 AND room_id = $2 AND $3 = ANY(roles) ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .bind(room_id)
    .bind(ADMIN_ROLE)
    .fetch_all(&mut *conn)
    .await?;

    let mut seen = BTreeSet::new();
    let mut tags = vec![tag(&["-"]), tag(&["d", room_id])];
    for pubkey in relay_admins {
        if seen.insert(pubkey.clone()) {
            tags.push(vec!["p".to_string(), pubkey]);
        }
    }
    for (pubkey, roles) in room_admins {
        if seen.insert(pubkey.clone()) {
            let mut admin = vec!["p".to_string(), pubkey];
            admin.extend(roles);
            tags.push(admin);
        }
    }

    let event = key.sign(kinds::ADMINS, tags, "", now);
    store(conn, relay_id, &event, now).await?;
    Ok(event)
}

/// Rewrite a room's member list (kind 39002).
pub async fn publish_members(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let pubkeys = sqlx::query_scalar::<_, String>(
        "SELECT pubkey FROM room_member WHERE relay_id = $1 AND room_id = $2 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .bind(room_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut tags = vec![tag(&["-"]), tag(&["d", room_id])];
    tags.extend(
        pubkeys
            .into_iter()
            .map(|pubkey| vec!["p".to_string(), pubkey]),
    );

    let event = key.sign(kinds::MEMBERS, tags, "", now);
    store(conn, relay_id, &event, now).await?;
    Ok(event)
}

/// Rewrite a room's pinned events (kind 39005).
pub async fn publish_pins(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let room = get_room(&mut *conn, relay_id, room_id).await?;
    let mut tags = vec![tag(&["-"]), tag(&["d", room_id])];
    tags.extend(room.map(|room| room.pins.0).unwrap_or_default());

    let event = key.sign(kinds::PINS, tags, "", now);
    store(conn, relay_id, &event, now).await?;
    Ok(event)
}
