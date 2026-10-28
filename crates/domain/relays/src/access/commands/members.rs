//! Admitting and removing a relay's members.

use coracle_lib::events::Event;
use sqlx::PgConnection;

use caravel_util::error::{Error, Result};
use caravel_util::nostr::Key;
use caravel_util::nostr::kinds::{RELAY_ADD_MEMBER, RELAY_MEMBERS, RELAY_REMOVE_MEMBER};

use crate::access::{MemberSource, RelayMember};
use crate::events;

use super::bans::unban_pubkey;
use crate::access::queries;

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
