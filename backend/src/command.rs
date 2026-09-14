use anyhow::Result;
use sqlx::types::Json;
use sqlx::{Sqlite, Transaction};

use crate::billing::BillingPeriod;
use crate::clock;
use crate::db::{pool, publish, with_tx};
use crate::models::{
    Activity, Bolt11, Checkout, Intent, Invoice, InvoiceItem, RELAY_STATUS_ACTIVE,
    RELAY_STATUS_DELINQUENT, RELAY_STATUS_INACTIVE, Relay, Snapshot, Tenant, UsageSample,
};

// --- Tenants ---

pub async fn create_tenant(tenant: &Tenant) -> Result<()> {
    sqlx::query(
        "INSERT INTO tenant (pubkey, return_url, nwc_url, created_at, stripe_customer_id)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&tenant.pubkey)
    .bind(&tenant.return_url)
    .bind(&tenant.nwc_url)
    .bind(tenant.created_at)
    .bind(&tenant.stripe_customer_id)
    .execute(pool())
    .await?;
    Ok(())
}

/// Update a tenant's NWC credentials, clearing any stored NWC error so a fresh
/// wallet starts from a clean slate (it re-errors on the next charge if invalid).
pub async fn update_tenant(tenant: &Tenant) -> Result<()> {
    sqlx::query("UPDATE tenant SET nwc_url = ?, nwc_error = NULL WHERE pubkey = ?")
        .bind(&tenant.nwc_url)
        .bind(&tenant.pubkey)
        .execute(pool())
        .await?;
    Ok(())
}

/// Cache the tenant's Stripe payment method id (or clear it with `None`) and clear
/// any stored Stripe error. Called when a card is (re)attached via the portal or
/// detected during reconciliation, so collection can charge it directly and the UI
/// reflects the change.
pub async fn set_tenant_stripe_payment_method(
    pubkey: &str,
    payment_method_id: &Option<String>,
) -> Result<()> {
    sqlx::query(
        "UPDATE tenant SET stripe_payment_method_id = ?, stripe_error = NULL WHERE pubkey = ?",
    )
    .bind(payment_method_id)
    .bind(pubkey)
    .execute(pool())
    .await?;
    Ok(())
}

pub async fn set_tenant_billing_anchor(tenant: &Tenant) -> Result<()> {
    sqlx::query("UPDATE tenant SET billing_anchor = ? WHERE pubkey = ?")
        .bind(tenant.billing_anchor)
        .bind(&tenant.pubkey)
        .execute(pool())
        .await?;
    Ok(())
}

pub async fn set_tenant_nwc_error(pubkey: &str, error: &str) -> Result<()> {
    sqlx::query("UPDATE tenant SET nwc_error = ? WHERE pubkey = ?")
        .bind(error)
        .bind(pubkey)
        .execute(pool())
        .await?;
    Ok(())
}

pub async fn set_tenant_stripe_error(pubkey: &str, error: &str) -> Result<()> {
    sqlx::query("UPDATE tenant SET stripe_error = ? WHERE pubkey = ?")
        .bind(error)
        .bind(pubkey)
        .execute(pool())
        .await?;
    Ok(())
}

/// Atomically churn a tenant whose grace period has elapsed: set the churn
/// marker, mark every active relay delinquent, and void the unpaid invoices.
pub async fn churn_tenant(tenant_pubkey: &str, now: i64, relays: &[Relay]) -> Result<()> {
    let activities = with_tx(async |tx| {
        set_tenant_churned_at_tx(tx, tenant_pubkey, Some(now)).await?;

        let mut activities = Vec::new();
        for relay in relays {
            if relay.status == RELAY_STATUS_ACTIVE && relay.purged_at.is_none() {
                let activity = set_relay_status_tx(
                    tx,
                    relay,
                    RELAY_STATUS_DELINQUENT,
                    "mark_relay_delinquent",
                )
                .await?;
                activities.push(activity);
            }
        }

        void_open_invoices_tx(tx, tenant_pubkey).await?;

        Ok(activities)
    })
    .await?;

    for activity in activities {
        publish(activity);
    }
    Ok(())
}

/// Atomically re-activate a churned tenant: clear the churn marker, restore every
/// delinquent relay to active, and void any still-open invoices. Returns the
/// `unmark_relay_delinquent` activities recorded for the restored relays, so the
/// caller can fold their prorated charges into the same reconcile pass.
pub async fn reactivate_tenant(tenant_pubkey: &str, relays: &[Relay]) -> Result<Vec<Activity>> {
    let activities = with_tx(async |tx| {
        set_tenant_churned_at_tx(tx, tenant_pubkey, None).await?;

        let mut activities = Vec::new();
        for relay in relays {
            if relay.status == RELAY_STATUS_DELINQUENT && relay.purged_at.is_none() {
                let activity =
                    set_relay_status_tx(tx, relay, RELAY_STATUS_ACTIVE, "unmark_relay_delinquent")
                        .await?;
                activities.push(activity);
            }
        }

        void_open_invoices_tx(tx, tenant_pubkey).await?;

        Ok(activities)
    })
    .await?;

    for activity in &activities {
        publish(activity.clone());
    }
    Ok(activities)
}

/// Atomically ban a tenant: set the ban marker, purge every relay the tenant
/// has, and forgive everything outstanding (open invoices voided; unbilled
/// activity marked billed — except the purge activities, which stay unbilled
/// so the prorated credit still accrues to the tenant's ledger, reconciled
/// like any other credit if the tenant is ever unbanned). The relay purges are
/// published after commit so the infra reactor deletes them from zooid.
pub async fn ban_tenant(tenant_pubkey: &str, relays: &[Relay]) -> Result<()> {
    let now = clock::now();

    let activities = with_tx(async |tx| {
        sqlx::query("UPDATE tenant SET banned_at = ? WHERE pubkey = ?")
            .bind(now)
            .bind(tenant_pubkey)
            .execute(&mut **tx)
            .await?;

        sqlx::query(
            "UPDATE activity SET billed_at = ? WHERE tenant_pubkey = ? AND billed_at IS NULL
               AND activity_type != 'purge_relay'",
        )
        .bind(now)
        .bind(tenant_pubkey)
        .execute(&mut **tx)
        .await?;

        void_open_invoices_tx(tx, tenant_pubkey).await?;

        let mut activities = Vec::new();
        for relay in relays {
            if relay.purged_at.is_none() {
                activities.push(purge_relay_tx(tx, relay).await?);
            }
        }

        Ok(activities)
    })
    .await?;

    for activity in activities {
        publish(activity);
    }
    Ok(())
}

/// Clear a tenant's ban marker. Restores nothing the ban destroyed — purged
/// relays stay purged and forgiven debt stays forgiven — but the tenant can
/// create relays again and billing resumes on new activity.
pub async fn unban_tenant(tenant_pubkey: &str) -> Result<()> {
    sqlx::query("UPDATE tenant SET banned_at = NULL WHERE pubkey = ?")
        .bind(tenant_pubkey)
        .execute(pool())
        .await?;
    Ok(())
}

// --- Relays ---

pub async fn create_relay(relay: &Relay) -> Result<()> {
    let created_at = clock::now();
    let activity = with_tx(async |tx| {
        sqlx::query(
            "INSERT INTO relay (
                id, tenant_pubkey, subdomain, zooid_domain, plan_id, status, synced, sync_error,
                info_name, info_icon, info_description,
                policy_public_read, policy_public_write,
                policy_public_join, policy_strip_signatures,
                groups_enabled, management_enabled, members_can_invite, blossom_enabled,
                livekit_enabled, push_enabled, custom_domain, created_at
            ) VALUES (?, ?, ?, ?, ?, 'active', 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&relay.id)
        .bind(&relay.tenant_pubkey)
        .bind(&relay.subdomain)
        .bind(&relay.zooid_domain)
        .bind(&relay.plan_id)
        .bind(&relay.sync_error)
        .bind(&relay.info_name)
        .bind(&relay.info_icon)
        .bind(&relay.info_description)
        .bind(relay.policy_public_read)
        .bind(relay.policy_public_write)
        .bind(relay.policy_public_join)
        .bind(relay.policy_strip_signatures)
        .bind(relay.groups_enabled)
        .bind(relay.management_enabled)
        .bind(relay.members_can_invite)
        .bind(relay.blossom_enabled)
        .bind(relay.livekit_enabled)
        .bind(relay.push_enabled)
        .bind(&relay.custom_domain)
        .bind(created_at)
        .execute(&mut **tx)
        .await?;
        let snapshot = Snapshot::Relay {
            plan: relay.plan_id.clone(),
            status: RELAY_STATUS_ACTIVE.to_string(),
        };
        insert_activity_tx(tx, "create_relay", &relay.id, snapshot).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn update_relay(relay: &Relay) -> Result<()> {
    let activity = with_tx(async |tx| {
        // Reset custom_domain_verified only when the verified pairing actually
        // changes — either the custom domain itself, or the zooid domain whose
        // canonical host that domain must CNAME to. The CASE compares the row's
        // existing values (SQLite evaluates SET expressions against the original
        // row) against the incoming ones, so an update that round-trips both —
        // e.g. a feature toggle — leaves a verification set by the background
        // poller untouched.
        sqlx::query(
            "UPDATE relay
             SET tenant_pubkey = ?, subdomain = ?, zooid_domain = ?, plan_id = ?, status = ?, sync_error = ?, synced = 0,
                 info_name = ?, info_icon = ?, info_description = ?,
                 policy_public_read = ?, policy_public_write = ?,
                 policy_public_join = ?, policy_strip_signatures = ?,
                 groups_enabled = ?, management_enabled = ?, members_can_invite = ?, blossom_enabled = ?,
                 livekit_enabled = ?, push_enabled = ?,
                 custom_domain = ?, redirect_from = ?,
                 custom_domain_verified = CASE
                     WHEN custom_domain = ? AND zooid_domain = ? THEN custom_domain_verified
                     ELSE 0
                 END
             WHERE id = ?",
        )
        .bind(&relay.tenant_pubkey)
        .bind(&relay.subdomain)
        .bind(&relay.zooid_domain)
        .bind(&relay.plan_id)
        .bind(&relay.status)
        .bind(&relay.sync_error)
        .bind(&relay.info_name)
        .bind(&relay.info_icon)
        .bind(&relay.info_description)
        .bind(relay.policy_public_read)
        .bind(relay.policy_public_write)
        .bind(relay.policy_public_join)
        .bind(relay.policy_strip_signatures)
        .bind(relay.groups_enabled)
        .bind(relay.management_enabled)
        .bind(relay.members_can_invite)
        .bind(relay.blossom_enabled)
        .bind(relay.livekit_enabled)
        .bind(relay.push_enabled)
        .bind(&relay.custom_domain)
        .bind(&relay.redirect_from)
        .bind(&relay.custom_domain)
        .bind(&relay.zooid_domain)
        .bind(&relay.id)
        .execute(&mut **tx)
        .await?;
        let snapshot = Snapshot::Relay {
            plan: relay.plan_id.clone(),
            status: relay.status.clone(),
        };
        insert_activity_tx(tx, "update_relay", &relay.id, snapshot).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn activate_relay(relay: &Relay) -> Result<()> {
    set_relay_status(relay, RELAY_STATUS_ACTIVE, "activate_relay").await
}

pub async fn deactivate_relay(relay: &Relay) -> Result<()> {
    set_relay_status(relay, RELAY_STATUS_INACTIVE, "deactivate_relay").await
}

/// Mark a relay purged and record the `purge_relay` activity the infra reactor
/// reacts to by deleting the relay's data and config from zooid. `synced` is
/// reset so the row reflects that zooid no longer matches it. The activity is
/// billable: purging an active relay credits the tenant like a deactivation.
pub async fn purge_relay(relay: &Relay) -> Result<()> {
    let activity = with_tx(async |tx| purge_relay_tx(tx, relay).await).await?;
    publish(activity);
    Ok(())
}

async fn set_relay_status(relay: &Relay, status: &str, activity_type: &str) -> Result<()> {
    let activity =
        with_tx(async |tx| set_relay_status_tx(tx, relay, status, activity_type).await).await?;
    publish(activity);
    Ok(())
}

pub async fn fail_relay_sync(relay: &Relay, sync_error: String) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query("UPDATE relay SET synced = 0, sync_error = ? WHERE id = ?")
            .bind(&sync_error)
            .bind(&relay.id)
            .execute(&mut **tx)
            .await?;
        let snapshot = Snapshot::Relay {
            plan: relay.plan_id.clone(),
            status: relay.status.clone(),
        };
        insert_activity_tx(tx, "fail_relay_sync", &relay.id, snapshot).await
    })
    .await?;
    publish(activity);
    Ok(())
}

pub async fn verify_relay_custom_domain(relay_id: &str) -> Result<()> {
    sqlx::query("UPDATE relay SET custom_domain_verified = 1, synced = 0 WHERE id = ?")
        .bind(relay_id)
        .execute(pool())
        .await?;
    Ok(())
}

pub async fn unverify_relay_custom_domain(relay_id: &str) -> Result<()> {
    sqlx::query("UPDATE relay SET custom_domain_verified = 0, synced = 0 WHERE id = ?")
        .bind(relay_id)
        .execute(pool())
        .await?;
    Ok(())
}

pub async fn complete_relay_sync(relay: &Relay) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query("UPDATE relay SET synced = 1, sync_error = '' WHERE id = ?")
            .bind(&relay.id)
            .execute(&mut **tx)
            .await?;
        let snapshot = Snapshot::Relay {
            plan: relay.plan_id.clone(),
            status: relay.status.clone(),
        };
        insert_activity_tx(tx, "complete_relay_sync", &relay.id, snapshot).await
    })
    .await?;
    publish(activity);
    Ok(())
}

// --- Invoice items ---

/// Persist a reconciled activity's line item and mark the activity billed in one
/// transaction, so a recovery pass never re-bills it.
pub async fn insert_invoice_item_for_activity(
    invoice_item: &InvoiceItem,
    activity_id: &str,
) -> Result<()> {
    let now = clock::now();

    with_tx(async |tx| {
        // Claim the activity first. If a concurrent reconcile pass already billed
        // it, the claim no-ops and we skip the item rather than duplicating it.
        if mark_activity_billed_tx(tx, activity_id, now).await? {
            insert_invoice_item_tx(tx, invoice_item).await?;
        }

        Ok(())
    })
    .await
}

/// Insert this period's renewal items and advance the tenant's `renewed_at`
/// marker to `period.start`, atomically and idempotently. Empty `items` is a
/// no-op — a tenant with no active paid relays has nothing to renew.
pub async fn insert_invoice_items_for_renewal(
    items: &[InvoiceItem],
    period: &BillingPeriod,
) -> Result<()> {
    let Some(first) = items.first() else {
        return Ok(());
    };
    let tenant_pubkey = &first.tenant_pubkey;

    with_tx(async |tx| {
        // Re-read the marker inside the transaction so the guard and the writes
        // commit together — this ensures idempotency so we don't double-invoice.
        let renewed_at =
            sqlx::query_scalar::<_, Option<i64>>("SELECT renewed_at FROM tenant WHERE pubkey = ?")
                .bind(tenant_pubkey)
                .fetch_one(&mut **tx)
                .await?;

        if renewed_at.is_some_and(|at| at >= period.start) {
            return Ok(());
        }

        for item in items {
            insert_invoice_item_tx(tx, item).await?;
        }

        sqlx::query("UPDATE tenant SET renewed_at = ? WHERE pubkey = ?")
            .bind(period.start)
            .bind(tenant_pubkey)
            .execute(&mut **tx)
            .await?;

        Ok(())
    })
    .await
}

/// Record one usage reading for a relay.
pub async fn insert_usage_sample(sample: &UsageSample) -> Result<()> {
    sqlx::query(
        "INSERT INTO usage_sample
            (id, relay_id, tenant_pubkey, storage_bytes, participant_seconds, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&sample.id)
    .bind(&sample.relay_id)
    .bind(&sample.tenant_pubkey)
    .bind(sample.storage_bytes)
    .bind(sample.participant_seconds)
    .bind(sample.created_at)
    .execute(pool())
    .await?;
    Ok(())
}

/// Insert a closed period's overage items and mark that period settled in one
/// transaction, so a re-tick can't bill the same usage twice. Unlike renewal this
/// advances the marker even with nothing to bill: a period that ran inside its
/// allowance is still a period we're done with.
pub async fn insert_invoice_items_for_overage(
    tenant_pubkey: &str,
    items: &[InvoiceItem],
    period: &BillingPeriod,
) -> Result<()> {
    with_tx(async |tx| {
        // Re-read the marker inside the transaction so the guard and the writes
        // commit together — this ensures idempotency so we don't double-invoice.
        let overage_billed_at = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT overage_billed_at FROM tenant WHERE pubkey = ?",
        )
        .bind(tenant_pubkey)
        .fetch_one(&mut **tx)
        .await?;

        if overage_billed_at.is_some_and(|at| at >= period.start) {
            return Ok(());
        }

        for item in items {
            insert_invoice_item_tx(tx, item).await?;
        }

        sqlx::query("UPDATE tenant SET overage_billed_at = ? WHERE pubkey = ?")
            .bind(period.start)
            .bind(tenant_pubkey)
            .execute(&mut **tx)
            .await?;

        Ok(())
    })
    .await
}

/// Mark an activity billed without a line item — for activities that produce no
/// charge (e.g. free-plan changes), so a recovery pass doesn't re-scan them.
pub async fn mark_activity_billed(activity_id: &str) -> Result<()> {
    let now = clock::now();

    with_tx(async |tx| {
        mark_activity_billed_tx(tx, activity_id, now).await?;
        Ok(())
    })
    .await
}

// --- Invoices ---

/// Claim a tenant's outstanding items onto a new invoice once the balance clears
/// the minimum. A `comped` invoice is born paid (method `comp`), so it is never
/// collected, dunned, or counted toward churn — see [`crate::billing`].
pub async fn create_invoice(
    tenant: &Tenant,
    period: &BillingPeriod,
    comped: bool,
) -> Result<Option<Invoice>> {
    with_tx(async |tx| {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(amount), 0) FROM invoice_item
             WHERE tenant_pubkey = ? AND invoice_id IS NULL AND voided_at IS NULL",
        )
        .bind(&tenant.pubkey)
        .fetch_one(&mut **tx)
        .await?;

        // Stripe's minimum charge is $0.50 USD; $1 leaves margin so a later
        // small credit can't drop a fresh invoice under that floor. Leave
        // items outstanding and carry to a later invoice.
        if total <= 100 {
            return Ok(None);
        }

        let invoice = insert_invoice_tx(tx, tenant, period, total, comped).await?;

        sqlx::query(
            "UPDATE invoice_item SET invoice_id = ?
             WHERE tenant_pubkey = ? AND invoice_id IS NULL AND voided_at IS NULL",
        )
        .bind(&invoice.id)
        .bind(&tenant.pubkey)
        .execute(&mut **tx)
        .await?;

        Ok(Some(invoice))
    })
    .await
}

// --- Payment settlement ---

/// Atomically record a Lightning settlement that happened out of band.
pub async fn settle_invoice_out_of_band(bolt11_id: &str, invoice_id: &str) -> Result<()> {
    with_tx(async |tx| {
        mark_bolt11_settled_tx(tx, bolt11_id).await?;
        mark_invoice_paid_tx(tx, invoice_id, "oob").await?;
        Ok(())
    })
    .await
}

/// Atomically record an NWC-settled invoice: clear the tenant's stored NWC error,
/// mark the bolt11 settled, and mark the invoice paid.
pub async fn settle_invoice_via_nwc(
    tenant_pubkey: &str,
    bolt11_id: &str,
    invoice_id: &str,
) -> Result<()> {
    with_tx(async |tx| {
        clear_tenant_nwc_error_tx(tx, tenant_pubkey).await?;
        mark_bolt11_settled_tx(tx, bolt11_id).await?;
        mark_invoice_paid_tx(tx, invoice_id, "nwc").await?;
        Ok(())
    })
    .await
}

/// Atomically settle an invoice paid off-session: stamp the write-ahead intent
/// with the Stripe PaymentIntent that confirmed it, clear the tenant's stored
/// Stripe error, and mark the invoice paid. `intent_id` is our row id (from
/// [`insert_pending_intent`]); `payment_intent_id` is the Stripe `pi_…`.
pub async fn settle_invoice_via_intent(
    tenant_pubkey: &str,
    intent_id: &str,
    payment_intent_id: &str,
    invoice_id: &str,
) -> Result<()> {
    with_tx(async |tx| {
        clear_tenant_stripe_error_tx(tx, tenant_pubkey).await?;
        mark_intent_settled_tx(tx, intent_id, payment_intent_id).await?;
        mark_invoice_paid_tx(tx, invoice_id, "stripe").await?;
        Ok(())
    })
    .await
}

/// Atomically record an invoice paid via a hosted Checkout session: stamp the
/// checkout settled, clear the tenant's stored Stripe error, and mark the invoice
/// paid. The checkout was inserted unsettled by [`insert_checkout`]. `checkout_id`
/// is our row id, not the Stripe Checkout Session id.
pub async fn settle_invoice_via_checkout(
    tenant_pubkey: &str,
    checkout_id: &str,
    invoice_id: &str,
) -> Result<()> {
    with_tx(async |tx| {
        clear_tenant_stripe_error_tx(tx, tenant_pubkey).await?;
        mark_checkout_settled_tx(tx, checkout_id).await?;
        mark_invoice_paid_tx(tx, invoice_id, "stripe").await?;
        Ok(())
    })
    .await
}

/// Stamp an invoice with the time its manual-payment reminder DM was sent, so
/// dunning sends that DM once instead of on every hourly poll.
pub async fn mark_invoice_notified(invoice_id: &str) -> Result<()> {
    let notified_at = clock::now();

    sqlx::query("UPDATE invoice SET notified_at = ? WHERE id = ?")
        .bind(notified_at)
        .bind(invoice_id)
        .execute(pool())
        .await?;
    Ok(())
}

// --- Bolt11 records ---

pub async fn insert_bolt11(
    invoice_id: &str,
    lnbc: &str,
    msats: i64,
    expires_at: i64,
) -> Result<Option<Bolt11>> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = clock::now();

    Ok(sqlx::query_as::<_, Bolt11>(
        "INSERT INTO bolt11 (id, invoice_id, lnbc, msats, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(id)
    .bind(invoice_id)
    .bind(lnbc)
    .bind(msats)
    .bind(created_at)
    .bind(expires_at)
    .fetch_optional(pool())
    .await?)
}

// --- Checkout records ---

/// Record a pending Stripe Checkout session for an invoice, returning the stored
/// [`Checkout`]. Mirrors [`insert_bolt11`]: created unsettled with our own id,
/// then stamped by [`settle_invoice_via_checkout`] once the session is paid.
pub async fn insert_checkout(
    invoice_id: &str,
    session_id: &str,
    url: &str,
    expires_at: i64,
) -> Result<Option<Checkout>> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = clock::now();

    Ok(sqlx::query_as::<_, Checkout>(
        "INSERT INTO checkout (id, invoice_id, session_id, url, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(id)
    .bind(invoice_id)
    .bind(session_id)
    .bind(url)
    .bind(created_at)
    .bind(expires_at)
    .fetch_optional(pool())
    .await?)
}

// --- Internal utils that take an explicit transaction ---

// --- Activities ---

async fn insert_activity_tx(
    tx: &mut Transaction<'_, Sqlite>,
    activity_type: &str,
    resource_id: &str,
    snapshot: Snapshot,
) -> Result<Activity> {
    let resource_type = snapshot.resource_type();
    let tenant_pubkey = match &snapshot {
        Snapshot::Relay { .. } => {
            sqlx::query_scalar::<_, String>("SELECT tenant_pubkey FROM relay WHERE id = ?")
                .bind(resource_id)
                .fetch_one(&mut **tx)
                .await?
        }
    };

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = clock::now();
    let snapshot = Json(snapshot);

    sqlx::query(
        "INSERT INTO activity (id, tenant_pubkey, created_at, activity_type, resource_type, resource_id, snapshot)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&tenant_pubkey)
    .bind(created_at)
    .bind(activity_type)
    .bind(resource_type)
    .bind(resource_id)
    .bind(&snapshot)
    .execute(&mut **tx)
    .await?;

    Ok(Activity {
        id,
        tenant_pubkey,
        created_at,
        activity_type: activity_type.to_string(),
        resource_type: resource_type.to_string(),
        resource_id: resource_id.to_string(),
        billed_at: None,
        snapshot,
    })
}

/// Claim an activity as billed. Returns `true` if this call set the marker, and
/// `false` if it was already set — e.g. a concurrent reconcile pass won the race —
/// so callers can skip work that would otherwise double-bill.
async fn mark_activity_billed_tx(
    tx: &mut Transaction<'_, Sqlite>,
    activity_id: &str,
    billed_at: i64,
) -> Result<bool> {
    let result =
        sqlx::query("UPDATE activity SET billed_at = ? WHERE id = ? AND billed_at IS NULL")
            .bind(billed_at)
            .bind(activity_id)
            .execute(&mut **tx)
            .await?;
    Ok(result.rows_affected() > 0)
}

// --- Tenants ---

/// Set or clear the tenant's churn marker. Set when an invoice ages past the
/// grace period, cleared when billing is re-activated.
async fn set_tenant_churned_at_tx(
    tx: &mut Transaction<'_, Sqlite>,
    pubkey: &str,
    churned_at: Option<i64>,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET churned_at = ? WHERE pubkey = ?")
        .bind(churned_at)
        .bind(pubkey)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn clear_tenant_nwc_error_tx(tx: &mut Transaction<'_, Sqlite>, pubkey: &str) -> Result<()> {
    sqlx::query("UPDATE tenant SET nwc_error = NULL WHERE pubkey = ?")
        .bind(pubkey)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn clear_tenant_stripe_error_tx(
    tx: &mut Transaction<'_, Sqlite>,
    pubkey: &str,
) -> Result<()> {
    sqlx::query("UPDATE tenant SET stripe_error = NULL WHERE pubkey = ?")
        .bind(pubkey)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// --- Relays ---

/// Set a relay's status (and flag it for re-sync), recording the matching
/// activity. Returns the activity so the caller can `publish` it after the
/// enclosing transaction commits.
async fn set_relay_status_tx(
    tx: &mut Transaction<'_, Sqlite>,
    relay: &Relay,
    status: &str,
    activity_type: &str,
) -> Result<Activity> {
    sqlx::query("UPDATE relay SET status = ?, synced = 0 WHERE id = ?")
        .bind(status)
        .bind(&relay.id)
        .execute(&mut **tx)
        .await?;
    let snapshot = Snapshot::Relay {
        plan: relay.plan_id.clone(),
        status: status.to_string(),
    };
    insert_activity_tx(tx, activity_type, &relay.id, snapshot).await
}

/// Mark a relay purged inside an enclosing transaction, returning the
/// `purge_relay` activity for the caller to publish after commit.
async fn purge_relay_tx(tx: &mut Transaction<'_, Sqlite>, relay: &Relay) -> Result<Activity> {
    let now = clock::now();

    sqlx::query("UPDATE relay SET purged_at = ?, synced = 0, sync_error = '' WHERE id = ?")
        .bind(now)
        .bind(&relay.id)
        .execute(&mut **tx)
        .await?;
    let snapshot = Snapshot::Relay {
        plan: relay.plan_id.clone(),
        status: relay.status.clone(),
    };
    insert_activity_tx(tx, "purge_relay", &relay.id, snapshot).await
}

// --- Invoices ---

async fn insert_invoice_tx(
    tx: &mut Transaction<'_, Sqlite>,
    tenant: &Tenant,
    period: &BillingPeriod,
    amount: i64,
    comped: bool,
) -> Result<Invoice> {
    let now = clock::now();
    let invoice_id = uuid::Uuid::new_v4().to_string();
    let (paid_at, method) = if comped {
        (Some(now), Some("comp"))
    } else {
        (None, None)
    };

    Ok(sqlx::query_as::<_, Invoice>(
        "INSERT INTO invoice
            (id, tenant_pubkey, amount, period_start, period_end, created_at, paid_at, method)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(invoice_id)
    .bind(&tenant.pubkey)
    .bind(amount)
    .bind(period.start)
    .bind(period.end)
    .bind(now)
    .bind(paid_at)
    .bind(method)
    .fetch_one(&mut **tx)
    .await?)
}

async fn insert_invoice_item_tx(
    tx: &mut Transaction<'_, Sqlite>,
    item: &InvoiceItem,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO invoice_item
            (id, invoice_id, activity_id, tenant_pubkey, relay_id, plan_id, amount, description, created_at, voided_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&item.id)
    .bind(&item.invoice_id)
    .bind(&item.activity_id)
    .bind(&item.tenant_pubkey)
    .bind(&item.relay_id)
    .bind(&item.plan_id)
    .bind(item.amount)
    .bind(&item.description)
    .bind(item.created_at)
    .bind(item.voided_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Mark an invoice paid, but only while it is still open — a late Lightning
/// payment never flips a voided/forgiven invoice to paid, and a Stripe-paid
/// invoice never has its provenance overwritten by a later bolt11.
async fn mark_invoice_paid_tx(
    tx: &mut Transaction<'_, Sqlite>,
    invoice_id: &str,
    method: &str,
) -> Result<()> {
    let paid_at = clock::now();

    sqlx::query(
        "UPDATE invoice SET method = ?, paid_at = ?
         WHERE id = ? AND paid_at IS NULL AND voided_at IS NULL",
    )
    .bind(method)
    .bind(paid_at)
    .bind(invoice_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Void all of a tenant's open invoices and unpaid line items, forgiving the
/// balance — used when a tenant churns or re-activates, so old debt never has to
/// be collected. Voiding the items too (both outstanding ones and those on the
/// just-voided invoices) keeps a credit from bleeding into a future invoice and
/// lets a re-billed period start from a clean ledger. Items on a paid invoice are
/// left untouched.
async fn void_open_invoices_tx(
    tx: &mut Transaction<'_, Sqlite>,
    tenant_pubkey: &str,
) -> Result<()> {
    let voided_at = clock::now();

    sqlx::query(
        "UPDATE invoice SET voided_at = ?
         WHERE tenant_pubkey = ? AND paid_at IS NULL AND voided_at IS NULL",
    )
    .bind(voided_at)
    .bind(tenant_pubkey)
    .execute(&mut **tx)
    .await?;

    // Run after voiding the invoices above, so the `paid_at IS NULL` subquery
    // catches their now-voided items along with the still-outstanding ones.
    sqlx::query(
        "UPDATE invoice_item SET voided_at = ?
         WHERE tenant_pubkey = ? AND voided_at IS NULL
           AND (invoice_id IS NULL OR invoice_id IN (
               SELECT id FROM invoice WHERE paid_at IS NULL
           ))",
    )
    .bind(voided_at)
    .bind(tenant_pubkey)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// --- Bolt11 ---

/// Stamp a bolt11 as settled but don't overwrite an existing settled_at.
async fn mark_bolt11_settled_tx(tx: &mut Transaction<'_, Sqlite>, bolt11_id: &str) -> Result<()> {
    let settled_at = clock::now();

    sqlx::query("UPDATE bolt11 SET settled_at = ? WHERE id = ? AND settled_at IS NULL")
        .bind(settled_at)
        .bind(bolt11_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// --- Checkouts ---

/// Stamp a checkout as settled but don't overwrite an existing settled_at, so a
/// re-reconcile of the same session is a no-op. Keyed by our row id.
async fn mark_checkout_settled_tx(
    tx: &mut Transaction<'_, Sqlite>,
    checkout_id: &str,
) -> Result<()> {
    let settled_at = clock::now();

    sqlx::query("UPDATE checkout SET settled_at = ? WHERE id = ? AND settled_at IS NULL")
        .bind(settled_at)
        .bind(checkout_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// --- Intents ---

/// Write-ahead an off-session charge attempt before confirming it with Stripe,
/// returning the stored [`Intent`]. Records the payment method so a retry after a
/// lost settle re-confirms the same (idempotent) PaymentIntent; settled later by
/// [`settle_invoice_via_intent`], or dropped by [`delete_intent`] if it declines.
///
/// Get-or-create, atomically: if the invoice already has an unsettled intent,
/// returns it unchanged (keeping its original `payment_method_id`, so the retry
/// re-confirms the same charge); otherwise inserts a fresh one. The partial
/// unique index on (invoice_id) WHERE settled_at IS NULL makes this race-free —
/// concurrent reconciles converge on one intent instead of two.
pub async fn ensure_pending_intent(invoice_id: &str, payment_method_id: &str) -> Result<Intent> {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = clock::now();

    Ok(sqlx::query_as::<_, Intent>(
        "INSERT INTO intent (id, invoice_id, payment_method_id, created_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(invoice_id) WHERE settled_at IS NULL
           DO UPDATE SET payment_method_id = intent.payment_method_id
         RETURNING *",
    )
    .bind(id)
    .bind(invoice_id)
    .bind(payment_method_id)
    .bind(created_at)
    .fetch_one(pool())
    .await?)
}

/// Stamp an off-session intent settled with the Stripe PaymentIntent that
/// confirmed it, but don't overwrite an existing settled_at — so reconciling the
/// same attempt twice is a no-op.
async fn mark_intent_settled_tx(
    tx: &mut Transaction<'_, Sqlite>,
    intent_id: &str,
    payment_intent_id: &str,
) -> Result<()> {
    let settled_at = clock::now();

    sqlx::query(
        "UPDATE intent SET settled_at = ?, payment_intent_id = ?
         WHERE id = ? AND settled_at IS NULL",
    )
    .bind(settled_at)
    .bind(payment_intent_id)
    .bind(intent_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Drop a write-ahead intent whose charge didn't go through, so the next attempt
/// starts clean (on the tenant's current method). A charge that confirmed but
/// whose settle failed is instead left unsettled, for reconcile to re-confirm.
pub async fn delete_intent(intent_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM intent WHERE id = ?")
        .bind(intent_id)
        .execute(pool())
        .await?;
    Ok(())
}
