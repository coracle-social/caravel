import { Show } from "solid-js"

type SignerTab = "qr" | "paste"

// Presentational NIP-46 signer panel. No signers are created here; QR/URI are
// generated in Login.tsx and passed down, and actions are surfaced as callbacks.
// Props are reactive only when read lazily, so access props.* inside JSX, never
// destructure signal-bearing props at the top.
type LoginSignerScreenProps = {
  signerTab: () => SignerTab
  setSignerTab: (tab: SignerTab) => void
  qrDataUrl: () => string
  nostrConnectUri: () => string
  bunkerUrl: () => string
  setBunkerUrl: (value: string) => void
  loading: () => boolean
  onBack: () => void
  onCopyUri: () => void
  onScan: () => void
  onConnectBunker: () => void
}

export default function LoginSignerScreen(props: LoginSignerScreenProps) {
  return (
    <>
      <button
        type="button"
        class="flex items-center gap-1 text-sm text-gray-500 hover:text-gray-900"
        onClick={props.onBack}
      >
        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5M12 19l-7-7 7-7"/></svg>
        Back
      </button>
      <h2 class="mt-3 text-lg font-semibold text-gray-900">Log in with signer</h2>
      <div class="mt-4 space-y-4">
        <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm ${props.signerTab() === "qr" ? "bg-gray-900 text-white" : "text-gray-700"}`}
            onClick={() => props.setSignerTab("qr")}
          >
            Use QR Code
          </button>
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm ${props.signerTab() === "paste" ? "bg-gray-900 text-white" : "text-gray-700"}`}
            onClick={() => props.setSignerTab("paste")}
          >
            Paste Link
          </button>
        </div>

        <Show when={props.signerTab() === "qr"}>
          <Show when={props.qrDataUrl()} fallback={
            <div class="flex items-center justify-center py-8 text-sm text-gray-400">
              {props.loading() ? "Generating..." : "Loading QR code..."}
            </div>
          }>
            <img src={props.qrDataUrl()} alt="Nostrconnect QR code" class="mx-auto rounded-lg" />
          </Show>
          <div class="flex rounded-lg border border-gray-300">
            <input
              type="text"
              readOnly
              value={props.nostrConnectUri()}
              class="min-w-0 flex-1 rounded-l-lg border-0 px-3 py-2 text-xs text-gray-500 bg-transparent focus:outline-none"
            />
            <button
              type="button"
              class="flex items-center px-3 text-gray-400 hover:text-gray-700"
              onClick={props.onCopyUri}
              title="Copy link"
            >
              <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
            </button>
          </div>
        </Show>

        <Show when={props.signerTab() === "paste"}>
          <div class="flex rounded-lg border border-gray-300">
            <input
              value={props.bunkerUrl()}
              onInput={(e) => props.setBunkerUrl(e.currentTarget.value)}
              placeholder="bunker://..."
              class="min-w-0 flex-1 rounded-l-lg border-0 px-3 py-2 text-sm bg-transparent focus:outline-none"
            />
            <button
              type="button"
              class="flex items-center px-3 text-gray-400 hover:text-gray-700"
              onClick={props.onScan}
              title="Scan QR code"
            >
              <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>
            </button>
          </div>
          <button
            type="button"
            class="w-full rounded-lg border border-gray-300 px-3 py-2 text-sm font-medium disabled:opacity-50"
            disabled={props.loading() || !props.bunkerUrl().trim()}
            onClick={props.onConnectBunker}
          >
            Connect to Signer
          </button>
        </Show>
      </div>
    </>
  )
}
