//! Applying room events to their rooms, and rewriting the events the relay
//! publishes about each room.

use coracle_lib::events::Event;
use sqlx::PgConnection;
use sqlx::types::Json;

use crate::commands;
use crate::queries;
use caravel_domain as domain;
use caravel_util::error::Result;
use caravel_domain::rooms::MODERATION_KINDS;
use caravel_util::nostr::Key;
use caravel_util::nostr::kinds::rooms as kinds;

/// Apply a room event a client just published to its room, returning the events
/// the relay signed to publish the change. The caller has already checked that the
/// author may publish it.
pub async fn apply_event(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    event: &Event,
    now: i64,
) -> Result<Vec<Event>> {
    let Some(room_id) = domain::rooms::room_id(event) else {
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
                let permissions = domain::rooms::permission_kinds(grant.as_slice())
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
                let permissions = domain::rooms::permission_kinds(grant.as_slice())
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
            let room = queries::rooms::get_room(&mut *conn, relay_id, room_id).await?;
            let invited = queries::rooms::is_invited(&mut *conn, relay_id, room_id, event).await?;
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

/// Publish, as the relay, a put-user or remove-user event (`kind`) for one room
/// member, and the room's new member and admin lists.
async fn publish_member_change(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    room_id: &str,
    pubkey: &str,
    kind: u16,
    now: i64,
) -> Result<Vec<Event>> {
    let event = commands::events::publish(
        &mut *conn,
        key,
        relay_id,
        kind,
        vec![
            domain::events::tag(&["p", pubkey]),
            domain::events::tag(&["h", room_id]),
        ],
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
    key: &Key,
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
    key: &Key,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let room = queries::rooms::get_room(&mut *conn, relay_id, room_id).await?;
    let mut tags = vec![domain::events::tag(&["d", room_id])];
    tags.extend(room.map(|room| room.tags.0).unwrap_or_default());

    commands::events::publish(conn, key, relay_id, kinds::METADATA, tags, now).await
}

/// Rewrite a room's admin list (kind 39001), naming the moderation kinds each admin
/// may publish: the relay's owner, who may publish all of them in every room, then
/// the room's members permitted any.
pub async fn publish_admins(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let owner = queries::relays::require_relay(&mut *conn, relay_id)
        .await?
        .tenant_pubkey;
    let members = queries::rooms::list_members_of_room(&mut *conn, relay_id, room_id).await?;
    let tags = domain::rooms::admins_tags(room_id, &owner, &members);

    commands::events::publish(conn, key, relay_id, kinds::ADMINS, tags, now).await
}

/// Rewrite a room's member list (kind 39002), with each member's roles.
pub async fn publish_members(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let members = queries::rooms::list_members_of_room(&mut *conn, relay_id, room_id).await?;
    let tags = domain::rooms::members_tags(room_id, &members);

    commands::events::publish(conn, key, relay_id, kinds::MEMBERS, tags, now).await
}

/// Rewrite a room's pinned events (kind 39005).
pub async fn publish_pins(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let room = queries::rooms::get_room(&mut *conn, relay_id, room_id).await?;
    let mut tags = vec![
        domain::events::tag(&["-"]),
        domain::events::tag(&["d", room_id]),
    ];
    tags.extend(room.map(|room| room.pins.0).unwrap_or_default());

    commands::events::publish(conn, key, relay_id, kinds::PINS, tags, now).await
}
