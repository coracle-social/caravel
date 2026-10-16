import { Show, type JSX } from "solid-js"
import Modal from "@/shared/components/Modal"
import ModalHeader from "@/shared/components/ModalHeader"
import { account } from "@/shared/lib/account"
import { createBillingPortalSession } from "@/shared/lib/api"
import type { NwcSetup } from "@/shared/lib/useNwcSetup"
import type { Redirect } from "@/shared/lib/useRedirect"

type PaymentSetupShellProps = {
  open: boolean
  onClose: () => void
  title: string
  description: string
  error?: string
  footer: JSX.Element
  children: JSX.Element
}

// The frame every payment-setup dialog shares, around its body and footer.
export function PaymentSetupShell(props: PaymentSetupShellProps) {
  return (
    <Modal open={props.open} onClose={props.onClose}>
      <ModalHeader title={props.title} onClose={props.onClose}>
        <p class="text-sm text-gray-500 mt-1">{props.description}</p>
      </ModalHeader>

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

export function PaymentSetupBody(props: { children: JSX.Element }) {
  return <div class="px-6 py-4 min-h-[180px] flex flex-col justify-center">{props.children}</div>
}

// "Done" once a method is saved, otherwise a button to dismiss the dialog.
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

// Adding or managing a card in Stripe's billing portal. `isUpdate` is for a tenant with one.
export function CardSetupBody(props: { billingPortal: Redirect; isUpdate?: boolean }) {
  function open() {
    void props.billingPortal.go(
      () => createBillingPortalSession(account()!.pubkey, window.location.href),
      "Failed to open billing portal",
    )
  }

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
        onClick={open}
        disabled={props.billingPortal.redirecting()}
        class="w-full py-2 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 transition-colors"
      >
        {props.billingPortal.redirecting() ? "Redirecting..." : props.isUpdate ? "Manage card" : "Add a payment card"}
      </button>
    </div>
  )
}
