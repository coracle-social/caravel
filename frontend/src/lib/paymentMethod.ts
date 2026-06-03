import type { InvoiceMethod, Tenant } from "@/lib/api"

// A discriminated view of a single payment method's state, derived AT the
// boundary from the raw tenant fields (nwc_is_set/stripe_payment_method_id and
// the matching *_error). This replaces the ad-hoc per-page MethodState objects
// and the scattered nwc/stripe boolean expressions so the surfaces can't drift.
// No new wire data is emitted: these read the same raw fields the API returns.
export type PaymentMethodState =
  | { kind: "not_set_up" }
  | { kind: "ok" }
  | { kind: "error"; message: string }

export function nwcState(t: Pick<Tenant, "nwc_is_set" | "nwc_error">): PaymentMethodState {
  if (!t.nwc_is_set) return { kind: "not_set_up" }
  if (t.nwc_error) return { kind: "error", message: t.nwc_error }
  return { kind: "ok" }
}

export function cardState(t: Pick<Tenant, "stripe_payment_method_id" | "stripe_error">): PaymentMethodState {
  if (!t.stripe_payment_method_id) return { kind: "not_set_up" }
  // Don't surface Stripe's raw decline/error text to the tenant (it can be noisy
  // or sensitive); show a generic message. The detail stays on the tenant record
  // for admins (see AdminTenantDetail).
  if (t.stripe_error) return { kind: "error", message: "Payment failed" }
  return { kind: "ok" }
}

// True when the tenant has any usable automatic payment method on file.
export function autopayConfigured(
  t: Pick<Tenant, "nwc_is_set" | "stripe_payment_method_id">,
): boolean {
  return t.nwc_is_set || Boolean(t.stripe_payment_method_id)
}

const METHOD_LABELS: Record<InvoiceMethod, string> = {
  nwc: "Lightning",
  stripe: "Card",
  oob: "Lightning",
}

export function methodLabel(m: InvoiceMethod): string {
  return METHOD_LABELS[m]
}
