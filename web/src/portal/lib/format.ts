import type { Interval } from "@/portal/lib/api"

export const formatPrice = (cents: number, interval: Interval) =>
  `$${(cents / 100).toFixed(2)} / ${interval}`

export const formatDate = (secs: number) =>
  new Date(secs * 1000).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" })
