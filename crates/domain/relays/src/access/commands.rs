//! Writing who holds which access to a relay.

use coracle_lib::events::Event;
use sqlx::{PgConnection, PgExecutor};

use caravel_util::error::{Error, Result};
use caravel_util::nostr::Key;
use caravel_util::nostr::kinds::{RELAY_ADD_MEMBER, RELAY_MEMBERS, RELAY_REMOVE_MEMBER};

use caravel_util::nostr::kinds::{DELETION, RELAY_ROLE};

use super::queries;
use super::{MemberSource, RelayMember, Role};
use crate::events;

/// Ban a pubkey: remove it as a member, record the ban, and delete everything it
/// wrote on the relay but its tombstones, so what it deleted stays deleted.
pub async fn ban_pubkey(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    tenant_pubkey: &str,
    pubkey: &str,
    reason: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let published = remove(&mut *conn, key, relay_id, tenant_pubkey, pubkey, now).await?;

    sqlx::query(
        "INSERT INTO banned_pubkey (relay_id, pubkey, reason, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (relay_id, pubkey) DO NOTHING",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(reason)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    let author = events::decode_id(pubkey)
        .ok_or_else(|| Error::invalid("invalid-pubkey", "invalid pubkey"))?;
    events::commands::delete_events_by_author(&mut *conn, relay_id, &author).await?;

    Ok(published)
}

pub async fn unban_pubkey(db: impl PgExecutor<'_>, relay_id: &str, pubkey: &str) -> Result<()> {
    sqlx::query("DELETE FROM banned_pubkey WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey)
        .execute(db)
        .await?;
    Ok(())
}

/// Ban an event: delete it and refuse it from then on.
pub async fn ban_event(
    conn: &mut PgConnection,
    relay_id: &str,
    event_id: &str,
    reason: &str,
    now: i64,
) -> Result<()> {
    let id = events::decode_id(event_id)
        .ok_or_else(|| Error::invalid("invalid-id", "invalid event id"))?;
    events::commands::delete_event(&mut *conn, relay_id, &id).await?;

    sqlx::query(
        "INSERT INTO banned_event (relay_id, event_id, reason, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (relay_id, event_id) DO UPDATE SET reason = EXCLUDED.reason",
    )
    .bind(relay_id)
    .bind(event_id)
    .bind(reason)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn allow_event(db: impl PgExecutor<'_>, relay_id: &str, event_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM banned_event WHERE relay_id = $1 AND event_id = $2")
        .bind(relay_id)
        .bind(event_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Register a claim, or bring a revoked one back. Creating a live claim again
/// does nothing.
pub async fn create_claim(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    code: &str,
    now: i64,
) -> Result<()> {
    if code.is_empty() {
        return Err(Error::invalid("claim-required", "claim is required"));
    }

    sqlx::query(
        "INSERT INTO claim (relay_id, code, created_at) VALUES ($1, $2, $3)
         ON CONFLICT (relay_id, code) DO UPDATE SET revoked_at = NULL",
    )
    .bind(relay_id)
    .bind(code)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Stop a claim working, keeping its history.
pub async fn revoke_claim(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    code: &str,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE claim SET revoked_at = $3 WHERE relay_id = $1 AND code = $2 AND revoked_at IS NULL",
    )
    .bind(relay_id)
    .bind(code)
    .bind(now)
    .execute(db)
    .await?;
    Ok(())
}

/// Define a role, publishing it.
pub async fn create_role(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    role: &Role,
    now: i64,
) -> Result<Vec<Event>> {
    role.validate()?;

    let inserted = sqlx::query(
        "INSERT INTO role (relay_id, id, label, description, hue, position, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (relay_id, id) DO NOTHING",
    )
    .bind(relay_id)
    .bind(&role.id)
    .bind(&role.label)
    .bind(&role.description)
    .bind(role.hue)
    .bind(role.position)
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if inserted == 0 {
        return Err(Error::conflict(
            "role-exists",
            format!("role {:?} already exists", role.id),
        ));
    }

    Ok(vec![publish_role(conn, key, relay_id, role, now).await?])
}

/// Redefine a role, publishing it.
pub async fn edit_role(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    role: &Role,
    now: i64,
) -> Result<Vec<Event>> {
    role.validate()?;

    let updated = sqlx::query(
        "UPDATE role SET label = $3, description = $4, hue = $5, position = $6 WHERE relay_id = $1 AND id = $2",
    )
    .bind(relay_id)
    .bind(&role.id)
    .bind(&role.label)
    .bind(&role.description)
    .bind(role.hue)
    .bind(role.position)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if updated == 0 {
        return Err(Error::conflict(
            "role-missing",
            format!("role {:?} does not exist", role.id),
        ));
    }

    Ok(vec![publish_role(conn, key, relay_id, role, now).await?])
}

/// Delete a role and take it from everyone who held it, publishing a deletion of its
/// definition and the new member list.
pub async fn delete_role(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    role_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let deleted = sqlx::query("DELETE FROM role WHERE relay_id = $1 AND id = $2")
        .bind(relay_id)
        .bind(role_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    let mut published = Vec::new();
    if deleted > 0 {
        let address = format!("{RELAY_ROLE}:{}:{role_id}", key.pubkey());
        let deletion = events::commands::publish(
            &mut *conn,
            key,
            relay_id,
            DELETION,
            vec![
                events::tag(&["a", &address]),
                events::tag(&["k", &RELAY_ROLE.to_string()]),
            ],
            now,
        )
        .await?;
        events::commands::apply_deletion(&mut *conn, relay_id, &deletion).await?;
        published.push(deletion);
    }

    let unassigned = sqlx::query(
        "UPDATE relay_member SET roles = array_remove(roles, $2) WHERE relay_id = $1 AND $2 = ANY(roles)",
    )
    .bind(relay_id)
    .bind(role_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if unassigned > 0 {
        published.push(publish_member_list(conn, key, relay_id, now).await?);
    }

    Ok(published)
}

/// Give a pubkey a role, making it a member first if it isn't one.
pub async fn assign_role(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    pubkey: &str,
    role_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    queries::require_role(&mut *conn, relay_id, role_id).await?;
    let mut published = admit(&mut *conn, key, relay_id, pubkey, MemberSource::Manual, now).await?;

    let assigned = sqlx::query(
        "UPDATE relay_member SET roles = array_append(roles, $3)
         WHERE relay_id = $1 AND pubkey = $2 AND NOT ($3 = ANY(roles))",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(role_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if assigned > 0 {
        published.retain(|event| event.kind != RELAY_MEMBERS);
        published.push(publish_member_list(conn, key, relay_id, now).await?);
    }

    Ok(published)
}

pub async fn unassign_role(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    pubkey: &str,
    role_id: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let unassigned = sqlx::query(
        "UPDATE relay_member SET roles = array_remove(roles, $3)
         WHERE relay_id = $1 AND pubkey = $2 AND $3 = ANY(roles)",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(role_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if unassigned == 0 {
        return Ok(Vec::new());
    }

    Ok(vec![publish_member_list(conn, key, relay_id, now).await?])
}

/// Rewrite a role's definition (kind 33534).
pub async fn publish_role(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    role: &Role,
    now: i64,
) -> Result<Event> {
    events::commands::publish(conn, key, relay_id, RELAY_ROLE, role.tags(), now).await
}

pub async fn admit(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    pubkey: &str,
    source: MemberSource,
    now: i64,
) -> Result<Vec<Event>> {
    let inserted = sqlx::query(
        "INSERT INTO relay_member (relay_id, pubkey, source, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (relay_id, pubkey) DO NOTHING",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(source)
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();

    if inserted == 0 {
        return Ok(Vec::new());
    }

    let added = events::commands::publish(
        &mut *conn,
        key,
        relay_id,
        RELAY_ADD_MEMBER,
        vec![events::tag(&["-"]), events::tag(&["p", pubkey])],
        now,
    )
    .await?;
    let list = publish_member_list(conn, key, relay_id, now).await?;
    Ok(vec![added, list])
}

/// Remove a member, with the methods granted to them, publishing the removal and
/// the new member list. The relay's owner and its own key can't be removed.
pub async fn remove(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    tenant_pubkey: &str,
    pubkey: &str,
    now: i64,
) -> Result<Vec<Event>> {
    if pubkey == tenant_pubkey || pubkey == key.pubkey() {
        return Err(Error::conflict(
            "owner",
            "Can't remove the relay owner from the relay.",
        ));
    }

    let deleted = sqlx::query("DELETE FROM relay_member WHERE relay_id = $1 AND pubkey = $2")
        .bind(relay_id)
        .bind(pubkey)
        .execute(&mut *conn)
        .await?
        .rows_affected();

    if deleted == 0 {
        return Ok(Vec::new());
    }

    let removed = events::commands::publish(
        &mut *conn,
        key,
        relay_id,
        RELAY_REMOVE_MEMBER,
        vec![events::tag(&["-"]), events::tag(&["p", pubkey])],
        now,
    )
    .await?;
    let list = publish_member_list(conn, key, relay_id, now).await?;
    Ok(vec![removed, list])
}

/// Let a pubkey in: make it a member and lift any ban on it.
pub async fn allow_pubkey(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    pubkey: &str,
    now: i64,
) -> Result<Vec<Event>> {
    let published = admit(&mut *conn, key, relay_id, pubkey, MemberSource::Manual, now).await?;
    unban_pubkey(conn, relay_id, pubkey).await?;
    Ok(published)
}

/// Rewrite the relay's member list (kind 13534): every member and their roles.
pub async fn publish_member_list(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    now: i64,
) -> Result<Event> {
    let members = queries::list_members(&mut *conn, relay_id).await?;
    let tags = RelayMember::list_tags(&members);

    events::commands::publish(conn, key, relay_id, RELAY_MEMBERS, tags, now).await
}
