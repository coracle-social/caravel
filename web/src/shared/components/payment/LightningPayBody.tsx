import { Show } from "solid-js"

type Bolt11Status = "idle" | "loading" | "ready" | "error"

// Presentational lightning payment body: loading/error/ready states, the bolt11
// QR + input, and copy. All fetching/QR generation stays in PaymentDialog and is
// surfaced via accessors/callbacks. Props are reactive only when read lazily, so
// access props.* inside JSX, never destructure signal-bearing props at the top.
type LightningPayBodyProps = {
  bolt11Status: () => Bolt11Status
  bolt11: () => string
  qrDataUrl: () => string
  bolt11Error: () => string
  onRetry: () => void
  onCopy: () => void
}

export default function LightningPayBody(props: LightningPayBodyProps) {
  return (
    <>
      <Show when={props.bolt11Status() === "idle" || props.bolt11Status() === "loading"}>
        <div class="flex items-center justify-center py-12 text-sm text-gray-400">Generating invoice...</div>
      </Show>
      <Show when={props.bolt11Status() === "error"}>
        <div class="rounded-lg border border-red-200 bg-red-50 p-4">
          <p class="text-sm font-medium text-red-700">Unable to generate invoice</p>
          <p class="mt-1 text-xs text-red-600 wrap-break-word">{props.bolt11Error()}</p>
          <button
            type="button"
            onClick={props.onRetry}
            class="mt-3 inline-flex items-center rounded-lg bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-700"
          >
            Retry
          </button>
        </div>
      </Show>
      <Show when={props.bolt11Status() === "ready"}>
        <img src={props.qrDataUrl()} alt="Lightning invoice QR code" class="mx-auto rounded-lg" />
        <Show when={props.bolt11()}>
          <div class="flex rounded-lg border border-gray-300">
            <input
              type="text"
              readOnly
              value={props.bolt11()}
              class="min-w-0 flex-1 rounded-l-lg border-0 px-3 py-2 text-xs text-gray-500 bg-transparent focus:outline-none"
            />
            <button
              type="button"
              class="flex items-center px-3 text-gray-400 hover:text-gray-700"
              onClick={props.onCopy}
              title="Copy invoice"
            >
              <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <rect x="9" y="9" width="13" height="13" rx="2" />
                <path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1" />
              </svg>
            </button>
          </div>
        </Show>
        <p class="text-xs text-gray-500 text-center">
          Scan this QR code with a Bitcoin Lightning wallet to pay.
        </p>
      </Show>
    </>
  )
}
