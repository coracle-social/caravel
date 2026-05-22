import { createEffect, createResource, createSignal, onCleanup } from "solid-js"
import { getProfilePicture } from "applesauce-core/helpers/profile"
import { createOutboxMap, selectOptimalRelays, setFallbackRelays } from "applesauce-core/helpers/relay-selection"
import { includeMailboxes } from "applesauce-core/observable"
import { map, of } from "rxjs"
import {
  createRelay,
  deactivateRelay,
  reactivateRelay,
  getRelay,
  getTenant,
  listRelayActivity,
  listRelays,
  listTenantInvoices,
  listTenantRelays,
  listTenants,
  updateRelay,
  updateTenant,
  type Activity,
  type CreateRelayInput,
  type Invoice,
  type Relay,
  type Tenant,
  type UpdateRelayInput,
} from "@/lib/api"
import { account, eventStore, pool } from "@/lib/state"

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

export const useRelay = (relayId: () => string) => createResource(relayId, getRelay)

export const useRelayActivity = (relayId: () => string) => createResource(relayId, async (id) => {
  const result = await listRelayActivity(id)
  return result.activity
})

export const useAdminTenants = () => createResource(listTenants)

export const useAdminRelays = () => createResource(listRelays)

export const useAdminTenant = (pubkey: () => string) => createResource(pubkey, getTenant)

export const useAdminTenantRelays = (pubkey: () => string) => createResource(pubkey, listTenantRelays)

export const createRelayForActiveTenant = (input: CreateRelayInput) => {
  const defaults = {
    info_name: "",
    info_icon: "",
    info_description: "",
    policy_public_join: 0,
    policy_strip_signatures: 0,
    groups_enabled: 1,
    management_enabled: 1,
    push_enabled: 1,
  }

  const overrides = {
    tenant: account()!.pubkey,
    blossom_enabled: input.plan === "free" ? 0 : 1,
    livekit_enabled: input.plan === "free" ? 0 : 1,
  }

  return createRelay({...defaults, ...input, ...overrides})
}

export const updateActiveTenant = (input: { nwc_url?: string }) => updateTenant(account()!.pubkey, input)

export const updateRelayById = (id: string, input: UpdateRelayInput) => updateRelay(id, input)

export const updateRelayPlanById = (id: string, plan: string) => updateRelay(id, { plan })

export const deactivateRelayById = (id: string) => deactivateRelay(id)

export const reactivateRelayById = (id: string) => reactivateRelay(id)

export async function tenantNeedsPaymentSetup(): Promise<boolean> {
  const tenant = await getTenant(account()!.pubkey)
  return !tenant.nwc_is_set && !tenant.stripe_subscription_id
}

export async function getLatestOpenInvoice(): Promise<Invoice | null> {
  const invoices = await listTenantInvoices(account()!.pubkey)
  const open = invoices
    .filter(inv => inv.status === "open" && inv.amount_due > 0)
    .sort((a, b) => b.period_start - a.period_start)
  return open[0] ?? null
}

export type { Activity, Invoice, Relay, Tenant }
