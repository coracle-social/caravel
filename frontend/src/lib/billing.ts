import { createMemo } from "solid-js"
import { indexBy } from "@welshman/lib"
import { invoiceStatus, selectPayableInvoice, type Invoice, type Tenant } from "@/lib/api"
import { autopayConfigured, cardState, nwcState } from "@/lib/paymentMethod"
import { autopayBilling, billingDraftInvoice, billingInvoices, billingRelays, billingTenant, plans, refetchBilling } from "@/lib/state"

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

  // The current period's in-progress bill (outstanding items not yet cut into a
  // real invoice), or undefined when nothing is due. Shown as a "draft" row.
  const draftInvoice = () => billingDraftInvoice() ?? undefined

  const openInvoices = createMemo(() =>
    invoices().filter((inv) => invoiceStatus(inv) === "open" && inv.amount > 0),
  )
  // The autopay/dunning target — the same pick autopayBilling collects.
  const openInvoice = () => selectPayableInvoice(invoices())

  // Amount due: the total of all open invoices.
  const balance = () => openInvoices().reduce((sum, inv) => sum + inv.amount, 0)

  const hasPaidSubscription = createMemo(() => {
    const planById = indexBy((p) => p.id, plans())
    return (billingRelays() ?? []).some((relay) => {
      const plan = planById.get(relay.plan_id)
      return Boolean(plan && plan.amount > 0 && relay.status === "active")
    })
  })

  const loading = () => billingTenant.loading || billingInvoices.loading || billingDraftInvoice.loading

  return { tenant, invoices, draftInvoice, balance, openInvoice, hasPaidSubscription, loading, refetch: refetchBilling, autopay: autopayBilling }
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

  const hasAutopay = autopayConfigured(tenant)
  const nwc = nwcState(tenant)
  const card = cardState(tenant)
  const methodError = nwc.kind === "error" || card.kind === "error"
  const suppressInline = opts?.suppressInline ?? false

  // Any open invoice gets a "Pay now" surface, even with autopay configured:
  // autopay may not have fired yet or may have failed without setting an error,
  // and the user still needs a way to pay manually. Only the inline create/upgrade
  // flow (suppressInline) handles its own invoice, so defer to it there.
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

// Coarse account-health summary for the status badge. Pure function of the same
// snapshot `activeBillingPrompt` consumes, so the badge can never disagree with
// the prompt. Mutually exclusive and total:
//   - delinquent: churned_at is set — the ONLY frontend-visible signal of a real
//     suspension (churn_tenant is the single place relays are paused). An open
//     invoice alone is NOT delinquency: the tenant has a 7-day grace window and
//     autopay may simply not have fired yet. Matches the sole severity:"error"
//     branch in activeBillingPrompt.
//   - active: not churned AND there is paid business to keep running — an active
//     paid relay, an open balance, or a configured payment method. A failed method
//     (nwc_error/stripe_error) or an unpaid invoice within grace stays "active";
//     the per-method rows and the inline prompt carry that detail.
//   - inactive: not churned and nothing billable — no paid relay, no balance, no
//     method. The brand-new or free-only tenant (typically billing_anchor == null).
export function accountStatus(s: BillingStatusSnapshot): AccountStatus {
  const tenant = s.tenant
  if (!tenant) return "inactive"
  if (tenant.churned_at) return "delinquent"

  const hasAutopay = autopayConfigured(tenant)
  const hasOpenInvoice = Boolean(s.openInvoice)

  if (s.hasPaidSubscription || hasOpenInvoice || hasAutopay) return "active"
  return "inactive"
}
