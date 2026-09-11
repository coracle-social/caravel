import type { PlanId } from "@/lib/api"

// Plan ids are permanent billing keys: past charges are priced by resolving the id
// recorded on them, so an id is never reused or repriced. Mirrored in the
// backend's models.rs.
export const FREE: PlanId = "free"
export const BASIC: PlanId = "basic"
export const GROWTH: PlanId = "growth"
