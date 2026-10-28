//! Applying a room event a client published to the rows that hold the room: its
//! metadata, members, permissions, invites, pins and deletions. Each takes the
//! event the rules already admitted and makes the change it describes.

use coracle_lib::events::Event;
use sqlx::PgConnection;
use sqlx::types::Json;

use caravel_util::error::Result;
use caravel_util::nostr::kinds::rooms as kinds;

use crate::rooms::MODERATION_KINDS;

/// Store a room's metadata: the tags a create or an edit carries, without the `h`
/// tag that only names the room.
pub async fn metadata(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
) -> Result<()> {
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
    Ok(())
}

/// Put the pubkeys a `PUT_USER` names into the room, with the roles it gives them.
pub async fn users(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
    now: i64,
) -> Result<()> {
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
    Ok(())
}

/// Grant the moderation kinds a permission event names. Someone granted one
/// becomes a member, to see what they moderate.
pub async fn grant_permissions(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
    now: i64,
) -> Result<()> {
    for grant in event.tags.find_all("p") {
        let permissions = crate::rooms::permission_kinds(grant.as_slice())
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
    Ok(())
}

/// Take back the moderation kinds a permission event names.
pub async fn revoke_permissions(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
) -> Result<()> {
    for grant in event.tags.find_all("p") {
        let permissions = crate::rooms::permission_kinds(grant.as_slice())
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
    Ok(())
}

/// Remove the pubkeys a `REMOVE_USER` names.
pub async fn remove_users(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
) -> Result<()> {
    let pubkeys: Vec<String> = event.tags.values("p").map(str::to_string).collect();
    sqlx::query(
        "DELETE FROM room_member WHERE relay_id = $1 AND room_id = $2 AND pubkey = ANY($3)",
    )
    .bind(relay_id)
    .bind(room_id)
    .bind(pubkeys)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Add the event's author to the room. Returns false when they were a member
/// already, when there is nothing to publish.
pub async fn join(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    pubkey: &str,
    now: i64,
) -> Result<bool> {
    let added = sqlx::query(
        "INSERT INTO room_member (relay_id, room_id, pubkey, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT DO NOTHING",
    )
    .bind(relay_id)
    .bind(room_id)
    .bind(pubkey)
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(added > 0)
}

/// Take the event's author out of the room. Returns false when they were not a
/// member, when there is nothing to publish.
pub async fn leave(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    pubkey: &str,
) -> Result<bool> {
    let removed =
        sqlx::query("DELETE FROM room_member WHERE relay_id = $1 AND room_id = $2 AND pubkey = $3")
            .bind(relay_id)
            .bind(room_id)
            .bind(pubkey)
            .execute(&mut *conn)
            .await?
            .rows_affected();
    Ok(removed > 0)
}

/// Record the invite codes an invite event names.
pub async fn invites(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
    now: i64,
) -> Result<()> {
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
    Ok(())
}

/// Replace the room's pins with the events and addresses a pins event names.
pub async fn pins(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
) -> Result<()> {
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
    Ok(())
}

/// Delete the events a deletion names from the room.
pub async fn delete_events(
    conn: &mut PgConnection,
    relay_id: &str,
    room_id: &str,
    event: &Event,
) -> Result<()> {
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
    Ok(())
}

/// Delete the room and everything in it, but its metadata and pins.
pub async fn delete_room(conn: &mut PgConnection, relay_id: &str, room_id: &str) -> Result<()> {
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
    Ok(())
}
