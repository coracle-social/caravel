import { createEffect, createSignal, onCleanup } from "solid-js"
import { EventStore } from "applesauce-core"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import { createOutboxMap, selectOptimalRelays, setFallbackRelays } from "applesauce-core/helpers/relay-selection"
import { includeMailboxes } from "applesauce-core/observable"
import { RelayPool } from "applesauce-relay"
import { AccountManager } from "applesauce-accounts"
import type { IAccount, SerializedAccount } from "applesauce-accounts"
import { registerCommonAccountTypes } from "applesauce-accounts/accounts"
import { createEventLoaderForStore } from "applesauce-loaders/loaders"
import { NostrConnectSigner } from "applesauce-signers"
import { map, of } from "rxjs"

export const API_URL = import.meta.env.VITE_API_URL
export const PLATFORM_NAME = import.meta.env.VITE_PLATFORM_NAME || "Caravel"

export const eventStore = new EventStore()
export const pool = new RelayPool()
export const accounts = new AccountManager()

createEventLoaderForStore(eventStore, pool, {
  lookupRelays: ["wss://purplepag.es/", "wss://relay.damus.io/", "wss://indexer.coracle.social/"],
  extraRelays: ["wss://relay.damus.io/", "wss://nos.lol/", "wss://relay.primal.net/"],
})

const ACCOUNTS_STORAGE_KEY = "caravel.accounts"
const ACTIVE_ACCOUNT_STORAGE_KEY = "caravel.activeAccount"

registerCommonAccountTypes(accounts)
NostrConnectSigner.subscriptionMethod = pool.subscription.bind(pool)
NostrConnectSigner.publishMethod = pool.publish.bind(pool)

export function restoreAccounts() {
  const raw = localStorage.getItem(ACCOUNTS_STORAGE_KEY)
  if (raw) {
    try {
      const saved = JSON.parse(raw) as SerializedAccount[]
      accounts.fromJSON(saved, true)
    } catch {
      // ignore corrupted local state
    }
  }

  const activeId = localStorage.getItem(ACTIVE_ACCOUNT_STORAGE_KEY)
  if (activeId && accounts.getAccount(activeId)) {
    accounts.setActive(activeId)
  }
}

export function activateAccount(account: IAccount) {
  accounts.addAccount(account)
  accounts.setActive(account)
  persistAccounts()
}

export function persistAccounts() {
  localStorage.setItem(ACCOUNTS_STORAGE_KEY, JSON.stringify(accounts.toJSON(true)))
  const active = accounts.getActive()
  if (active) {
    localStorage.setItem(ACTIVE_ACCOUNT_STORAGE_KEY, active.id)
  } else {
    localStorage.removeItem(ACTIVE_ACCOUNT_STORAGE_KEY)
  }
}

export function useActiveAccount() {
  const [account, setAccount] = createSignal(accounts.active)
  const sub = accounts.active$.subscribe(setAccount)
  onCleanup(() => sub.unsubscribe())
  return account
}

export function useProfilePicture(pubkey: () => string | undefined) {
  const [picture, setPicture] = createSignal<string | undefined>()

  createEffect(() => {
    const pk = pubkey()

    if (!pk) {
      setPicture(undefined)
      return
    }

    // Subscribe to profile changes in the event store
    const profileSub = eventStore.profile(pk).subscribe(profile => {
      setPicture(getProfilePicture(profile))
    })

    const reqSub = primeProfiles([pk])

    onCleanup(() => {
      profileSub.unsubscribe()
      reqSub.unsubscribe()
    })
  })

  return picture
}

export function primeProfiles(pubkeys: string[]) {
  const uniquePubkeys = Array.from(new Set(pubkeys.filter(Boolean)))
  if (uniquePubkeys.length === 0) {
    return { unsubscribe() {} }
  }

  const seedRelays = Array.from(pool.relays.keys())
  const mailboxSeedSub = seedRelays.length
    ? pool.request(seedRelays, { kinds: [10002], authors: uniquePubkeys }).subscribe((event) => {
      eventStore.add(event)
    })
    : undefined

  const outboxMap$ = of(uniquePubkeys.map((pubkey) => ({ pubkey }))).pipe(
    includeMailboxes(eventStore),
    map((pointers) => (seedRelays.length > 0 ? setFallbackRelays(pointers, seedRelays) : pointers)),
    map((pointers) => selectOptimalRelays(pointers, { maxConnections: 8, maxRelaysPerUser: 3 })),
    map(createOutboxMap),
  )

  const profileSub = pool.outboxSubscription(outboxMap$, { kinds: [0] }).subscribe((message) => {
    if (message !== "EOSE") eventStore.add(message)
  })

  return {
    unsubscribe() {
      profileSub.unsubscribe()
      mailboxSeedSub?.unsubscribe()
    },
  }
}
