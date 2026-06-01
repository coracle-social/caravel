import { createResource, createSignal } from "solid-js"
import { AccountManager } from "applesauce-accounts"
import type { IAccount } from "applesauce-accounts"
import { registerCommonAccountTypes } from "applesauce-accounts/accounts"
import { EventStore } from "applesauce-core"
import { createEventLoaderForStore } from "applesauce-loaders/loaders"
import { RelayPool } from "applesauce-relay"
import { NostrConnectSigner } from "applesauce-signers"
import { getIdentity, getTenant, listPlans, listTenantInvoices, listTenantRelays, registerAccountGetter, type Plan } from "@/lib/api"

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

export const PLATFORM_NAME = import.meta.env.VITE_PLATFORM_NAME || "Caravel"

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

export const [plans] = createResource<Plan[]>(listPlans, { initialValue: [] })

export const [identity, { refetch: refetchIdentity, mutate: setIdentity }] = createResource(
  () => account()?.pubkey,
  pubkey => {
    if (pubkey) return getIdentity()
  }
)

// Shared billing reads, fetched once per session and consumed by the dashboard
// shell, the billing page, and the billing-prompt surface. Keyed on the active
// pubkey so they refetch on account switch; refetchBilling() refreshes them all
// after a mutation (payment, method update, plan change).
const billingKey = () => account()?.pubkey

export const [billingTenant, { refetch: refetchBillingTenant }] = createResource(billingKey, getTenant)
export const [billingInvoices, { refetch: refetchBillingInvoices }] = createResource(billingKey, listTenantInvoices)
export const [billingRelays, { refetch: refetchBillingRelays }] = createResource(billingKey, listTenantRelays)

export function refetchBilling() {
  void refetchBillingTenant()
  void refetchBillingInvoices()
  void refetchBillingRelays()
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

  accountManager.active$.subscribe(account => {
    setAccount(account)

    localStorage.setItem("caravel.accounts", JSON.stringify(accountManager.toJSON(true)))

    if (account) {
      localStorage.setItem("caravel.accounts.active", account.id)
    } else {
      localStorage.removeItem("caravel.accounts.active")
    }

    refetchIdentity()
  })
})
