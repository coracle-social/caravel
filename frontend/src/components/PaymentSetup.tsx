import { createSignal, Show } from "solid-js"
import Modal from "@/components/Modal"
import { updateActiveTenant } from "@/lib/hooks"
import { createPortalSession } from "@/lib/api"
import { account } from "@/lib/state"

type Tab = "nwc" | "card"

type PaymentSetupProps = {
  open: boolean
  onClose: () => void
  onSaved?: () => void
}

export default function PaymentSetup(props: PaymentSetupProps) {
  const [tab, setTab] = createSignal<Tab>("nwc")
  const [nwcUrl, setNwcUrl] = createSignal("")
  const [saving, setSaving] = createSignal(false)
  const [saved, setSaved] = createSignal(false)
  const [error, setError] = createSignal("")
  const [redirecting, setRedirecting] = createSignal(false)

  async function saveNwc() {
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

  async function openPortal() {
    setRedirecting(true)
    setError("")
    try {
      const { url } = await createPortalSession(account()!.pubkey)
      window.location.href = url
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to open billing portal")
      setRedirecting(false)
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
            <h2 class="text-lg font-semibold text-gray-900">Set Up Payments</h2>
            <p class="text-sm text-gray-500 mt-1">Choose how you'd like to pay once invoices are issued for your relay.</p>
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

      <div class="px-6 pt-4">
        <p class="text-xs font-medium text-gray-500 uppercase tracking-wide mb-2">Pay with</p>
        <div class="flex gap-2 border border-gray-200 rounded-lg p-1">
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm transition-colors ${tab() === "nwc" ? "bg-gray-900 text-white" : "text-gray-700 hover:bg-gray-50"}`}
            onClick={() => setTab("nwc")}
          >
            Lightning (NWC)
          </button>
          <button
            type="button"
            class={`flex-1 rounded-md px-3 py-2 text-sm transition-colors ${tab() === "card" ? "bg-gray-900 text-white" : "text-gray-700 hover:bg-gray-50"}`}
            onClick={() => setTab("card")}
          >
            Card
          </button>
        </div>
      </div>

      <div class="px-6 py-4 min-h-[180px] flex flex-col justify-center">
        <Show when={tab() === "nwc"}>
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
                onClick={saveNwc}
                disabled={saving() || !nwcUrl().trim()}
                class="w-full py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
              >
                {saving() ? "Saving..." : "Save"}
              </button>
            </div>
          </Show>
        </Show>

        <Show when={tab() === "card"}>
          <div class="text-center space-y-4">
            <div class="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-gray-100">
              <svg class="w-6 h-6 text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <rect x="1" y="4" width="22" height="16" rx="2" ry="2" />
                <line x1="1" y1="10" x2="23" y2="10" />
              </svg>
            </div>
            <p class="text-sm text-gray-600">Add a payment card via Stripe to enable automatic billing. If an invoice is currently due, we will retry collection after card setup.</p>
            <button
              type="button"
              onClick={openPortal}
              disabled={redirecting()}
              class="w-full py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
            >
              {redirecting() ? "Redirecting..." : "Add a payment card"}
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
        <Show when={saved()}>
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
        <Show when={!saved()}>
          <button
            type="button"
            onClick={handleClose}
            class="py-2 px-4 border border-gray-300 text-gray-700 text-sm rounded-lg hover:bg-gray-50 transition-colors"
          >
            Set up later
          </button>
        </Show>
      </div>
    </Modal>
  )
}
