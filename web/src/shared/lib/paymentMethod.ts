import type { InvoiceMethod, Tenant } from "@/shared/lib/api"

// Whether one of a tenant's payment methods is set up, and whether it last failed.
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
  // Stripe's own decline text is for admins, who see it on the tenant's page.
  if (t.stripe_error) return { kind: "error", message: "Payment failed" }
  return { kind: "ok" }
}

// Whether the tenant has a method to pay automatically with.
export function autopayConfigured(
  t: Pick<Tenant, "nwc_is_set" | "stripe_payment_method_id">,
): boolean {
  return t.nwc_is_set || Boolean(t.stripe_payment_method_id)
}

const METHOD_LABELS: Record<InvoiceMethod, string> = {
  nwc: "Lightning",
  stripe: "Card",
  oob: "Lightning",
  comp: "Complimentary",
}

export function methodLabel(m: InvoiceMethod): string {
  return METHOD_LABELS[m]
}
