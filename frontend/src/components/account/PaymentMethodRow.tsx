import { Show } from "solid-js"
import type { PaymentMethodState } from "@/lib/paymentMethod"

// Style/label lookups for a payment method's state, co-located with the row that
// consumes them so any future remodel of PaymentMethodState is a single-file
// change.
const methodStatusStyles: Record<PaymentMethodState["kind"], string> = {
  not_set_up: "bg-gray-100 text-gray-500 border-gray-200",
  ok: "bg-green-50 text-green-700 border-green-200",
  error: "bg-red-50 text-red-700 border-red-200",
}

const methodStatusLabels: Record<PaymentMethodState["kind"], string> = {
  not_set_up: "not set up",
  ok: "ok",
  error: "error",
}

// Presentational payment-method list row (title, optional error line, status
// badge, Set up/Update CTA). Props are reactive only when read lazily, so access
// props.* inside JSX, never destructure at the top.
type PaymentMethodRowProps = {
  title: string
  state: PaymentMethodState
  onAction: () => void
}

export default function PaymentMethodRow(props: PaymentMethodRowProps) {
  return (
    <li class="rounded-lg border border-gray-200 p-4">
      <div class="flex items-center justify-between gap-3">
        <div class="min-w-0">
          <p class="text-sm font-medium text-gray-900">{props.title}</p>
          <Show when={props.state.kind === "error"}>
            <p class="text-xs text-red-600 mt-0.5 break-words">{(props.state as { message: string }).message}</p>
          </Show>
        </div>
        <div class="flex items-center gap-3 flex-shrink-0">
          <span class={`inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium capitalize ${methodStatusStyles[props.state.kind]}`}>
            {methodStatusLabels[props.state.kind]}
          </span>
          <button
            type="button"
            onClick={props.onAction}
            class="text-sm font-medium text-blue-600 hover:text-blue-700 disabled:opacity-50"
          >
            {props.state.kind === "not_set_up" ? "Set up" : "Update"}
          </button>
        </div>
      </div>
    </li>
  )
}
