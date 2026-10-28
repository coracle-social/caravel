//! Defining a relay's roles and assigning them to members. Every change publishes
//! the events describing it in the same transaction.

use coracle_lib::events::Event;
use sqlx::PgConnection;

use crate::commands;
use crate::queries;
use caravel_domain as domain;
use caravel_domain::error::{Error, Result};
use caravel_domain::memberships::MemberSource;
use caravel_domain::roles::Role;
use caravel_util::nostr::Key;
use caravel_util::nostr::kinds::{DELETION, RELAY_MEMBERS, RELAY_ROLE};

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
        let deletion = commands::events::publish(
            &mut *conn,
            key,
            relay_id,
            DELETION,
            vec![
                domain::events::tag(&["a", &address]),
                domain::events::tag(&["k", &RELAY_ROLE.to_string()]),
            ],
            now,
        )
        .await?;
        commands::events::apply_deletion(&mut *conn, relay_id, &deletion).await?;
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
        published.push(commands::memberships::publish_member_list(conn, key, relay_id, now).await?);
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
    queries::roles::require_role(&mut *conn, relay_id, role_id).await?;
    let mut published =
        commands::memberships::admit(&mut *conn, key, relay_id, pubkey, MemberSource::Manual, now)
            .await?;

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
        published.push(commands::memberships::publish_member_list(conn, key, relay_id, now).await?);
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

    Ok(vec![
        commands::memberships::publish_member_list(conn, key, relay_id, now).await?,
    ])
}

/// Rewrite a role's definition (kind 33534).
pub async fn publish_role(
    conn: &mut PgConnection,
    key: &Key,
    relay_id: &str,
    role: &Role,
    now: i64,
) -> Result<Event> {
    commands::events::publish(conn, key, relay_id, RELAY_ROLE, role.tags(), now).await
}
