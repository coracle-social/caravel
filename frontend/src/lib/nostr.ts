import { createEffect, createSignal, onCleanup } from "solid-js"
import { EventStore } from "applesauce-core"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import { RelayPool } from "applesauce-relay"
import { AccountManager } from "applesauce-accounts"
import type { IAccount, SerializedAccount } from "applesauce-accounts"
import { registerCommonAccountTypes } from "applesauce-accounts/accounts"
import { NostrConnectSigner } from "applesauce-signers"

export const API_URL = import.meta.env.VITE_API_URL
export const PLATFORM_NAME = import.meta.env.VITE_PLATFORM_NAME || "Caravel"

const PROFILE_RELAYS = ["wss://purplepag.es", "wss://relay.damus.io", "wss://nos.lol"]

export const eventStore = new EventStore()
export const pool = new RelayPool()
export const accounts = new AccountManager()

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

    // Fetch the kind 0 from relays and add to the event store
    const reqSub = pool.request(PROFILE_RELAYS, { kinds: [0], authors: [pk] }).subscribe(event => {
      eventStore.add(event)
    })

    onCleanup(() => {
      profileSub.unsubscribe()
      reqSub.unsubscribe()
    })
  })

  return picture
}
