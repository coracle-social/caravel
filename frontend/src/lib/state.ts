import { createResource, createSignal } from "solid-js"
import { AccountManager } from "applesauce-accounts"
import type { IAccount } from "applesauce-accounts"
import { registerCommonAccountTypes } from "applesauce-accounts/accounts"
import { EventStore } from "applesauce-core"
import { createEventLoaderForStore } from "applesauce-loaders/loaders"
import { RelayPool } from "applesauce-relay"
import { NostrConnectSigner } from "applesauce-signers"
import { getIdentity } from "@/lib/api"

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

export const [identity, { refetch: refetchIdentity, mutate: setIdentity }] = createResource(
  () => account()?.pubkey,
  pubkey => {
    if (pubkey) return getIdentity()
  }
)

;(() => {
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
})()
