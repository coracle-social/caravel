//! Publishing the events a relay signs about a room: its metadata, admins,
//! members and pins, rewritten from the rows that hold them.

use coracle_lib::events::Event;
use sqlx::PgConnection;

use caravel_util::error::Result;
use caravel_util::nostr::Key;

use super::queries;
use caravel_util::nostr::kinds::rooms as kinds;

/// Publish, as the relay, a put-user or remove-user event (`kind`) for one room
/// member, and the room's new member and admin lists.
pub(super) async fn publish_member_change(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    room_id: &str,
    pubkey: &str,
    kind: u16,
    now: i64,
) -> Result<Vec<Event>> {
    let event = crate::events::commands::publish(
        &mut *conn,
        key,
        relay_id,
        kind,
        vec![
            crate::events::tag(&["p", pubkey]),
            crate::events::tag(&["h", room_id]),
        ],
        now,
    )
    .await?;

    let mut published = vec![event];
    published.extend(publish_membership(conn, key, relay_id, room_id, now).await?);
    Ok(published)
}

/// Rewrite a room's member and admin lists, which a membership change can alter.
pub(super) async fn publish_membership(
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
    let room = queries::get_room(&mut *conn, relay_id, room_id).await?;
    let mut tags = vec![crate::events::tag(&["d", room_id])];
    tags.extend(room.map(|room| room.tags.0).unwrap_or_default());

    crate::events::commands::publish(conn, key, relay_id, kinds::METADATA, tags, now).await
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
    let owner = crate::access::queries::owner_pubkey(&mut *conn, relay_id).await?;
    let members = queries::list_members_of_room(&mut *conn, relay_id, room_id).await?;
    let tags = crate::rooms::admins_tags(room_id, &owner, &members);

    crate::events::commands::publish(conn, key, relay_id, kinds::ADMINS, tags, now).await
}

/// Rewrite a room's member list (kind 39002), with each member's roles.
pub async fn publish_members(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let members = queries::list_members_of_room(&mut *conn, relay_id, room_id).await?;
    let tags = crate::rooms::members_tags(room_id, &members);

    crate::events::commands::publish(conn, key, relay_id, kinds::MEMBERS, tags, now).await
}

/// Rewrite a room's pinned events (kind 39005).
pub async fn publish_pins(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    room_id: &str,
    now: i64,
) -> Result<Event> {
    let room = queries::get_room(&mut *conn, relay_id, room_id).await?;
    let mut tags = vec![
        crate::events::tag(&["-"]),
        crate::events::tag(&["d", room_id]),
    ];
    tags.extend(room.map(|room| room.pins.0).unwrap_or_default());

    crate::events::commands::publish(conn, key, relay_id, kinds::PINS, tags, now).await
}
