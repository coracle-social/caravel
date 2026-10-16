import { createEffect, createResource, createSignal, on, Show } from "solid-js"
import Modal from "@/shared/components/Modal"
import { account } from "@/shared/lib/account"
import { getPayoutStatus, openPayoutOnboarding, updateRelay, type MemberInterval, type Relay } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"
import { loaded } from "@/shared/lib/resource"
import { useRedirect } from "@/shared/lib/useRedirect"

type Props = {
  open: boolean
  onClose: () => void
  relay: () => Relay
  onSaved: (relay: Relay) => void
}

// Stripe's minimum charge, in cents.
const MIN_PRICE = 50

export default function MemberPriceModal(props: Props) {
  const [connect] = createResource(() => props.open && props.relay().tenant_pubkey, getPayoutStatus)
  const stripe = useRedirect()
  const [price, setPrice] = createSignal("")
  const [memberInterval, setMemberInterval] = createSignal<MemberInterval>("month")
  const [saving, setSaving] = createSignal(false)
  const [error, setError] = createSignal("")

  const cents = () => Math.round(Number(price()) * 100)
  const priceValid = () => price().trim() !== "" && Number.isFinite(cents()) && cents() >= MIN_PRICE
  const isPaid = () => props.relay().member_price > 0
  const isOwner = () => account()?.pubkey === props.relay().tenant_pubkey
  const publicJoin = () => props.relay().policy_public_join === 1

  createEffect(on(() => props.open, (open) => {
    if (!open) return

    const relay = props.relay()
    setPrice(relay.member_price > 0 ? (relay.member_price / 100).toFixed(2) : "")
    setMemberInterval(relay.member_interval)
    setError("")
  }))

  async function save(memberPrice: number) {
    setSaving(true)
    setError("")
    try {
      props.onSaved(await updateRelay(props.relay().id, { member_price: memberPrice, member_interval: memberInterval() }))
      props.onClose()
    } catch (e) {
      setError(formatError(e, "Failed to save price"))
    } finally {
      setSaving(false)
    }
  }

  function handleSubmit(e: Event) {
    e.preventDefault()
    void save(cents())
  }

  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      wrapperClass="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      panelClass="w-full max-w-lg bg-white rounded-2xl shadow-xl p-6 space-y-5"
    >
      <div class="flex items-center justify-between">
        <h2 class="text-lg font-semibold text-gray-900">Member price</h2>
        <button
          type="button"
          onClick={props.onClose}
          class="text-gray-400 hover:text-gray-600 text-xl leading-none"
          aria-label="Close"
        >
          ×
        </button>
      </div>

      <p class="text-sm text-gray-500">
        Charge each member a recurring fee for admission. Members pay through Stripe, and the owner receives what they pay, less the platform's fee
        <Show when={loaded(connect)}>{(status) => <> of {status().fee_percent}%</>}</Show>.
      </p>

      <Show when={!connect.loading} fallback={<p class="text-sm text-gray-500">Checking payouts…</p>}>
        <Show when={!connect.error} fallback={<p class="text-sm text-red-600">Failed to check payouts.</p>}>
          <Show
            when={connect()?.ready}
            fallback={
              <div class="rounded-lg border border-yellow-200 bg-yellow-50 px-4 py-3 space-y-3">
                <Show
                  when={isOwner()}
                  fallback={<p class="text-sm text-yellow-800">The owner of this relay has not finished setting up payouts.</p>}
                >
                  <p class="text-sm text-yellow-800">Set up payouts with Stripe before charging members.</p>
                  <Show when={stripe.error()}>
                    <p class="text-sm text-red-600">{stripe.error()}</p>
                  </Show>
                  <button
                    type="button"
                    onClick={() => stripe.go(() => openPayoutOnboarding(account()!.pubkey, window.location.href), "Failed to open Stripe")}
                    disabled={stripe.redirecting()}
                    class="inline-flex items-center rounded-lg bg-yellow-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-yellow-700 disabled:opacity-50"
                  >
                    {stripe.redirecting() ? "Opening Stripe…" : connect()?.account_id ? "Finish setting up payouts" : "Set up payouts"}
                  </button>
                </Show>
              </div>
            }
          >
            <form onSubmit={handleSubmit} class="space-y-4">
              <div class="flex gap-3">
                <div class="flex-1">
                  <label class="block text-sm font-medium text-gray-700 mb-1">Price (USD)</label>
                  <div class="flex items-center border border-gray-300 rounded-lg overflow-hidden focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500">
                    <span class="px-3 py-2 bg-gray-50 text-gray-500 text-sm border-r border-gray-300">$</span>
                    <input
                      type="number"
                      min="0.50"
                      step="0.01"
                      inputmode="decimal"
                      placeholder="5.00"
                      value={price()}
                      onInput={(e) => setPrice(e.currentTarget.value)}
                      class="flex-1 min-w-0 px-3 py-2 text-sm focus:outline-none"
                    />
                  </div>
                </div>
                <div>
                  <label class="block text-sm font-medium text-gray-700 mb-1">Billed</label>
                  <select
                    value={memberInterval()}
                    onChange={(e) => setMemberInterval(e.currentTarget.value === "year" ? "year" : "month")}
                    class="rounded-lg border border-gray-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
                  >
                    <option value="month">Monthly</option>
                    <option value="year">Yearly</option>
                  </select>
                </div>
              </div>
              <Show when={publicJoin()}>
                <p class="rounded-lg border border-yellow-200 bg-yellow-50 px-3 py-2 text-sm text-yellow-800">
                  Anyone can join this relay for free. Turn off public join before charging for admission.
                </p>
              </Show>
              <p class="text-xs text-gray-500">
                The minimum price is $0.50. A new price applies to new members, and existing members keep paying the price they signed up at.
              </p>
              <button
                type="submit"
                disabled={saving() || !priceValid() || publicJoin()}
                class="inline-flex items-center rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
              >
                {saving() ? "Saving…" : "Save price"}
              </button>
            </form>
          </Show>
        </Show>
      </Show>

      <Show when={error()}>
        <p class="text-sm text-red-600">{error()}</p>
      </Show>

      <Show when={isPaid()}>
        <div class="border-t border-gray-200 pt-4 space-y-2">
          <p class="text-xs text-gray-500">
            Making this relay free stops every member's subscription at once, without a refund, and keeps them on the relay.
          </p>
          <button
            type="button"
            onClick={() => void save(0)}
            disabled={saving()}
            class="inline-flex items-center rounded-lg border border-red-200 px-3 py-2 text-sm font-medium text-red-600 hover:bg-red-50 disabled:opacity-50"
          >
            Make this relay free
          </button>
        </div>
      </Show>
    </Modal>
  )
}
