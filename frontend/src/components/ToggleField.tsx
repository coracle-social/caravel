import type { JSX } from "solid-js"

export default function ToggleField(props: { label: string; children: JSX.Element }) {
  return (
    <div class="flex items-center justify-between gap-3">
      <dt class="text-xs font-medium text-gray-500 uppercase tracking-wide">{props.label}</dt>
      <dd class="text-sm text-gray-900">{props.children}</dd>
    </div>
  )
}
