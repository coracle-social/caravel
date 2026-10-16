import { createMemo } from "solid-js"
import { indexBy } from "@welshman/lib"
import { listPayableInvoices, type Invoice, type Tenant } from "@/shared/lib/api"
import { autopayConfigured, cardState, nwcState } from "@/shared/lib/paymentMethod"
import { autopayBilling, billingDraftInvoice, billingInvoices, billingRelays, billingTenant, plans } from "@/shared/lib/state"

export type BillingPromptKind = "churned" | "pay_invoice" | "update_method" | "setup_autopay"

export type Severity = "error" | "warn" | "info"

export type BillingPrompt = {
  kind: BillingPromptKind
  severity: Severity
  message: string
}

export type BillingStatusSnapshot = {
  tenant: Tenant | undefined
  openInvoice: Invoice | undefined
  hasPaidSubscription: boolean
}

// The signed-in tenant's billing. `openInvoice` is the oldest owed, which collection charges first.
export function useBillingStatus() {
  const tenant = () => billingTenant()
  const invoices = () => billingInvoices() ?? []
  const draftInvoice = () => billingDraftInvoice() ?? undefined
  const openInvoices = createMemo(() => listPayableInvoices(invoices()))
  const openInvoice = () => openInvoices()[0]

  const hasPaidSubscription = createMemo(() => {
    const planById = indexBy((p) => p.id, plans())
    return (billingRelays() ?? []).some((relay) => {
      const plan = planById.get(relay.plan_id)
      return Boolean(plan && plan.amount > 0 && relay.status === "active")
    })
  })

  const loading = () => billingTenant.loading || billingInvoices.loading || billingDraftInvoice.loading

  return { tenant, invoices, draftInvoice, openInvoice, hasPaidSubscription, loading, autopay: autopayBilling }
}

// The one billing prompt to show: churn, an open invoice, a failed method, then autopay.
// `suppressInline` leaves out the invoice and autopay ones while a page shows its own.
export function activeBillingPrompt(
  s: BillingStatusSnapshot,
  opts?: { suppressInline?: boolean },
): BillingPrompt | null {
  const tenant = s.tenant
  if (!tenant) return null

  if (tenant.churned_at) {
    return {
      kind: "churned",
      severity: "error",
      message:
        "Your account is past due and some relays are paused. Pay your balance or update your payment method to restore service.",
    }
  }

  const hasAutopay = autopayConfigured(tenant)
  const nwc = nwcState(tenant)
  const card = cardState(tenant)
  const methodError = nwc.kind === "error" || card.kind === "error"
  const suppressInline = opts?.suppressInline ?? false

  // Autopay may not have run yet, so an open invoice is payable by hand regardless.
  if (s.openInvoice && !suppressInline) {
    return {
      kind: "pay_invoice",
      severity: "warn",
      message: "You have an unpaid invoice. Pay it now to keep your relays running.",
    }
  }

  if (methodError) {
    return {
      kind: "update_method",
      severity: "warn",
      message: nwc.kind === "error"
        ? "Your Lightning wallet couldn't be charged. Update your payment method."
        : "Your card couldn't be charged. Update your payment method.",
    }
  }

  if (s.hasPaidSubscription && !hasAutopay && !s.openInvoice && !suppressInline) {
    return {
      kind: "setup_autopay",
      severity: "info",
      message: "Set up automatic payments so your subscription renews without interruption.",
    }
  }

  return null
}

export type AccountStatus = "active" | "inactive" | "delinquent"

// Delinquent once churned, active with a paid relay, a balance or a method, else inactive.
export function accountStatus(s: BillingStatusSnapshot): AccountStatus {
  const tenant = s.tenant
  if (!tenant) return "inactive"
  if (tenant.churned_at) return "delinquent"

  const hasAutopay = autopayConfigured(tenant)
  const hasOpenInvoice = Boolean(s.openInvoice)

  if (s.hasPaidSubscription || hasOpenInvoice || hasAutopay) return "active"
  return "inactive"
}
