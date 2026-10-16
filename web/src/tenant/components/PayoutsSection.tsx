import { createResource, Show } from "solid-js"
import StatusBadge from "@/shared/components/StatusBadge"
import { getPayoutStatus, openPayoutDashboard, openPayoutOnboarding, type PayoutStatus } from "@/shared/lib/api"
import { loaded } from "@/shared/lib/resource"
import { useRedirect } from "@/shared/lib/useRedirect"

type PayoutState = "not_set_up" | "incomplete" | "ready"

function payoutState(status: PayoutStatus): PayoutState {
  if (!status.account_id) return "not_set_up"
  return status.ready ? "ready" : "incomplete"
}

// The tenant's Stripe account for what members pay, whose readiness each read refreshes.
export default function PayoutsSection(props: { pubkey: string }) {
  const [connect] = createResource(() => props.pubkey, getPayoutStatus)
  const status = () => loaded(connect)
  const stripe = useRedirect()

  const openSetup = () => stripe.go(() => openPayoutOnboarding(props.pubkey, window.location.href), "Failed to open Stripe")
  const openDashboard = () => stripe.go(() => openPayoutDashboard(props.pubkey), "Failed to open Stripe")

  return (
    <section class="bg-white border border-gray-200 rounded-xl p-6">
      <div class="flex items-center justify-between gap-3 mb-2">
        <h2 class="text-lg font-semibold text-gray-900">Payouts</h2>
        <Show when={status()}>
          {(s) => <StatusBadge status={payoutState(s())} />}
        </Show>
      </div>
      <p class="text-sm text-gray-500 mb-4">
        Charge members for admission to your relays, and receive what they pay through Stripe
        <Show when={status()}>{(s) => <>, less the platform's fee of {s().fee_percent}%</>}</Show>.
      </p>
      <Show when={connect.error}>
        <p class="text-sm text-red-600 mb-3">Failed to load your payout account.</p>
      </Show>
      <Show when={stripe.error()}>
        <p class="text-sm text-red-600 mb-3">{stripe.error()}</p>
      </Show>
      <Show when={status()}>
        {(s) => (
          <Show
            when={payoutState(s()) === "ready"}
            fallback={
              <button
                type="button"
                onClick={openSetup}
                disabled={stripe.redirecting()}
                class="inline-flex items-center rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
              >
                {stripe.redirecting() ? "Opening Stripe…" : payoutState(s()) === "incomplete" ? "Finish setting up payouts" : "Set up payouts"}
              </button>
            }
          >
            <button
              type="button"
              onClick={openDashboard}
              disabled={stripe.redirecting()}
              class="inline-flex items-center rounded-lg border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
            >
              {stripe.redirecting() ? "Opening Stripe…" : "View payouts in Stripe"}
            </button>
          </Show>
        )}
      </Show>
    </section>
  )
}
