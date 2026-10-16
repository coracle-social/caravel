import { Show } from "solid-js"
import StatusBadge from "@/shared/components/StatusBadge"
import type { PaymentMethodState } from "@/shared/lib/paymentMethod"

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
          <Show when={props.state.kind === "error" && props.state}>
            {(failed) => <p class="text-xs text-red-600 mt-0.5 break-words">{failed().message}</p>}
          </Show>
        </div>
        <div class="flex items-center gap-3 flex-shrink-0">
          <StatusBadge status={props.state.kind} />
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
