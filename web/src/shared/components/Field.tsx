import type { JSX } from "solid-js"

export default function Field(props: { label: string; children: JSX.Element }) {
  return (
    <div>
      <dt class="text-xs font-medium text-gray-500 uppercase tracking-wide">{props.label}</dt>
      <dd class="mt-0.5 text-sm text-gray-900">{props.children}</dd>
    </div>
  )
}
