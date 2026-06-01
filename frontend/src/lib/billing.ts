import { createMemo, createSignal } from "solid-js"
import { invoiceStatus, type Invoice, type Tenant } from "@/lib/api"
import { billingInvoices, billingRelays, billingTenant, plans, refetchBilling } from "@/lib/state"

// Set while the create/upgrade flow drives its own payment/setup modals, so the
// shared prompt surface doesn't double-prompt for the same invoice. Cleared on
// every close path of that flow.
export const [billingFlowActive, setBillingFlowActive] = createSignal(false)

export type BillingPromptKind = "churned" | "pay_invoice" | "update_method" | "setup_autopay"

export type BillingPrompt = {
  kind: BillingPromptKind
  severity: "error" | "warn" | "info"
  message: string
}

export type BillingStatusSnapshot = {
  tenant: Tenant | undefined
  openInvoice: Invoice | undefined
  hasPaidSubscription: boolean
}

// The single billing read shared by the dashboard shell and the billing page.
// `openInvoice` is the OLDEST open, positive invoice — matching the backend's
// dunning order so the UI pays the same one collection targets.
export function useBillingStatus() {
  const tenant = () => billingTenant()
  const invoices = () => billingInvoices() ?? []

  const openInvoices = createMemo(() =>
    invoices()
      .filter((inv) => invoiceStatus(inv) === "open" && inv.amount > 0)
      .sort((a, b) => a.created_at - b.created_at),
  )
  const openInvoice = () => openInvoices()[0]

  // Amount due: the total of all open invoices.
  const balance = () => openInvoices().reduce((sum, inv) => sum + inv.amount, 0)

  const hasPaidSubscription = createMemo(() => {
    const planById = new Map(plans().map((p) => [p.id, p]))
    return (billingRelays() ?? []).some((relay) => {
      const plan = planById.get(relay.plan_id)
      return Boolean(plan && plan.amount > 0 && relay.status === "active")
    })
  })

  const loading = () => billingTenant.loading || billingInvoices.loading

  return { tenant, invoices, balance, openInvoice, hasPaidSubscription, loading, refetch: refetchBilling }
}

// Pure priority selector: returns the single highest-priority billing prompt to
// surface, or null. Priority: churned > pay an open invoice > fix a failed method
// > set up autopay. `suppressInline` hides the prompts the create/upgrade inline
// flow already handles (pay_invoice, setup_autopay) while still surfacing churn
// and method errors.
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

  const autopayConfigured = tenant.nwc_is_set || Boolean(tenant.stripe_payment_method_id)
  const methodError = tenant.nwc_error ?? tenant.stripe_error
  const suppressInline = opts?.suppressInline ?? false

  if (s.openInvoice && !suppressInline && (!autopayConfigured || methodError)) {
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
      message: tenant.nwc_error
        ? "Your Lightning wallet couldn't be charged. Update your payment method."
        : "Your card couldn't be charged. Update your payment method.",
    }
  }

  if (s.hasPaidSubscription && !autopayConfigured && !s.openInvoice && !suppressInline) {
    return {
      kind: "setup_autopay",
      severity: "info",
      message: "Set up automatic payments so your subscription renews without interruption.",
    }
  }

  return null
}
