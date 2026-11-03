import { createResource, createSignal, For, Show } from "solid-js"
import MemberPriceModal from "@/shared/components/relay/MemberPriceModal"
import StatusBadge from "@/shared/components/StatusBadge"
import { getPayoutStatus, listRelayMemberships, relayPortalUrl, type Relay } from "@/shared/lib/api"
import { copyToClipboard } from "@/shared/lib/clipboard"
import { payoutReason } from "@/shared/lib/payouts"
import { loaded } from "@/shared/lib/resource"
import { formatDate, formatUsd } from "@/shared/lib/format"
import { shortenNpub } from "@/shared/lib/pubkey"

type Props = {
  relay: () => Relay
  mutateRelay: (relay: Relay) => void
}

// What a relay charges its members, where they pay, and who is paying.
export default function MemberPayments(props: Props) {
  const [modalOpen, setModalOpen] = createSignal(false)
  const [memberships] = createResource(() => props.relay().id, listRelayMemberships)
  const [connect] = createResource(() => props.relay().tenant_pubkey, getPayoutStatus)

  const paying = () => (loaded(memberships) ?? []).filter(m => m.status === "active" || m.status === "canceled")
  const isPaid = () => props.relay().member_price > 0
  const payUrl = () => relayPortalUrl(props.relay())
  // A relay can charge members whether or not its tenant can still be paid.
  const unpaid = () => isPaid() && connect()?.ready === false

  return (
    <div>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-6">Member Payments</h3>
      <div class="space-y-4">
        <div class="flex items-center justify-between gap-2">
          <Show when={isPaid()} fallback={<span class="text-gray-400 text-sm">Members don't pay</span>}>
            <span class="font-medium text-gray-900">{formatUsd(props.relay().member_price)} per {props.relay().member_interval}</span>
          </Show>
          <button
            class="inline-flex items-center rounded-lg border border-gray-200 px-3 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
            onClick={() => setModalOpen(true)}
          >
            {isPaid() ? "Change price" : "Set price"}
          </button>
        </div>
        <Show when={unpaid()}>
          <p class="rounded-lg border border-yellow-200 bg-yellow-50 px-4 py-3 text-sm text-yellow-800">
            Members can't pay until you finish setting up payouts
            <Show when={payoutReason(connect())}>{(reason) => <>. {reason()}</>}</Show>
          </p>
        </Show>
        <Show when={isPaid()}>
          <div class="flex items-center justify-between gap-2">
            <a href={payUrl()} target="_blank" rel="noopener" class="text-sm text-blue-600 hover:underline break-all">
              {payUrl()}
            </a>
            <button
              type="button"
              onClick={() => void copyToClipboard(payUrl(), { successMessage: "Membership link copied" })}
              class="text-sm font-medium text-blue-600 hover:text-blue-700 flex-shrink-0"
            >
              Copy
            </button>
          </div>
        </Show>
        <Show when={paying().length > 0}>
          <ul class="divide-y divide-gray-100 rounded-lg border border-gray-200 px-4">
            <For each={paying()}>
              {(membership) => (
                <li class="flex items-center justify-between gap-3 py-2">
                  <Show when={membership.pubkey} fallback={<span class="text-sm text-gray-500">Unused invite</span>}>
                    {(pubkey) => <span class="font-mono text-sm text-gray-700">{shortenNpub(pubkey())}</span>}
                  </Show>
                  <div class="flex items-center gap-3">
                    <Show when={membership.current_period_end}>
                      {(end) => (
                        <span class="text-xs text-gray-500">
                          {membership.status === "canceled" ? "Ends" : "Renews"} {formatDate(end())}
                        </span>
                      )}
                    </Show>
                    <StatusBadge status={membership.status} />
                  </div>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>

      <MemberPriceModal
        open={modalOpen()}
        onClose={() => setModalOpen(false)}
        relay={props.relay}
        onSaved={props.mutateRelay}
      />
    </div>
  )
}
