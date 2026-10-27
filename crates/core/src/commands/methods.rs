//! Granting NIP-86 methods to pubkeys, and taking them back.

use coracle_lib::events::Event;
use sqlx::{PgConnection, PgExecutor};

use crate::commands;
use crate::crypto::RelayKey;
use crate::error::{Error, Result};
use crate::models::members::MemberSource;
use crate::models::methods::MANAGED_METHODS;

/// Grant a pubkey a NIP-86 method, making it a member first if it isn't one. The
/// owner and the relay's own key hold every method already.
pub async fn grant_method(
    conn: &mut PgConnection,
    key: &RelayKey,
    relay_id: &str,
    tenant_pubkey: &str,
    pubkey: &str,
    method: &str,
    now: i64,
) -> Result<Vec<Event>> {
    if !MANAGED_METHODS.contains(&method) {
        return Err(Error::invalid(
            "unknown-method",
            format!("unknown method {method:?}"),
        ));
    }
    if pubkey == tenant_pubkey || pubkey == key.pubkey() {
        return Err(Error::conflict(
            "implicit-methods",
            "the relay owner and the relay's own key always have every management method",
        ));
    }

    let published =
        commands::members::admit(&mut *conn, key, relay_id, pubkey, MemberSource::Manual, now)
            .await?;
    sqlx::query(
        "INSERT INTO method_grant (relay_id, pubkey, method, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT DO NOTHING",
    )
    .bind(relay_id)
    .bind(pubkey)
    .bind(method)
    .bind(now)
    .execute(conn)
    .await?;

    Ok(published)
}

/// Take a NIP-86 method back from a pubkey. The owner and the relay's own key
/// hold every method regardless.
pub async fn revoke_method(
    db: impl PgExecutor<'_>,
    relay_id: &str,
    tenant_pubkey: &str,
    relay_pubkey: &str,
    pubkey: &str,
    method: &str,
) -> Result<()> {
    if pubkey == tenant_pubkey || pubkey == relay_pubkey {
        return Err(Error::conflict(
            "implicit-methods",
            "the relay owner and the relay's own key always have every management method",
        ));
    }

    sqlx::query("DELETE FROM method_grant WHERE relay_id = $1 AND pubkey = $2 AND method = $3")
        .bind(relay_id)
        .bind(pubkey)
        .bind(method)
        .execute(db)
        .await?;
    Ok(())
}
