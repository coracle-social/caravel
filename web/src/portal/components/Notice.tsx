import type { JSX } from "solid-js"

const TONES = {
  success: "border-emerald-200 bg-emerald-50 text-emerald-900",
  warning: "border-amber-200 bg-amber-50 text-amber-900",
  error: "border-red-200 bg-red-50 text-red-800",
  neutral: "border-zinc-200 bg-zinc-50 text-zinc-700",
}

export default function Notice(props: { tone: keyof typeof TONES; children: JSX.Element }) {
  return <div class={`space-y-2 rounded-xl border px-4 py-3 text-sm ${TONES[props.tone]}`}>{props.children}</div>
}
