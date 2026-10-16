import { Show, type JSX } from "solid-js"
import Modal from "@/shared/components/Modal"
import type { CardPortal, NwcSetup } from "@/shared/lib/usePaymentSetup"

type PaymentSetupShellProps = {
  open: boolean
  onClose: () => void
  title: string
  description: string
  error?: string
  footer: JSX.Element
  children: JSX.Element
}

// Shared chrome for the payment-setup dialogs: the modal frame, the
// title/description header with a close button, the error line, and the footer
// container. Each caller supplies the body (children) and footer buttons.
export function PaymentSetupShell(props: PaymentSetupShellProps) {
  return (
    <Modal
      open={props.open}
      onClose={props.onClose}
      wrapperClass="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      panelClass="w-full max-w-md rounded-2xl bg-white shadow-xl overflow-hidden"
    >
      <div class="px-6 pt-6 pb-4 border-b border-gray-100">
        <div class="flex items-start justify-between gap-3">
          <div>
            <h2 class="text-lg font-semibold text-gray-900">{props.title}</h2>
            <p class="text-sm text-gray-500 mt-1">{props.description}</p>
          </div>
          <button
            type="button"
            onClick={props.onClose}
            class="text-gray-400 hover:text-gray-700 rounded p-1 hover:bg-gray-100 flex-shrink-0"
            aria-label="Close"
          >
            <svg class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>

      {props.children}

      <Show when={props.error}>
        <div class="px-6 pb-2">
          <p class="text-xs text-red-600">{props.error}</p>
        </div>
      </Show>

      <div class="px-6 py-4 border-t border-gray-100">{props.footer}</div>
    </Modal>
  )
}

// The fixed-height content region between the header and footer.
export function PaymentSetupBody(props: { children: JSX.Element }) {
  return <div class="px-6 py-4 min-h-[180px] flex flex-col justify-center">{props.children}</div>
}

// Footer for every payment-setup dialog: a "Done" confirm once an action has
// succeeded, otherwise a secondary dismiss button. Card setup never "saves" (it
// redirects away), so it always shows the dismiss button.
export function SetupFooter(props: { saved?: boolean; cancelLabel: string; onClose: () => void }) {
  return (
    <Show
      when={props.saved}
      fallback={
        <button
          type="button"
          onClick={props.onClose}
          class="py-2 px-4 border border-gray-300 text-gray-700 text-sm rounded-lg hover:bg-gray-50 transition-colors"
        >
          {props.cancelLabel}
        </button>
      }
    >
      <div class="flex justify-end">
        <button
          type="button"
          onClick={props.onClose}
          class="py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700"
        >
          Done
        </button>
      </div>
    </Show>
  )
}

// Lightning/NWC body: the URL input + save, or the success state once saved.
export function NwcSetupBody(props: { nwc: NwcSetup }) {
  const nwc = props.nwc
  return (
    <Show
      when={!nwc.saved()}
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
          value={nwc.nwcUrl()}
          onInput={(e) => nwc.setNwcUrl(e.currentTarget.value)}
          placeholder="nostr+walletconnect://..."
          class="w-full border border-gray-300 rounded-lg px-3 py-2"
        />
        <button
          type="button"
          onClick={nwc.save}
          disabled={nwc.saving() || !nwc.nwcUrl().trim()}
          class="w-full py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
        >
          {nwc.saving() ? "Saving..." : "Save"}
        </button>
      </div>
    </Show>
  )
}

// Card body: an explanation plus the button that redirects to the Stripe portal.
// `isUpdate` adjusts the copy for tenants who already have a card on file.
export function CardSetupBody(props: { card: CardPortal; isUpdate?: boolean }) {
  return (
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
        onClick={props.card.openPortal}
        disabled={props.card.redirecting()}
        class="w-full py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
      >
        {props.card.redirecting() ? "Redirecting..." : props.isUpdate ? "Manage card" : "Add a payment card"}
      </button>
    </div>
  )
}
