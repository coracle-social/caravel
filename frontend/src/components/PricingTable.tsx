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

function priceLabel(amount: number) {
  if (amount === 0) return "Free"
  return `$${amount / 100}`
}

function memberLabel(members: number | null) {
  if (members === null) return "Unlimited members"
  return `Up to ${members} members`
}

type PricingTableProps = {
  selectable?: boolean
  selectedPlanId?: PlanId
  onSelect?: (planId: PlanId) => void
  onCta?: (planId: PlanId) => void
}

export default function PricingTable(props: PricingTableProps) {
  // Hidden plans (e.g. the legacy "basic" tier) stay resolvable for billing via
  // the backend but never show up in the public pricing table or plan selector.
  // This is the only place the hidden flag is honored.
  const visiblePlans = () => plans().filter((plan) => !plan.hidden)

  // The full pricing page also shows the synthetic "Custom" card, so the column
  // count tracks the real number of cards (literal classes so Tailwind keeps them).
  const cardCount = () => visiblePlans().length + (props.selectable ? 0 : 1)
  const gridCols: Record<number, string> = {
    1: "lg:grid-cols-1",
    2: "lg:grid-cols-2",
    3: "lg:grid-cols-3",
    4: "lg:grid-cols-4",
  }

  return (
    <div class={`grid grid-cols-1 gap-6 items-start ${gridCols[cardCount()] ?? "lg:grid-cols-4"}`}>
      <For each={visiblePlans()}>
        {(plan) => {
          const isPopular = plan.id === "basic"
          const isSelected = () => props.selectable && props.selectedPlanId === plan.id

          const card = (
            <>
              {isPopular && !props.selectable && (
                <span class="absolute -top-3.5 left-1/2 -translate-x-1/2 bg-blue-600 text-white text-xs font-bold px-4 py-1 rounded-full tracking-wide">
                  POPULAR
                </span>
              )}
              <h3 class="text-lg font-bold text-gray-900 mb-1">{plan.name}</h3>
              <div class="mb-8">
                <span class="text-4xl font-extrabold text-gray-900">{priceLabel(plan.amount)}</span>
                <span class="text-sm text-gray-400 ml-1">/ mo</span>
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
      {!props.selectable && (
        <div class="relative bg-white rounded-2xl p-8 border border-gray-200">
          <h3 class="text-lg font-bold text-gray-900 mb-1">Custom</h3>
          <div class="mb-8">
            <span class="text-4xl font-extrabold text-gray-900">Let's talk</span>
          </div>
          <ul class="mb-8 text-sm text-gray-600 space-y-3">
            <li class="flex items-start gap-2"><CheckIcon />White-labeled app</li>
            <li class="flex items-start gap-2"><CheckIcon />Dedicated support</li>
            <li class="flex items-start gap-2"><CheckIcon />Custom feature development</li>
          </ul>
          <a
            href="https://cal.com/coracle.social/30min"
            target="_blank"
            rel="noopener noreferrer"
            class="block w-full text-center py-2.5 px-4 text-sm rounded-xl font-semibold transition-colors border border-gray-200 text-gray-700 hover:bg-gray-50"
          >
            Contact us
          </a>
        </div>
      )}
    </div>
  )
}
