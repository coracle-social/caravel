import { For, Show } from "solid-js"
import type { Activity } from "@/lib/hooks"

const ACTIVITY_LABELS: Record<string, string> = {
  create_relay:                   "Relay created",
  update_relay:                   "Relay updated",
  deactivate_relay:               "Relay deactivated",
  fail_relay_sync:                "Relay sync failed",
  create_tenant:                  "Account created",
  update_tenant_billing_anchor:   "Billing anchor updated",
  update_tenant_nwc_url:          "Wallet connection updated",
  create_invoice:                 "Invoice created",
  mark_invoice_paid:              "Invoice paid",
  mark_invoice_attempted:         "Invoice payment attempted",
  mark_invoice_sent:              "Invoice sent",
  mark_invoice_closed:            "Invoice closed",
  mark_relay_synced:              "Relay synchronized",
}

function formatDate(ts: number) {
  return new Date(ts * 1000).toLocaleString(undefined, {
    month: "short", day: "numeric", year: "numeric",
    hour: "2-digit", minute: "2-digit",
  })
}

export default function ActivityFeed(props: { activity: Activity[]; loading: boolean }) {
  return (
    <section class="bg-white border border-gray-200 rounded-xl p-6">
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-6">Activity</h3>
      <Show when={props.loading}>
        <p class="text-sm text-gray-400">Loading activity...</p>
      </Show>
      <Show when={!props.loading && props.activity.length === 0}>
        <p class="text-sm text-gray-400">No activity yet.</p>
      </Show>
      <Show when={!props.loading && props.activity.length > 0}>
        <ol class="relative border-l border-gray-200 space-y-6 ml-3">
          <For each={props.activity}>
            {(item) => (
              <li class="pl-6">
                <span class="absolute -left-1.5 mt-1.5 h-3 w-3 rounded-full border-2 border-white bg-gray-300" />
                <p class="text-sm font-medium text-gray-900">
                  {ACTIVITY_LABELS[item.activity_type] ?? item.activity_type.replace(/_/g, " ")}
                </p>
                <time class="text-xs text-gray-400">{formatDate(item.created_at)}</time>
              </li>
            )}
          </For>
        </ol>
      </Show>
    </section>
  )
}
