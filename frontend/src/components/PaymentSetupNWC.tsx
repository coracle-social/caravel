import { createSignal, Show } from "solid-js"
import Modal from "@/components/Modal"
import { updateActiveTenant } from "@/lib/hooks"

type PaymentSetupNWCProps = {
  open: boolean
  onClose: () => void
  onSaved?: () => void
  // The tenant already has a wallet connected, so the copy frames this as
  // replacing it (the stored URL is write-only and never sent back).
  isUpdate?: boolean
}

// Focused Lightning/NWC connect dialog. PaymentSetup offers both methods behind
// tabs for the general setup flow; here the entry point is explicitly "connect a
// Lightning wallet", so there's no method switcher — the card path lives on its
// own row that redirects to Stripe.
export default function PaymentSetupNWC(props: PaymentSetupNWCProps) {
  const [nwcUrl, setNwcUrl] = createSignal("")
  const [saving, setSaving] = createSignal(false)
  const [saved, setSaved] = createSignal(false)
  const [error, setError] = createSignal("")

  async function save() {
    const url = nwcUrl().trim()
    if (!url) return
    setSaving(true)
    setError("")
    try {
      await updateActiveTenant({ nwc_url: url })
      setSaved(true)
      props.onSaved?.()
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to save wallet connection")
    } finally {
      setSaving(false)
    }
  }

  function handleClose() {
    setNwcUrl("")
    setSaved(false)
    setError("")
    props.onClose()
  }

  return (
    <Modal
      open={props.open}
      onClose={handleClose}
      wrapperClass="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      panelClass="w-full max-w-md rounded-2xl bg-white shadow-xl overflow-hidden"
    >
      <div class="px-6 pt-6 pb-4 border-b border-gray-100">
        <div class="flex items-start justify-between gap-3">
          <div>
            <h2 class="text-lg font-semibold text-gray-900">
              {props.isUpdate ? "Update Lightning Wallet" : "Connect Lightning Wallet"}
            </h2>
            <p class="text-sm text-gray-500 mt-1">
              {props.isUpdate
                ? "Paste a new Nostr Wallet Connect URL to replace your connected wallet."
                : "Paste your Nostr Wallet Connect URL to pay invoices automatically over Lightning."}
            </p>
          </div>
          <button
            type="button"
            onClick={handleClose}
            class="text-gray-400 hover:text-gray-700 rounded p-1 hover:bg-gray-100 flex-shrink-0"
            aria-label="Close"
          >
            <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>

      <div class="px-6 py-4 min-h-[180px] flex flex-col justify-center">
        <Show
          when={!saved()}
          fallback={
            <div class="text-center">
              <div class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-green-100">
                <svg class="w-6 h-6 text-green-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              </div>
              <p class="text-sm font-medium text-gray-900">Wallet connected!</p>
              <p class="text-xs text-gray-500 mt-1">Automatic payments are now enabled.</p>
            </div>
          }
        >
          <div class="space-y-3">
            <label class="block text-sm font-medium text-gray-700">Nostr Wallet Connect URL</label>
            <input
              type="text"
              value={nwcUrl()}
              onInput={(e) => setNwcUrl(e.currentTarget.value)}
              placeholder="nostr+walletconnect://..."
              class="w-full border border-gray-300 rounded-lg px-3 py-2"
            />
            <button
              type="button"
              onClick={save}
              disabled={saving() || !nwcUrl().trim()}
              class="w-full py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
            >
              {saving() ? "Saving..." : "Save"}
            </button>
          </div>
        </Show>
      </div>

      <Show when={error()}>
        <div class="px-6 pb-2">
          <p class="text-xs text-red-600">{error()}</p>
        </div>
      </Show>

      <div class="px-6 py-4 border-t border-gray-100">
        <Show
          when={saved()}
          fallback={
            <button
              type="button"
              onClick={handleClose}
              class="py-2 px-4 border border-gray-300 text-gray-700 text-sm rounded-lg hover:bg-gray-50 transition-colors"
            >
              Cancel
            </button>
          }
        >
          <div class="flex justify-end">
            <button
              type="button"
              onClick={handleClose}
              class="py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700"
            >
              Done
            </button>
          </div>
        </Show>
      </div>
    </Modal>
  )
}
