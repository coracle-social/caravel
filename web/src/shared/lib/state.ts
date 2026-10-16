import { createResource, createSignal } from "solid-js"
import { AccountManager } from "applesauce-accounts"
import type { IAccount } from "applesauce-accounts"
import { registerCommonAccountTypes } from "applesauce-accounts/accounts"
import { EventStore } from "applesauce-core"
import { createEventLoaderForStore } from "applesauce-loaders/loaders"
import { RelayPool } from "applesauce-relay"
import { NostrConnectSigner } from "applesauce-signers"
import { createTenant, getDraftInvoice, getIdentity, getTenant, listPayableInvoices, listPlans, listTenantInvoices, listTenantRelays, reconcileInvoice, reconcileTenant, registerAccountGetter, type Plan } from "@/shared/lib/api"
import { autopayConfigured } from "@/shared/lib/paymentMethod"

export type UnsignedEvent = {
  kind: number
  content: string
  created_at: number
  tags: string[][]
}

export type SignedEvent = UnsignedEvent & {
  id: string
  pubkey: string
  sig: string
}

export type EventSigner = {
  signEvent(event: UnsignedEvent): Promise<SignedEvent>
}

export const eventStore = new EventStore()
export const pool = new RelayPool()

createEventLoaderForStore(eventStore, pool, {
  lookupRelays: ["wss://purplepag.es/", "wss://relay.damus.io/", "wss://indexer.coracle.social/"],
  extraRelays: ["wss://relay.damus.io/", "wss://nos.lol/", "wss://relay.primal.net/"],
})

NostrConnectSigner.subscriptionMethod = pool.subscription.bind(pool)
NostrConnectSigner.publishMethod = pool.publish.bind(pool)

export const accountManager = new AccountManager()

registerCommonAccountTypes(accountManager)

export const [account, setAccount] = createSignal<IAccount | undefined>()

registerAccountGetter(account)

export type ToastVariant = "error" | "success"

export const [toastVariant, setToastVariant] = createSignal<ToastVariant>("error")
export const [toastMessage, setRawToastMessage] = createSignal("")

export function setToastMessage(message: string, variant: ToastVariant = "error") {
  setToastVariant(variant)
  setRawToastMessage(message)
}

// Set while the create/upgrade flow drives its own payment/setup modals, so the
// shared prompt surface doesn't double-prompt for the same invoice. Cleared on
// every close path of that flow.
export const [billingFlowActive, setBillingFlowActive] = createSignal(false)

export const [plans] = createResource<Plan[]>(listPlans, { initialValue: [] })

export const [identity, { refetch: refetchIdentity, mutate: setIdentity }] = createResource(
  () => account()?.pubkey,
  pubkey => {
    if (pubkey) return getIdentity()
  }
)

// Shared billing reads, fetched once per session and consumed by the dashboard
// shell, the billing page, and the billing-prompt surface. They're gated on
// billingPubkey rather than the active account directly: the tenant row is
// provisioned lazily on first login, so getTenant/listTenantInvoices 404 until
// ensureSessionTenant() has created it. billingPubkey is reset on activation and
// re-set once the tenant is ensured, so the reads still refetch on account switch
// without racing ahead of provisioning. refetchBilling() refreshes them all after
// a mutation (payment, method update, plan change).
const [billingPubkey, setBillingPubkey] = createSignal<string>()
const billingKey = () => billingPubkey()

export const [billingTenant, { refetch: refetchBillingTenant }] = createResource(billingKey, getTenant)
export const [billingInvoices, { refetch: refetchBillingInvoices }] = createResource(billingKey, listTenantInvoices)
export const [billingRelays, { refetch: refetchBillingRelays }] = createResource(billingKey, listTenantRelays)
export const [billingDraftInvoice, { refetch: refetchBillingDraftInvoice }] = createResource(billingKey, getDraftInvoice)

export function refetchBilling() {
  void Promise.allSettled([
    refetchBillingTenant(),
    refetchBillingInvoices(),
    refetchBillingRelays(),
    refetchBillingDraftInvoice(),
  ]).then(results => {
    if (results.some(r => r.status === "rejected")) {
      const err = results.find(r => r.status === "rejected") as PromiseRejectedResult | undefined
      console.error("Failed to refresh billing data", err?.reason)
      setToastMessage("Failed to refresh billing data")
    }
  })
}

// In-flight autopay run, keyed by pubkey, so concurrent triggers (a mount
// double-fire, two dialog onClose handlers) collapse into one run.
let autopayInFlight: { pubkey: string; promise: Promise<void> } | undefined

// The side-effecting billing refresh, layered above the pure refetchBilling: on
// load of the billing surface it reconciles the subscription (materializing the
// current invoice), syncs the Stripe payment method (picking up a portal-added
// card), and — when a method is on file and an invoice is due — collects it,
// then refreshes all billing reads. This is what makes "add a card, return to
// the app" actually pay the open invoice. Payment is skipped while the
// create/upgrade flow owns the invoice (billingFlowActive).
export function autopayBilling(pubkey: string): Promise<void> {
  if (autopayInFlight?.pubkey === pubkey) return autopayInFlight.promise

  const promise = (async () => {
    try {
      // The tenant row is provisioned lazily on first login; make sure it exists
      // before the tenant-scoped POSTs, since autopay can fire (on the Account
      // landing effect) before provisioning has completed.
      if (billingPubkey() !== pubkey) await ensureSessionTenant()

      // Sync the payment method + reconcile the subscription, then collect every
      // open invoice when a method is on file (and the create/upgrade flow isn't
      // already driving its own invoice). Sequentially and oldest first, so a
      // tenant carrying more than one invoice clears the whole balance in one
      // pass without firing concurrent charges at their wallet or card.
      const tenant = await reconcileTenant(pubkey)

      if (autopayConfigured(tenant) && !billingFlowActive()) {
        for (const invoice of listPayableInvoices(await listTenantInvoices(pubkey))) {
          await reconcileInvoice(invoice.id)
        }
      }
    } catch (e) {
      console.error("Autopay billing failed", e)
      setToastMessage(e instanceof Error ? e.message : "Failed to update billing")
    } finally {
      // Reflect the final state (paid invoice, cleared errors, relay changes).
      refetchBilling()
      if (autopayInFlight?.pubkey === pubkey) autopayInFlight = undefined
    }
  })()

  autopayInFlight = { pubkey, promise }
  return promise
}

// Ensure the active pubkey's tenant row exists, then unlock billing reads. The
// tenant is created lazily on first login, so this must run before any
// tenant-scoped read. The in-flight promise is shared so the login flow (which
// awaits it to roll back on failure) and the activation subscriber below don't
// double-provision; createTenant is itself idempotent.
let tenantEnsure: { pubkey: string; promise: Promise<void> } | undefined

export function ensureSessionTenant(): Promise<void> {
  const pubkey = account()?.pubkey
  if (!pubkey) return Promise.resolve()
  if (tenantEnsure?.pubkey === pubkey) return tenantEnsure.promise

  const promise = (async () => {
    try {
      await createTenant(window.location.origin + '/account')
      if (account()?.pubkey === pubkey) setBillingPubkey(pubkey)
    } finally {
      if (tenantEnsure?.pubkey === pubkey) tenantEnsure = undefined
    }
  })()

  tenantEnsure = { pubkey, promise }
  return promise
}

// Deferred to avoid circular-import TDZ errors (api.ts <-> state.ts)
queueMicrotask(() => {
  try {
    accountManager.fromJSON(JSON.parse(localStorage.getItem("caravel.accounts")!))
  } catch {
    // pass
  }

  const active = localStorage.getItem("caravel.accounts.active")

  if (active) {
    accountManager.setActive(active)
  }

  // Held for the whole app session: this callback persists accounts to
  // localStorage and ensures the session tenant on every switch, so it must
  // never be torn down by a component lifecycle. The only teardown is the HMR
  // dispose hook below, which prevents a Vite hot-update from stacking a
  // duplicate persisting subscriber during dev (production uses /* @refresh
  // reload */, so it never runs there).
  const accountSubscription = accountManager.active$.subscribe(account => {
    setAccount(account)

    localStorage.setItem("caravel.accounts", JSON.stringify(accountManager.toJSON(true)))

    if (account) {
      localStorage.setItem("caravel.accounts.active", account.id)
    } else {
      localStorage.removeItem("caravel.accounts.active")
    }

    refetchIdentity()

    // Lock billing reads until the new account's tenant is ensured, so they never
    // fire against a not-yet-provisioned tenant during signup.
    setBillingPubkey(undefined)
    if (account)
      void ensureSessionTenant().catch(e => {
        console.error("Failed to ensure tenant", e)
        setToastMessage(e instanceof Error ? e.message : "Failed to set up your billing account")
      })
  })

  if (import.meta.hot) import.meta.hot.dispose(() => accountSubscription.unsubscribe())
})
