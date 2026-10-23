//! Rooms: the NIP-29 groups inside a relay. Clients' moderation events stay
//! stored as history; each one's effect on its room is projected into tables in
//! the transaction that stores it, and the relay-signed events describing the room
//! are rewritten from those tables.

use coracle_lib::events::Event;
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use sqlx::{PgConnection, PgExecutor};

use crate::crypto::RelayKey;
use crate::error::Result;
use crate::events::{self, tag};

pub mod kinds {
    pub const PUT_USER: u16 = 9000;
    pub const REMOVE_USER: u16 = 9001;
    pub const EDIT_METADATA: u16 = 9002;
    pub const DELETE_EVENT: u16 = 9005;
    pub const CREATE_GROUP: u16 = 9007;
    pub const DELETE_GROUP: u16 = 9008;
    pub const CREATE_INVITE: u16 = 9009;
    pub const PUT_PINS: u16 = 9010;
    pub const ASSIGN_PERMISSION: u16 = 9011;
    pub const UNASSIGN_PERMISSION: u16 = 9012;
    pub const JOIN_REQUEST: u16 = 9021;
    pub const LEAVE_REQUEST: u16 = 9022;
    pub const METADATA: u16 = 39000;
    pub const ADMINS: u16 = 39001;
    pub const MEMBERS: u16 = 39002;
    pub const ROLES: u16 = 39003;
    pub const LIVEKIT_PARTICIPANTS: u16 = 39004;
    pub const PINS: u16 = 39005;
}

/// Kinds the relay publishes about a room, addressed by its `d` tag.
pub fn is_metadata_kind(kind: u16) -> bool {
    (kinds::METADATA..=kinds::PINS).contains(&kind)
}

/// Kinds that change a room, each of which only the relay's managers and the room
/// members permitted that kind may publish.
pub const MODERATION_KINDS: [u16; 10] = [
    kinds::PUT_USER,
    kinds::REMOVE_USER,
    kinds::EDIT_METADATA,
    kinds::DELETE_EVENT,
    kinds::CREATE_GROUP,
    kinds::DELETE_GROUP,
    kinds::CREATE_INVITE,
    kinds::PUT_PINS,
    kinds::ASSIGN_PERMISSION,
    kinds::UNASSIGN_PERMISSION,
];

pub fn is_moderation_kind(kind: u16) -> bool {
    MODERATION_KINDS.contains(&kind)
}

/// The kinds a permission tag (`["p", <pubkey>, <kind>...]`) grants or revokes, or
/// `None` if one isn't a moderation kind.
pub fn permission_kinds(tag: &[String]) -> Option<Vec<u16>> {
    tag.get(2..)
        .unwrap_or_default()
        .iter()
        .map(|kind| {
            kind.parse::<u16>()
                .ok()
                .filter(|kind| is_moderation_kind(*kind))
        })
        .collect()
}

/// Whether an event belongs to the room machinery: it is about a room, changes
/// one, asks to join or leave one, or is posted in one.
pub fn is_room_event(kind: u16, room: Option<&str>) -> bool {
    is_metadata_kind(kind)
        || is_moderation_kind(kind)
        || matches!(kind, kinds::JOIN_REQUEST | kinds::LEAVE_REQUEST)
        || room.is_some()
}

/// The tag naming the room an event of `kind` is about: `d` for the relay's room
/// events, and `h` otherwise.
pub fn room_tag(kind: u16) -> &'static str {
    if is_metadata_kind(kind) { "d" } else { "h" }
}

/// The room an event is about.
pub fn room_id(event: &Event) -> Option<&str> {
    event
        .tags
        .value(room_tag(event.kind))
        .filter(|id| !id.is_empty())
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
    /// Labels the room's moderators give the member, which grant nothing.
    pub roles: Vec<String>,
    /// The moderation kinds the member may publish in the room.
    pub permissions: Vec<i32>,
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
        "SELECT room_id, pubkey, roles, permissions, created_at FROM room_member
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
        kinds::ASSIGN_PERMISSION => {
            // Someone granted a permission becomes a member, to see what they moderate.
            for grant in event.tags.find_all("p") {
                let permissions = permission_kinds(grant.as_slice())
                    .unwrap_or_default()
                    .into_iter()
                    .map(i32::from)
                    .collect::<Vec<_>>();
                sqlx::query(
                    "INSERT INTO room_member (relay_id, room_id, pubkey, permissions, created_at)
                     VALUES ($1, $2, $3, ARRAY(SELECT DISTINCT unnest($4::int[]) ORDER BY 1), $5)
                     ON CONFLICT (relay_id, room_id, pubkey) DO UPDATE SET permissions =
                       ARRAY(SELECT DISTINCT unnest(room_member.permissions || $4::int[]) ORDER BY 1)",
                )
                .bind(relay_id)
                .bind(room_id)
                .bind(grant.value())
                .bind(permissions)
                .bind(now)
                .execute(&mut *conn)
                .await?;
            }
            publish_membership(conn, key, relay_id, room_id, now).await
        }
        kinds::UNASSIGN_PERMISSION => {
            for grant in event.tags.find_all("p") {
                let permissions = permission_kinds(grant.as_slice())
                    .unwrap_or_default()
                    .into_iter()
                    .map(i32::from)
                    .collect::<Vec<_>>();
                sqlx::query(
                    "UPDATE room_member SET permissions =
                       ARRAY(SELECT unnest(permissions) EXCEPT SELECT unnest($4::int[]) ORDER BY 1)
                     WHERE relay_id = $1 AND room_id = $2 AND pubkey = $3",
                )
                .bind(relay_id)
                .bind(room_id)
                .bind(grant.value())
                .bind(permissions)
                .execute(&mut *conn)
                .await?;
            }
            Ok(vec![
                publish_admins(conn, key, relay_id, room_id, now).await?,
            ])
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

            let pubkey = event.pubkey.to_hex();
            let added = sqlx::query(
                "INSERT INTO room_member (relay_id, room_id, pubkey, created_at) VALUES ($1, $2, $3, $4)
                 ON CONFLICT DO NOTHING",
            )
            .bind(relay_id)
            .bind(room_id)
            .bind(&pubkey)
            .bind(now)
            .execute(&mut *conn)
            .await?
            .rows_affected();
            if added == 0 {
                return Ok(Vec::new());
            }
            publish_member_change(conn, key, relay_id, room_id, &pubkey, kinds::PUT_USER, now).await
        }
        kinds::LEAVE_REQUEST => {
            let pubkey = event.pubkey.to_hex();
            let removed = sqlx::query(
                "DELETE FROM room_member WHERE relay_id = $1 AND room_id = $2 AND pubkey = $3",
            )
            .bind(relay_id)
            .bind(room_id)
            .bind(&pubkey)
            .execute(&mut *conn)
            .await?
            .rows_affected();
            if removed == 0 {
                return Ok(Vec::new());
            }
            publish_member_change(
                conn,
                key,
                relay_id,
                room_id,
                &pubkey,
                kinds::REMOVE_USER,
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
            // The room's own state events stay, or it would describe itself wrongly.
            let ids: Vec<Vec<u8>> = event
                .tags
                .values("e")
                .filter_map(|id| hex::decode(id).ok().filter(|id| id.len() == 32))
                .collect();
            sqlx::query(
                "DELETE FROM event
                 WHERE relay_id = $1 AND id = ANY($2) AND tag_values @> ARRAY['h:' || $3]
                   AND NOT (kind BETWEEN $4 AND $5 OR kind = ANY($6))",
            )
            .bind(relay_id)
            .bind(ids)
            .bind(room_id)
            .bind(i32::from(kinds::METADATA))
            .bind(i32::from(kinds::PINS))
            .bind(MODERATION_KINDS.map(i32::from))
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
                 WHERE relay_id = $1 AND kind <> $3
                   AND ((kind BETWEEN $4 AND $5 AND d = $2) OR tag_values @> ARRAY['h:' || $2])",
            )
            .bind(relay_id)
            .bind(room_id)
            .bind(i32::from(kinds::DELETE_GROUP))
            .bind(i32::from(kinds::METADATA))
            .bind(i32::from(kinds::PINS))
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

/// Publish, as the relay, a put-user or remove-user event (`kind`) for one room
/// member, and the room's new member and admin lists.
async fn publish_member_change(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    pubkey: &str,
    kind: u16,
    now: i64,
) -> Result<Vec<Event>> {
    let event = events::publish(
        &mut *conn,
        key,
        relay_id,
        kind,
        vec![tag(&["p", pubkey]), tag(&["h", room_id])],
        now,
    )
    .await?;

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

    events::publish(conn, key, relay_id, kinds::METADATA, tags, now).await
}

/// Rewrite a room's admin list (kind 39001), naming the moderation kinds each admin
/// may publish: the relay's owner, who may publish all of them in every room, then
/// the room's members permitted any.
pub async fn publish_admins(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let owner: String = sqlx::query_scalar("SELECT tenant_pubkey FROM relay WHERE id = $1")
        .bind(relay_id)
        .fetch_one(&mut *conn)
        .await?;

    let room_admins = sqlx::query_as::<_, (String, Vec<i32>)>(
        "SELECT pubkey, permissions FROM room_member
         WHERE relay_id = $1 AND room_id = $2 AND cardinality(permissions) > 0 AND pubkey <> $3
         ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .bind(room_id)
    .bind(&owner)
    .fetch_all(&mut *conn)
    .await?;

    let admin = |pubkey: String, kinds: Vec<String>| {
        let mut tag = vec!["p".to_string(), pubkey];
        tag.extend(kinds);
        tag
    };
    let mut tags = vec![tag(&["-"]), tag(&["d", room_id])];
    tags.push(admin(
        owner,
        MODERATION_KINDS.iter().map(u16::to_string).collect(),
    ));
    for (pubkey, permissions) in room_admins {
        tags.push(admin(
            pubkey,
            permissions.iter().map(i32::to_string).collect(),
        ));
    }

    events::publish(conn, key, relay_id, kinds::ADMINS, tags, now).await
}

/// Rewrite a room's member list (kind 39002), with each member's roles.
pub async fn publish_members(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let members = sqlx::query_as::<_, (String, Vec<String>)>(
        "SELECT pubkey, roles FROM room_member WHERE relay_id = $1 AND room_id = $2 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .bind(room_id)
    .fetch_all(&mut *conn)
    .await?;

    let mut tags = vec![tag(&["-"]), tag(&["d", room_id])];
    tags.extend(members.into_iter().map(|(pubkey, roles)| {
        let mut member = vec!["p".to_string(), pubkey];
        member.extend(roles);
        member
    }));

    events::publish(conn, key, relay_id, kinds::MEMBERS, tags, now).await
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

    events::publish(conn, key, relay_id, kinds::PINS, tags, now).await
}
