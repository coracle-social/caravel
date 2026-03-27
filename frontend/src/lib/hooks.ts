import { createEffect, createResource, createSignal, onCleanup } from "solid-js"
import { AccountManager } from "applesauce-accounts"
import type { IAccount } from "applesauce-accounts"
import { registerCommonAccountTypes } from "applesauce-accounts/accounts"
import { EventStore } from "applesauce-core"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import { createOutboxMap, selectOptimalRelays, setFallbackRelays } from "applesauce-core/helpers/relay-selection"
import { includeMailboxes } from "applesauce-core/observable"
import { createEventLoaderForStore } from "applesauce-loaders/loaders"
import { Relay as NostrRelay, RelayManagement } from "applesauce-relay"
import { RelayPool } from "applesauce-relay"
import { NostrConnectSigner } from "applesauce-signers"
import { map, of } from "rxjs"
import {
  createRelay,
  deactivateRelay,
  getIdentity,
  getRelay,
  getTenant,
  listRelays,
  listTenantInvoices,
  listTenantRelays,
  listTenants,
  updateRelay,
  updateTenantBilling,
  type CreateRelayInput,
  type Relay,
  type Tenant,
  type Identity,
  type UpdateRelayInput,
} from "./api"

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

export const [identity, setIdentity] = createSignal<Identity | undefined>()

;(() => {
  accountManager.active$.subscribe(account => {
    setAccount(account)

    localStorage.setItem("caravel.accounts", JSON.stringify(accountManager.toJSON(true)))

    if (account) {
      localStorage.setItem("caravel.accounts.active", account.id)
      getIdentity().then(setIdentity)
    } else {
      localStorage.removeItem("caravel.accounts.active")
      setIdentity(undefined)
    }
  })
})()

export function useProfilePicture(pubkey: () => string | undefined) {
  const [picture, setPicture] = createSignal<string | undefined>()

  createEffect(() => {
    const pk = pubkey()

    if (!pk) {
      setPicture(undefined)
      return
    }

    const profileSub = eventStore.profile(pk).subscribe((profile) => {
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

export const useTenant = () => createResource(() => getTenant(account()!.pubkey))

export const useTenantRelays = () => createResource(() => listTenantRelays(account()!.pubkey))

export const useTenantInvoices = () => createResource(() => listTenantInvoices(account()!.pubkey))

export const useRelay = (relayId: () => string) => createResource(relayId, getRelay)

export const useAdminTenants = () => createResource(listTenants)

export const useAdminRelays = () => createResource(listRelays)

export const useAdminTenant = (pubkey: () => string) => createResource(pubkey, getTenant)

export const useAdminTenantRelays = (pubkey: () => string) => createResource(pubkey, listTenantRelays)

export const createRelayForActiveTenant = (input: CreateRelayInput) => createRelay({ ...input, tenant: account()!.pubkey })

export const updateActiveTenantBilling = (nwc_url: string) => updateTenantBilling(account()!.pubkey, { nwc_url })

export const updateRelayById = (id: string, input: UpdateRelayInput) => updateRelay(id, input)

export const updateRelayPlanById = (id: string, plan: string) => updateRelay(id, { plan })

export const deactivateRelayById = (id: string) => deactivateRelay(id)

export async function getRelayMembers(url: string) {
  const management = new RelayManagement(new NostrRelay(url), account()!.signer)

  try {
    return await management.listAllowedPubkeys()
  } catch {
    return []
  }
}

export type { Relay, Tenant }
