import { For, Show, createMemo } from "solid-js"
import type { InvoiceItem } from "@/shared/lib/api"
import { formatUsd } from "@/shared/lib/format"

const MAX_VISIBLE_ITEMS = 8

// What an invoice charges for, the first few items of it.
type InvoiceItemsListProps = {
  items: InvoiceItem[]
}

export default function InvoiceItemsList(props: InvoiceItemsListProps) {
  const visibleItems = createMemo(() => props.items.slice(0, MAX_VISIBLE_ITEMS))
  return (
    <div class="rounded-lg border border-gray-200 bg-gray-50 p-3">
      <p class="text-xs font-medium text-gray-500 uppercase tracking-wide mb-2">On this invoice</p>
      <ul class="space-y-1.5">
        <For each={visibleItems()}>
          {(item) => (
            <li class="flex items-center justify-between gap-3 text-sm">
              <span class="truncate text-gray-900">{item.description}</span>
              <span class="flex-shrink-0 text-xs text-gray-500">{formatUsd(item.amount)}</span>
            </li>
          )}
        </For>
        <Show when={props.items.length > MAX_VISIBLE_ITEMS}>
          <li class="text-xs text-gray-500">
            + {props.items.length - MAX_VISIBLE_ITEMS} more
          </li>
        </Show>
      </ul>
    </div>
  )
}
