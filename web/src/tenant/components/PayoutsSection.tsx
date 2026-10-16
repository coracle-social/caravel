import { createResource, Show } from "solid-js"
import { getPayoutStatus, type PayoutStatus } from "@/shared/lib/api"
import { usePayoutRedirect } from "@/shared/lib/usePayoutRedirect"

type PayoutState = "not_set_up" | "incomplete" | "ready"

const payoutStatusStyles: Record<PayoutState, string> = {
  not_set_up: "bg-gray-100 text-gray-500 border-gray-200",
  incomplete: "bg-yellow-50 text-yellow-700 border-yellow-200",
  ready: "bg-green-50 text-green-700 border-green-200",
}

const payoutStatusLabels: Record<PayoutState, string> = {
  not_set_up: "not set up",
  incomplete: "incomplete",
  ready: "ready",
}

function payoutState(status: PayoutStatus): PayoutState {
  if (!status.account_id) return "not_set_up"
  return status.ready ? "ready" : "incomplete"
}

// The tenant's Stripe Connect account, which receives what members pay for
// admission to their relays. Reading it refreshes readiness from Stripe, so
// landing back here from onboarding shows the result.
export default function PayoutsSection(props: { pubkey: string }) {
  const [connect] = createResource(() => props.pubkey, getPayoutStatus)
  const status = () => (connect.error ? undefined : connect())
  const payouts = usePayoutRedirect()

  return (
    <section class="bg-white border border-gray-200 rounded-xl p-6">
      <div class="flex items-center justify-between gap-3 mb-2">
        <h2 class="text-lg font-semibold text-gray-900">Payouts</h2>
        <Show when={status()}>
          {(s) => (
            <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${payoutStatusStyles[payoutState(s())]}`}>
              {payoutStatusLabels[payoutState(s())]}
            </span>
          )}
        </Show>
      </div>
      <p class="text-sm text-gray-500 mb-4">
        Charge members for admission to your relays, and receive what they pay through Stripe
        <Show when={status()}>{(s) => <>, less the platform's fee of {s().fee_percent}%</>}</Show>.
      </p>
      <Show when={connect.error}>
        <p class="text-sm text-red-600 mb-3">Failed to load your payout account.</p>
      </Show>
      <Show when={payouts.error()}>
        <p class="text-sm text-red-600 mb-3">{payouts.error()}</p>
      </Show>
      <Show when={status()}>
        {(s) => (
          <Show
            when={payoutState(s()) === "ready"}
            fallback={
              <button
                type="button"
                onClick={payouts.openSetup}
                disabled={payouts.redirecting()}
                class="inline-flex items-center rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
              >
                {payouts.redirecting() ? "Opening Stripe…" : payoutState(s()) === "incomplete" ? "Finish setting up payouts" : "Set up payouts"}
              </button>
            }
          >
            <button
              type="button"
              onClick={payouts.openDashboard}
              disabled={payouts.redirecting()}
              class="inline-flex items-center rounded-lg border border-gray-300 px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
            >
              {payouts.redirecting() ? "Opening Stripe…" : "View payouts in Stripe"}
            </button>
          </Show>
        )}
      </Show>
    </section>
  )
}
