export const RELAY_PLAN_IDS = ["free", "basic", "growth"] as const

export type RelayPlanId = (typeof RELAY_PLAN_IDS)[number]

export const RELAY_PLANS = [
  {
    id: "free",
    label: "Free",
    subtitle: "Get started, no commitment.",
    price: 0,
    priceLabel: "0",
    memberLabel: "Up to 10 members",
    blossom: false,
    livekit: false,
  },
  {
    id: "basic",
    label: "Basic",
    subtitle: "For growing communities.",
    price: 10_000,
    priceLabel: "10K",
    memberLabel: "Up to 100 members",
    blossom: true,
    livekit: true,
  },
  {
    id: "growth",
    label: "Growth",
    subtitle: "For large-scale communities.",
    price: 50_000,
    priceLabel: "50K",
    memberLabel: "Unlimited members",
    blossom: true,
    livekit: true,
  },
] as const satisfies readonly {
  id: RelayPlanId
  label: string
  subtitle: string
  price: number
  priceLabel: string
  memberLabel: string
  blossom: boolean
  livekit: boolean
}[]
