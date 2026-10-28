//! Reading invoices and what they charge for.

use caravel_billing::tenants::BillingPeriod;
use caravel_billing::tenants::Tenant;
use caravel_billing::tenants::{Invoice, InvoiceItem};
use caravel_util::error::Result;

use crate::Core;

// --- Invoices ---

/// The invoice with `id`, or a 404.
pub async fn require_invoice(core: &Core, id: &str) -> Result<Invoice> {
    caravel_billing::tenants::queries::billing::require_invoice(core.db.pool(), id).await
}

/// Every invoice on the platform, newest first.
pub async fn list_invoices(core: &Core) -> Result<Vec<Invoice>> {
    caravel_billing::tenants::queries::billing::list_invoices(core.db.pool()).await
}

/// A tenant's invoices, newest first.
pub async fn list_invoices_for_tenant(core: &Core, tenant_pubkey: &str) -> Result<Vec<Invoice>> {
    caravel_billing::tenants::queries::billing::list_invoices_for_tenant(
        core.db.pool(),
        tenant_pubkey,
    )
    .await
}

/// The line items of an invoice.
pub async fn list_invoice_items(core: &Core, invoice_id: &str) -> Result<Vec<InvoiceItem>> {
    caravel_billing::tenants::queries::billing::list_invoice_items_for_invoice(
        core.db.pool(),
        invoice_id,
    )
    .await
}

/// The line items a tenant owes for that no invoice has claimed.
pub async fn list_outstanding_items(core: &Core, tenant_pubkey: &str) -> Result<Vec<InvoiceItem>> {
    caravel_billing::tenants::queries::billing::list_outstanding_invoice_items_for_tenant(
        core.db.pool(),
        tenant_pubkey,
    )
    .await
}

/// The draft invoice: an unsaved invoice summing the tenant's outstanding items for
/// the current period, or `None` when nothing is outstanding.
pub async fn get_draft_invoice(core: &Core, tenant: &Tenant) -> Result<Option<Invoice>> {
    let now = core.clock.now();
    let Some(period) = BillingPeriod::at(tenant.billing_anchor, now) else {
        return Ok(None);
    };

    let items =
        caravel_billing::tenants::queries::billing::list_outstanding_invoice_items_for_tenant(
            core.db.pool(),
            &tenant.pubkey,
        )
        .await?;
    if items.is_empty() {
        return Ok(None);
    }

    Ok(Some(Invoice {
        id: "draft".to_string(),
        tenant_pubkey: tenant.pubkey.clone(),
        amount: items.iter().map(|item| item.amount).sum(),
        period_start: period.start,
        period_end: period.end,
        created_at: now,
        paid_at: None,
        voided_at: None,
        notified_at: None,
        method: None,
    }))
}
