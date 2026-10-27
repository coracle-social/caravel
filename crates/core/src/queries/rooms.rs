//! Reading a relay's rooms, their members and their invites.

use coracle_lib::events::Event;
use sqlx::{PgConnection, PgExecutor};

use crate::error::Result;
use crate::models::rooms::{Room, RoomMember};

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

/// The members of one room, oldest first.
pub async fn list_members_of_room(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    room_id: &str,
) -> Result<Vec<RoomMember>> {
    Ok(sqlx::query_as::<_, RoomMember>(
        "SELECT room_id, pubkey, roles, permissions, created_at FROM room_member
         WHERE relay_id = $1 AND room_id = $2 ORDER BY created_at, pubkey",
    )
    .bind(relay_id)
    .bind(room_id)
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

pub async fn get_room(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
) -> Result<Option<Room>> {
    Ok(sqlx::query_as::<_, Room>(
        "SELECT id, tags, pins, created_at FROM room WHERE relay_id = $1 AND id = $2",
    )
    .bind(relay_id)
    .bind(room_id)
    .fetch_optional(conn)
    .await?)
}

/// Whether a join request carries one of the room's invite codes.
pub async fn is_invited(
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
