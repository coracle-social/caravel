import type { PlanId } from "@/shared/lib/api"

// Permanent billing keys, never reused or repriced; the catalog is crates/core/src/models/plans.rs.
export const FREE: PlanId = "free"
export const BASIC: PlanId = "basic"

export function priceLabel(amount: number) {
  if (amount === 0) return "Free"
  return `$${amount / 100}`
}

export function memberLabel(members: number | null) {
  if (members === null) return "Unlimited members"
  return `Up to ${members} members`
}
