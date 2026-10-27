import { createResource, createSignal } from "solid-js"
import { AccountManager } from "applesauce-accounts"
import { registerCommonAccountTypes } from "applesauce-accounts/accounts"
import { EventStore } from "applesauce-core"
import { createEventLoaderForStore } from "applesauce-loaders/loaders"
import { RelayPool } from "applesauce-relay"
import { NostrConnectSigner } from "applesauce-signers"
import { account, setAccount } from "@/shared/lib/account"
import { createTenant, getDraftInvoice, getIdentity, getTenant, listPayableInvoices, listPlans, listTenantInvoices, listTenantRelays, reconcileInvoice, reconcileTenant, type Plan } from "@/shared/lib/api"
import { formatError } from "@/shared/lib/format"
import { autopayConfigured } from "@/shared/lib/paymentMethod"

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

type ToastVariant = "error" | "success"

export const [toastVariant, setToastVariant] = createSignal<ToastVariant>("error")
export const [toastMessage, setRawToastMessage] = createSignal("")

export function setToastMessage(message: string, variant: ToastVariant = "error") {
  setToastVariant(variant)
  setRawToastMessage(message)
}

// Set while a page prompts for an invoice itself, so the banner doesn't too.
export const [billingFlowActive, setBillingFlowActive] = createSignal(false)

export const [plans] = createResource<Plan[]>(listPlans, { initialValue: [] })

export const [identity, { refetch: refetchIdentity }] = createResource(() => account()?.pubkey, () => getIdentity())

// Whose billing the reads below fetch, set once that tenant exists.
const [billingPubkey, setBillingPubkey] = createSignal<string>()

export const [billingTenant, { refetch: refetchBillingTenant }] = createResource(billingPubkey, getTenant)
export const [billingInvoices, { refetch: refetchBillingInvoices }] = createResource(billingPubkey, listTenantInvoices)
export const [billingRelays, { refetch: refetchBillingRelays }] = createResource(billingPubkey, listTenantRelays)
export const [billingDraftInvoice, { refetch: refetchBillingDraftInvoice }] = createResource(billingPubkey, getDraftInvoice)

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

let autopayInFlight: { pubkey: string; promise: Promise<void> } | undefined

// Bring billing up to date and pay what's owed, oldest invoice first, with any
// method on file unless a page is prompting for it. Concurrent calls share a run.
export function autopayBilling(pubkey: string): Promise<void> {
  if (autopayInFlight?.pubkey === pubkey) return autopayInFlight.promise

  const promise = (async () => {
    try {
      if (billingPubkey() !== pubkey) await ensureSessionTenant()

      const tenant = await reconcileTenant(pubkey)

      if (autopayConfigured(tenant) && !billingFlowActive()) {
        for (const invoice of listPayableInvoices(await listTenantInvoices(pubkey))) {
          await reconcileInvoice(invoice.id)
        }
      }
    } catch (e) {
      console.error("Autopay billing failed", e)
      setToastMessage(formatError(e, "Failed to update billing"))
    } finally {
      refetchBilling()
      if (autopayInFlight?.pubkey === pubkey) autopayInFlight = undefined
    }
  })()

  autopayInFlight = { pubkey, promise }
  return promise
}

let tenantEnsure: { pubkey: string; promise: Promise<void> } | undefined

// Make the signed-in pubkey a tenant, then let billing reads run. Concurrent calls share a run.
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

try {
  accountManager.fromJSON(JSON.parse(localStorage.getItem("caravel.accounts")!), true)
} catch {
  // pass
}

const active = localStorage.getItem("caravel.accounts.active")

if (active && accountManager.getAccount(active)) {
  accountManager.setActive(active)
}

// Lives as long as the page, except across a Vite hot update.
const accountSubscription = accountManager.active$.subscribe(account => {
  setAccount(account)

  localStorage.setItem("caravel.accounts", JSON.stringify(accountManager.toJSON(true)))

  if (account) {
    localStorage.setItem("caravel.accounts.active", account.id)
  } else {
    localStorage.removeItem("caravel.accounts.active")
  }

  refetchIdentity()

  setBillingPubkey(undefined)
  if (account)
    void ensureSessionTenant().catch(e => {
      console.error("Failed to ensure tenant", e)
      setToastMessage(formatError(e, "Failed to set up your billing account"))
    })
})

if (import.meta.hot) import.meta.hot.dispose(() => accountSubscription.unsubscribe())
