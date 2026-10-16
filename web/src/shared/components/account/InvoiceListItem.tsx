import { Show } from "solid-js"
import StatusBadge from "@/shared/components/StatusBadge"
import { formatUsd } from "@/shared/lib/format"

// An invoice, or this period's draft, in the tenant's payment history.
type InvoiceListItemProps = {
  amount: number
  status: string
  periodLabel: string
  method?: string
  isDraft?: boolean
  isOpen?: boolean
  onPay?: () => void
  onPrintPdf: () => void
  printing: boolean
}

export default function InvoiceListItem(props: InvoiceListItemProps) {
  return (
    <li class={`rounded-lg border p-4 text-sm ${props.isDraft ? "border-dashed border-gray-300" : "border-gray-200"}`}>
      <div class="flex items-center justify-between gap-3">
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <span class="font-medium text-gray-900">{formatUsd(props.amount)}</span>
            <StatusBadge status={props.status} />
            <Show when={props.method}>
              <span class="text-xs text-gray-500">· paid via {props.method}</span>
            </Show>
          </div>
          <Show when={props.periodLabel}>
            <p class="text-xs text-gray-500 mt-0.5">{props.periodLabel}</p>
          </Show>
          <Show when={props.isDraft}>
            <p class="text-xs text-gray-400 mt-1">Charges accruing this period. You'll be invoiced once a balance is due.</p>
          </Show>
        </div>
        <div class="flex items-center gap-2 flex-shrink-0">
          <Show when={props.isOpen}>
            <button
              type="button"
              onClick={() => props.onPay?.()}
              class="py-1 px-3 bg-blue-600 text-white text-xs font-medium rounded-lg hover:bg-blue-700 transition-colors"
            >
              Pay now
            </button>
          </Show>
          <button
            type="button"
            onClick={props.onPrintPdf}
            disabled={props.printing}
            class="text-xs font-medium text-gray-500 hover:text-gray-800 underline disabled:opacity-50"
          >
            PDF
          </button>
        </div>
      </div>
    </li>
  )
}
