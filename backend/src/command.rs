use anyhow::Result;
use sqlx::types::Json;
use sqlx::{Sqlite, Transaction};

use crate::billing::BillingPeriod;
use crate::db::{pool, publish, with_tx};
use crate::models::{
    Activity, Bolt11, Checkout, Invoice, InvoiceItem, RELAY_STATUS_ACTIVE, RELAY_STATUS_DELINQUENT,
    RELAY_STATUS_INACTIVE, Relay, Snapshot, Tenant,
};

// --- Tenants ---

pub async fn create_tenant(tenant: &Tenant) -> Result<()> {
    sqlx::query(
        "INSERT INTO tenant (pubkey, nwc_url, created_at, stripe_customer_id)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&tenant.pubkey)
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
            if relay.status == RELAY_STATUS_ACTIVE {
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
/// delinquent relay to active, and void any still-open invoices.
pub async fn reactivate_tenant(tenant_pubkey: &str, relays: &[Relay]) -> Result<()> {
    let activities = with_tx(async |tx| {
        set_tenant_churned_at_tx(tx, tenant_pubkey, None).await?;

        let mut activities = Vec::new();
        for relay in relays {
            if relay.status == RELAY_STATUS_DELINQUENT {
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

    for activity in activities {
        publish(activity);
    }
    Ok(())
}

// --- Relays ---

pub async fn create_relay(relay: &Relay) -> Result<()> {
    let activity = with_tx(async |tx| {
        sqlx::query(
            "INSERT INTO relay (
                id, tenant_pubkey, subdomain, plan_id, status, synced, sync_error,
                info_name, info_icon, info_description,
                policy_public_join, policy_strip_signatures,
                groups_enabled, management_enabled, blossom_enabled,
                livekit_enabled, push_enabled
            ) VALUES (?, ?, ?, ?, 'active', 0, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&relay.id)
        .bind(&relay.tenant_pubkey)
        .bind(&relay.subdomain)
        .bind(&relay.plan_id)
        .bind(&relay.sync_error)
        .bind(&relay.info_name)
        .bind(&relay.info_icon)
        .bind(&relay.info_description)
        .bind(relay.policy_public_join)
        .bind(relay.policy_strip_signatures)
        .bind(relay.groups_enabled)
        .bind(relay.management_enabled)
        .bind(relay.blossom_enabled)
        .bind(relay.livekit_enabled)
        .bind(relay.push_enabled)
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
        sqlx::query(
            "UPDATE relay
             SET tenant_pubkey = ?, subdomain = ?, plan_id = ?, status = ?, sync_error = ?, synced = 0,
                 info_name = ?, info_icon = ?, info_description = ?,
                 policy_public_join = ?, policy_strip_signatures = ?,
                 groups_enabled = ?, management_enabled = ?, blossom_enabled = ?,
                 livekit_enabled = ?, push_enabled = ?
             WHERE id = ?",
        )
        .bind(&relay.tenant_pubkey)
        .bind(&relay.subdomain)
        .bind(&relay.plan_id)
        .bind(&relay.status)
        .bind(&relay.sync_error)
        .bind(&relay.info_name)
        .bind(&relay.info_icon)
        .bind(&relay.info_description)
        .bind(relay.policy_public_join)
        .bind(relay.policy_strip_signatures)
        .bind(relay.groups_enabled)
        .bind(relay.management_enabled)
        .bind(relay.blossom_enabled)
        .bind(relay.livekit_enabled)
        .bind(relay.push_enabled)
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
    let now = chrono::Utc::now().timestamp();

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

/// Mark an activity billed without a line item — for activities that produce no
/// charge (e.g. free-plan changes), so a recovery pass doesn't re-scan them.
pub async fn mark_activity_billed(activity_id: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    with_tx(async |tx| {
        mark_activity_billed_tx(tx, activity_id, now).await?;
        Ok(())
    })
    .await
}

// --- Invoices ---

/// Claim all of a tenant's outstanding items onto a new invoice. A non-positive
/// balance leaves the items outstanding so the credit carries to the next positive
/// invoice. Returns the invoice, or `None` when there's nothing to bill.
pub async fn create_invoice(tenant: &Tenant, period: &BillingPeriod) -> Result<Option<Invoice>> {
    with_tx(async |tx| {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(amount), 0) FROM invoice_item
             WHERE tenant_pubkey = ? AND invoice_id IS NULL",
        )
        .bind(&tenant.pubkey)
        .fetch_one(&mut **tx)
        .await?;

        if total <= 0 {
            return Ok(None);
        }

        let invoice = insert_invoice_tx(tx, tenant, period, total).await?;

        sqlx::query(
            "UPDATE invoice_item SET invoice_id = ?
             WHERE tenant_pubkey = ? AND invoice_id IS NULL",
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

/// Atomically record a Stripe-settled invoice: persist the PaymentIntent, clear
/// the tenant's stored Stripe error, and mark the invoice paid.
pub async fn settle_invoice_via_intent(
    tenant_pubkey: &str,
    intent_id: &str,
    invoice_id: &str,
) -> Result<()> {
    with_tx(async |tx| {
        clear_tenant_stripe_error_tx(tx, tenant_pubkey).await?;
        insert_settled_intent_tx(tx, intent_id, invoice_id).await?;
        mark_invoice_paid_tx(tx, invoice_id, "stripe").await?;
        Ok(())
    })
    .await
}

/// Atomically record an invoice paid via a hosted Checkout session: stamp the
/// checkout settled, clear the tenant's stored Stripe error, and mark the invoice
/// paid. The checkout was inserted unsettled by [`insert_checkout`]. `checkout_id`
/// is the Stripe Checkout Session id, which is the checkout's primary key.
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
    let notified_at = chrono::Utc::now().timestamp();

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
    let created_at = chrono::Utc::now().timestamp();

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
/// [`Checkout`]. Mirrors [`insert_bolt11`]: created unsettled, then stamped by
/// [`settle_invoice_via_checkout`] once the session is paid. Keyed by the Stripe
/// Checkout Session id, the same way `intent` is keyed by its PaymentIntent id.
pub async fn insert_checkout(
    invoice_id: &str,
    session_id: &str,
    url: &str,
    expires_at: i64,
) -> Result<Option<Checkout>> {
    let created_at = chrono::Utc::now().timestamp();

    Ok(sqlx::query_as::<_, Checkout>(
        "INSERT INTO checkout (id, invoice_id, url, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(session_id)
    .bind(invoice_id)
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
    let created_at = chrono::Utc::now().timestamp();
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

// --- Invoices ---

async fn insert_invoice_tx(
    tx: &mut Transaction<'_, Sqlite>,
    tenant: &Tenant,
    period: &BillingPeriod,
    amount: i64,
) -> Result<Invoice> {
    let now = chrono::Utc::now().timestamp();
    let invoice_id = uuid::Uuid::new_v4().to_string();

    Ok(sqlx::query_as::<_, Invoice>(
        "INSERT INTO invoice (id, tenant_pubkey, amount, period_start, period_end, created_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(invoice_id)
    .bind(&tenant.pubkey)
    .bind(amount)
    .bind(period.start)
    .bind(period.end)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?)
}

async fn insert_invoice_item_tx(
    tx: &mut Transaction<'_, Sqlite>,
    item: &InvoiceItem,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO invoice_item
            (id, invoice_id, activity_id, tenant_pubkey, relay_id, plan_id, amount, description, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    let paid_at = chrono::Utc::now().timestamp();

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

/// Void all of a tenant's open invoices, forgiving the balance — used when a
/// tenant churns or re-activates, so old debt never has to be collected.
async fn void_open_invoices_tx(
    tx: &mut Transaction<'_, Sqlite>,
    tenant_pubkey: &str,
) -> Result<()> {
    let voided_at = chrono::Utc::now().timestamp();

    sqlx::query(
        "UPDATE invoice SET voided_at = ?
         WHERE tenant_pubkey = ? AND paid_at IS NULL AND voided_at IS NULL",
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
    let settled_at = chrono::Utc::now().timestamp();

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
    let settled_at = chrono::Utc::now().timestamp();

    sqlx::query("UPDATE checkout SET settled_at = ? WHERE id = ? AND settled_at IS NULL")
        .bind(settled_at)
        .bind(checkout_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// --- Intents ---

/// Record the Stripe PaymentIntent that paid an invoice off-session. Keyed by
/// the Stripe PaymentIntent id, so it's an idempotent audit record of what paid
/// the invoice.
async fn insert_settled_intent_tx(
    tx: &mut Transaction<'_, Sqlite>,
    intent_id: &str,
    invoice_id: &str,
) -> Result<()> {
    let created_at = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO intent (id, invoice_id, created_at)
         VALUES (?, ?, ?) ON CONFLICT(id) DO NOTHING",
    )
    .bind(intent_id)
    .bind(invoice_id)
    .bind(created_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
