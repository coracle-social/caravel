export const RELAY_PLAN_IDS = ["free", "basic", "growth"] as const

export type RelayPlanId = (typeof RELAY_PLAN_IDS)[number]
