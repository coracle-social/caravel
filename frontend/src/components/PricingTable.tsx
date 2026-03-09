import { A } from "@solidjs/router"
import { For } from "solid-js"
import { RELAY_PLANS, type RelayPlanId } from "../lib/relayPlans"

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

type PricingTableProps = {
  selectable?: boolean
  selectedPlan?: RelayPlanId
  onSelect?: (plan: RelayPlanId) => void
  ctaHref?: string
  compactOnMobile?: boolean
}

export default function PricingTable(props: PricingTableProps) {
  return (
    <div class={`grid items-start ${props.compactOnMobile ? "grid-cols-3 gap-2 sm:grid-cols-1 sm:gap-6 md:grid-cols-3" : "grid-cols-1 md:grid-cols-3 gap-6"}`}>
      <For each={RELAY_PLANS}>
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
              <h3 class={`font-bold text-gray-900 mb-1 ${props.compactOnMobile ? "text-sm sm:text-lg" : "text-lg"}`}>{plan.label}</h3>
              <p class={`text-gray-400 mb-6 ${props.compactOnMobile ? "hidden sm:block text-sm" : "text-sm"}`}>{plan.subtitle}</p>
              <div class={props.compactOnMobile ? "mb-3 sm:mb-8" : "mb-8"}>
                <span class={`font-extrabold text-gray-900 ${props.compactOnMobile ? "text-xl sm:text-4xl" : "text-4xl"}`}>{plan.priceLabel}</span>
                <span class={`text-gray-400 ml-1 ${props.compactOnMobile ? "text-[10px] sm:text-sm" : "text-sm"}`}>sats / mo</span>
              </div>
              <ul class={`mb-8 text-sm text-gray-600 ${props.compactOnMobile ? "hidden sm:block space-y-3" : "space-y-3"}`}>
                <li class="flex items-start gap-2"><CheckIcon />{plan.memberLabel}</li>
                <li class={`flex items-start gap-2 ${plan.blossom ? "" : "text-gray-300"}`}>
                  {plan.blossom ? <CheckIcon /> : <XIcon />}
                  Blossom storage
                </li>
                <li class={`flex items-start gap-2 ${plan.livekit ? "" : "text-gray-300"}`}>
                  {plan.livekit ? <CheckIcon /> : <XIcon />}
                  LiveKit video
                </li>
              </ul>
              {props.compactOnMobile && props.selectable && (
                <div class="sm:hidden mb-3 text-[10px] text-gray-500 space-y-1">
                  <div>{plan.memberLabel.replace(" members", "")}</div>
                  <div class="flex items-center gap-1">
                    <span class={plan.blossom ? "text-blue-600" : "text-gray-300"}>{plan.blossom ? "✓" : "✕"}</span>
                    Blossom
                  </div>
                  <div class="flex items-center gap-1">
                    <span class={plan.livekit ? "text-blue-600" : "text-gray-300"}>{plan.livekit ? "✓" : "✕"}</span>
                    LiveKit
                  </div>
                </div>
              )}
              {!props.selectable && (
                <A
                  href={props.ctaHref ?? "/relays/new"}
                  class={`block text-center rounded-xl font-semibold transition-colors ${props.compactOnMobile ? "py-1.5 px-2 text-[11px] sm:py-2.5 sm:px-4 sm:text-sm" : "py-2.5 px-4 text-sm"} ${isPopular ? "bg-blue-600 text-white hover:bg-blue-700" : "border border-gray-200 text-gray-700 hover:bg-gray-50"}`}
                >
                  Get started
                </A>
              )}
            </>
          )

          if (props.selectable) {
            return (
              <button
                type="button"
                onClick={() => props.onSelect?.(plan.id)}
                class={`relative w-full bg-white rounded-2xl text-left transition-all ${props.compactOnMobile ? "p-3 sm:p-8" : "p-8"} ${isSelected() ? "border-2 border-blue-600 shadow-lg shadow-blue-100" : "border border-gray-200 hover:border-gray-300"}`}
              >
                {card}
              </button>
            )
          }

          return (
            <div class={`relative bg-white rounded-2xl ${props.compactOnMobile ? "p-3 sm:p-8" : "p-8"} ${isPopular ? "border-2 border-blue-600 shadow-lg shadow-blue-100" : "border border-gray-200"}`}>
              {card}
            </div>
          )
        }}
      </For>
    </div>
  )
}
