import { For } from "solid-js"

export type Tab<T extends string> = {
  value: T
  label: string
  disabled?: boolean
}

// A row of tabs, one of them selected.
export default function SegmentedTabs<T extends string>(props: { tabs: Tab<T>[]; selected: T; onSelect: (value: T) => void }) {
  return (
    <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
      <For each={props.tabs}>
        {(tab) => (
          <button
            type="button"
            disabled={tab.disabled}
            class={`flex-1 rounded-md px-3 py-2 text-sm transition-colors ${props.selected === tab.value ? "bg-gray-900 text-white" : "text-gray-700 hover:bg-gray-50"}`}
            onClick={() => props.onSelect(tab.value)}
          >
            {tab.label}
          </button>
        )}
      </For>
    </div>
  )
}
