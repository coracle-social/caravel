import { createSignal, Show } from "solid-js"
import Modal from "@/components/Modal"
import { createPortalSession } from "@/lib/api"
import { account } from "@/lib/state"

type PaymentSetupCardProps = {
  open: boolean
  onClose: () => void
  // The tenant already has a card on file, so the copy frames this as managing
  // the existing one rather than adding a first card.
  isUpdate?: boolean
}

// Focused card dialog. PaymentSetup offers both methods behind tabs for the
// general setup flow; here the entry point is explicitly "manage your card", so
// there's no method switcher — adding/updating a card is a redirect to the
// Stripe billing portal, which returns to wherever it was opened from.
export default function PaymentSetupCard(props: PaymentSetupCardProps) {
  const [redirecting, setRedirecting] = createSignal(false)
  const [error, setError] = createSignal("")

  async function openPortal() {
    setRedirecting(true)
    setError("")
    try {
      const { url } = await createPortalSession(account()!.pubkey, window.location.href)
      window.location.href = url
    } catch (e) {
      setError(e instanceof Error ? e.message : "Failed to open billing portal")
      setRedirecting(false)
    }
  }

  function handleClose() {
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
              {props.isUpdate ? "Manage Card" : "Add a Card"}
            </h2>
            <p class="text-sm text-gray-500 mt-1">
              {props.isUpdate
                ? "Manage your saved card in the Stripe billing portal."
                : "Add a card via the Stripe billing portal to pay invoices automatically."}
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
        <div class="text-center space-y-4">
          <div class="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-gray-100">
            <svg class="w-6 h-6 text-gray-400" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <rect x="1" y="4" width="22" height="16" rx="2" ry="2" />
              <line x1="1" y1="10" x2="23" y2="10" />
            </svg>
          </div>
          <p class="text-sm text-gray-600">
            {props.isUpdate
              ? "Update or remove your card in the Stripe billing portal. We'll retry any due invoice after you're done."
              : "Add a payment card via Stripe to enable automatic billing. If an invoice is currently due, we will retry collection after card setup."}
          </p>
          <button
            type="button"
            onClick={openPortal}
            disabled={redirecting()}
            class="w-full py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
          >
            {redirecting() ? "Redirecting..." : props.isUpdate ? "Manage card" : "Add a payment card"}
          </button>
        </div>
      </div>

      <Show when={error()}>
        <div class="px-6 pb-2">
          <p class="text-xs text-red-600">{error()}</p>
        </div>
      </Show>

      <div class="px-6 py-4 border-t border-gray-100">
        <button
          type="button"
          onClick={handleClose}
          class="py-2 px-4 border border-gray-300 text-gray-700 text-sm rounded-lg hover:bg-gray-50 transition-colors"
        >
          Cancel
        </button>
      </div>
    </Modal>
  )
}
