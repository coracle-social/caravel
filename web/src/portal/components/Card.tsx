import type { JSX } from "solid-js"

export default function Card(props: { children: JSX.Element }) {
  return <div class="space-y-6 rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm sm:p-8">{props.children}</div>
}
