import { For } from "solid-js"
import type { PlanId } from "@/lib/api"
import { plans } from "@/lib/state"

function CheckIcon() {
  return (
    <svg class="w-4 h-4 text-blue-500 shrink-0 mt-0.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
      <path d="M20 6L9 17l-5-5" />
    </svg>
  )
}

function XIcon() {
  return (
    <span class="w-4 h-4 shrink-0 mt-0.5 flex items-center justify-center">
      <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="M18 6L6 18M6 6l12 12" />
      </svg>
    </span>
  )
}

function priceLabel(sats: number) {
  if (sats === 0) return "0"
  if (sats >= 1000) return `${(sats / 1000).toLocaleString()}K`
  return sats.toLocaleString()
}

function memberLabel(members: number | null) {
  if (members === null) return "Unlimited members"
  return `Up to ${members} members`
}

type PricingTableProps = {
  selectable?: boolean
  selectedPlan?: PlanId
  onSelect?: (plan: PlanId) => void
  onCta?: (plan: PlanId) => void
}

export default function PricingTable(props: PricingTableProps) {
  return (
    <div class="grid grid-cols-1 lg:grid-cols-3 gap-6 items-start">
      <For each={plans()}>
        {(plan) => {
          const isPopular = plan.id === "basic"
          const isSelected = () => props.selectable && props.selectedPlan === plan.id

          const card = (
            <>
              {isPopular && !props.selectable && (
                <span class="absolute -top-3.5 left-1/2 -translate-x-1/2 bg-blue-600 text-white text-xs font-bold px-4 py-1 rounded-full tracking-wide">
                  POPULAR
                </span>
              )}
              <h3 class="text-lg font-bold text-gray-900 mb-1">{plan.name}</h3>
              <div class="mb-8">
                <span class="text-4xl font-extrabold text-gray-900">{priceLabel(plan.sats)}</span>
                <span class="text-sm text-gray-400 ml-1">sats / mo</span>
              </div>
              <ul class="mb-8 text-sm text-gray-600 space-y-3">
                <li class="flex items-start gap-2"><CheckIcon />{memberLabel(plan.members)}</li>
                <li class={`flex items-start gap-2 ${plan.blossom ? "" : "text-gray-300"}`}>
                  {plan.blossom ? <CheckIcon /> : <XIcon />}
                  Blossom storage
                </li>
                <li class={`flex items-start gap-2 ${plan.livekit ? "" : "text-gray-300"}`}>
                  {plan.livekit ? <CheckIcon /> : <XIcon />}
                  LiveKit video
                </li>
              </ul>
              {!props.selectable && (
                <button
                  type="button"
                  onClick={() => props.onCta?.(plan.id)}
                  class={`w-full text-center py-2.5 px-4 text-sm rounded-xl font-semibold transition-colors ${isPopular ? "bg-blue-600 text-white hover:bg-blue-700" : "border border-gray-200 text-gray-700 hover:bg-gray-50"}`}
                >
                  Get started
                </button>
              )}
            </>
          )

          if (props.selectable) {
            return (
              <button
                type="button"
                onClick={() => props.onSelect?.(plan.id)}
                class={`relative w-full bg-white rounded-2xl p-8 text-left transition-all border-2 ${isSelected() ? "border-blue-600 shadow-lg shadow-blue-100" : "border-gray-200 hover:border-gray-300"}`}
              >
                {card}
              </button>
            )
          }

          return (
            <div class={`relative bg-white rounded-2xl p-8 ${isPopular ? "border-2 border-blue-600 shadow-lg shadow-blue-100" : "border border-gray-200"}`}>
              {card}
            </div>
          )
        }}
      </For>
    </div>
  )
}
