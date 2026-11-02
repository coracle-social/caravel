//! Carrying out NIP-86 calls, and answering the ones that only read.

use caravel_billing::relays::Relay;
use caravel_util::error::Result;
use serde_json::{Value, json};

use crate::Core;

use caravel_relays::management::{Call, MANAGED_METHODS, Outcome};
use caravel_util::bus::Change;

use super::writes::{change_info, write};

/// Carry out a NIP-86 call on a relay, whose caller may make it, and return its
/// answer. A change to the relay's access lists or rooms goes out on the bus.
pub async fn call(core: &Core, relay: &Relay, call: Call) -> Result<Value> {
    let outcome = run_call(core, relay, call).await?;
    let result = outcome.result();
    if let Outcome::AccessChanged(published) = outcome {
        core.bus.publish(Change::AccessChanged {
            relay_id: relay.id.clone(),
            published,
        });
    }
    Ok(result)
}

/// Carry out a NIP-86 call on a relay, whose caller may make it, for the relay
/// runtime, which applies an AccessChanged outcome itself before answering.
pub async fn run_call(core: &Core, relay: &Relay, call: Call) -> Result<Outcome> {
    let pool = core.db.pool();
    let relay_id = relay.id.as_str();
    let tenant_pubkey = relay.tenant_pubkey.as_str();
    let now = core.clock.now();

    match &call {
        Call::ListBannedPubkeys => {
            let bans = caravel_relays::access::queries::list_banned_pubkeys(pool, relay_id).await?;
            Ok(Outcome::Answer(json!(
                bans.into_iter()
                    .map(|ban| json!({"pubkey": ban.subject, "reason": ban.reason}))
                    .collect::<Vec<_>>()
            )))
        }
        Call::ListAllowedPubkeys => {
            let members = caravel_relays::access::queries::list_members(pool, relay_id).await?;
            Ok(Outcome::Answer(json!(
                members
                    .into_iter()
                    .map(|member| json!({"pubkey": member.pubkey, "reason": "relay member"}))
                    .collect::<Vec<_>>()
            )))
        }
        Call::ListBannedEvents => {
            let bans = caravel_relays::access::queries::list_banned_events(pool, relay_id).await?;
            Ok(Outcome::Answer(json!(
                bans.into_iter()
                    .map(|ban| json!({"id": ban.subject, "reason": ban.reason}))
                    .collect::<Vec<_>>()
            )))
        }
        Call::ListMethodAssignees => {
            let all: Vec<&str> = MANAGED_METHODS.to_vec();
            let mut assignees = vec![
                json!({"pubkey": tenant_pubkey, "methods": all}),
                json!({"pubkey": relay.pubkey, "methods": all}),
            ];
            for (pubkey, methods) in
                caravel_relays::management::queries::list_method_grants(pool, relay_id).await?
            {
                if pubkey != tenant_pubkey && pubkey != relay.pubkey {
                    assignees.push(json!({"pubkey": pubkey, "methods": methods}));
                }
            }
            assignees.sort_by(|a, b| a["pubkey"].as_str().cmp(&b["pubkey"].as_str()));
            Ok(Outcome::Answer(json!(assignees)))
        }
        Call::ListClaims => Ok(Outcome::Answer(json!(
            caravel_relays::access::queries::list_claims(pool, relay_id).await?
        ))),
        Call::ChangeRelayName(name) => change_info(core, relay_id, Some(name), None, None).await,
        Call::ChangeRelayDescription(description) => {
            change_info(core, relay_id, None, Some(description), None).await
        }
        Call::ChangeRelayIcon(icon) => change_info(core, relay_id, None, None, Some(icon)).await,
        Call::UnbanPubkey(pubkey) => {
            caravel_relays::access::commands::unban_pubkey(pool, relay_id, pubkey).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::AllowEvent(id) => {
            caravel_relays::access::commands::allow_event(pool, relay_id, id).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::CreateClaim(code) => {
            caravel_relays::access::commands::create_claim(pool, relay_id, code, now).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::DeleteClaim(code) => {
            caravel_relays::access::commands::revoke_claim(pool, relay_id, code, now).await?;
            Ok(Outcome::AccessChanged(Vec::new()))
        }
        Call::BanPubkey(..)
        | Call::AllowPubkey(_)
        | Call::UnallowPubkey(_)
        | Call::BanEvent(..)
        | Call::CreateRole(_)
        | Call::EditRole(_)
        | Call::DeleteRole(_)
        | Call::AssignRole(..)
        | Call::UnassignRole(..)
        | Call::AssignMethod(..)
        | Call::UnassignMethod(..) => {
            let key = core.relay_key(relay)?;
            let published = core
                .db
                .with_tx(async |conn| write(conn, &key, relay, &call, now).await)
                .await?;

            // Removing or banning a member ends what they pay through, as the
            // dashboard's removal does. The sweep retries one Stripe refused.
            if let Call::UnallowPubkey(pubkey) | Call::BanPubkey(pubkey, _) = &call
                && let Err(error) = crate::memberships::release_removed(core, relay, pubkey).await
            {
                tracing::warn!(relay = %relay.id, pubkey, error = %error, "failed to end a removed member's membership");
            }
            Ok(Outcome::AccessChanged(published))
        }
    }
}
