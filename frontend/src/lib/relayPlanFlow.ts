// Pure decision module shared by useRelayToggles and RelayNew. No Solid signals,
// no awaits, no effects live here — only the decisions (payload shape, optimistic
// copy shape, the toggle next-relay computation, and the needs-setup -> invoice ->
// setup ladder). The effect layers (mutate, awaits, signal writes, navigate) stay
// in the hook/components that import these. Keeping the decisions pure makes the
// plan-upgrade and relay-creation flows testable and dedupes the post-paid ladder
// that RelayNew and handleUpdatePlan previously inlined separately.

import { flagToBool, boolToFlag } from "@/lib/relayFlags"
import { FREE } from "@/lib/plans"
import type { Invoice, PlanId, Relay, UpdateRelayInput } from "@/lib/api"

// CRITICAL: the returned object is the JSON request payload sent to PUT /relays.
// Keep exactly these field names/values so the wire stays byte-identical to the
// previous inline payload: `{ plan_id }` for paid plans, plus blossom/livekit
// disable flags for free.
export function planUpdatePayload(plan_id: PlanId): UpdateRelayInput {
  if (plan_id === FREE) {
    return { plan_id, blossom_enabled: 0, livekit_enabled: 0 }
  }
  return { plan_id }
}

// The optimistic local copy passed to mutate(). Internal-only type, mirrors the
// payload's effect on the relay row. Free downgrades also clear the paid feature
// flags so the optimistic view matches what the server will persist.
export function applyPlanToRelay(relay: Relay, plan_id: PlanId): Relay {
  if (plan_id === FREE) {
    return { ...relay, plan_id, blossom_enabled: 0, livekit_enabled: 0 }
  }
  return { ...relay, plan_id }
}

// Pure next-relay computation for a single boolean flag toggle, extracted from
// useRelayToggles. The flag is stored as 0/1 on the wire, so flip the derived
// boolean and convert back.
export function toggleField(relay: Relay, field: keyof Relay, fallback: boolean): Relay {
  return { ...relay, [field]: boolToFlag(!flagToBool(relay[field] as number, fallback)) }
}

// Discriminated decision for what to do after a paid create/upgrade succeeds and
// the tenant's payment-setup state has been resolved. Internal/derived — never
// serialized.
export type PaidFlowDecision =
  | { kind: "navigate" }
  | { kind: "pay_invoice"; invoice: Invoice }
  | { kind: "setup" }

// The exact ladder both RelayNew's post-create branch and handleUpdatePlan's
// post-upgrade branch inline: if the tenant already has autopay configured, just
// navigate; otherwise surface the materialized open invoice to pay directly, or
// fall back to payment setup when none is available.
export function decidePostPaidFlow(args: { needsSetup: boolean; invoice: Invoice | null }): PaidFlowDecision {
  if (!args.needsSetup) return { kind: "navigate" }
  if (args.invoice) return { kind: "pay_invoice", invoice: args.invoice }
  return { kind: "setup" }
}
