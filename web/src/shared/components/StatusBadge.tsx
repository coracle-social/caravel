import type { JSX } from "solid-js"

const GREEN = "bg-green-50 text-green-700 border-green-200"
const GRAY = "bg-gray-100 text-gray-500 border-gray-200"
const RED = "bg-red-50 text-red-700 border-red-200"
const YELLOW = "bg-yellow-50 text-yellow-700 border-yellow-200"

// Every badge's colors, for relays, tenants, invoices, memberships, payouts and payment methods.
const STATUS_STYLES: Record<string, string> = {
  active: GREEN,
  inactive: GRAY,
  verified: GREEN,
  delinquent: RED,
  purged: RED,
  banned: "bg-gray-800 text-gray-100 border-gray-800",
  canceled: YELLOW,
  draft: "bg-blue-50 text-blue-700 border-blue-200",
  open: YELLOW,
  paid: GREEN,
  void: GRAY,
  not_set_up: GRAY,
  incomplete: YELLOW,
  ready: GREEN,
  ok: GREEN,
  error: RED,
}

// A status as a colored pill, labeled with the status unless given children.
export default function StatusBadge(props: { status: string; children?: JSX.Element }) {
  return (
    <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${STATUS_STYLES[props.status] ?? GRAY}`}>
      {props.children ?? props.status.replace(/_/g, " ")}
    </span>
  )
}
