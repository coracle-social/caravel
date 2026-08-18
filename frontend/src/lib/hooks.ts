import { createEffect, createResource, createSignal, onCleanup } from "solid-js"
import { uniq } from "@welshman/lib"
import type { EventStore } from "applesauce-core"
import type { RelayPool } from "applesauce-relay"
import { getProfilePicture, type ProfileContent } from "applesauce-core/helpers/profile"
import { createOutboxMap, selectOptimalRelays, setFallbackRelays } from "applesauce-core/helpers/relay-selection"
import { includeMailboxes } from "applesauce-core/observable"
import { map, of } from "rxjs"
import {
  createRelay,
  deactivateRelay,
  reactivateRelay,
  getInvoice,
  getRelay,
  getTenant,
  invoiceStatus,
  listInvoices,
  listRelayActivity,
  listRelays,
  listTenantInvoices,
  listTenantRelays,
  listTenants,
  purgeRelay,
  reconcileTenant,
  updateRelay,
  updateTenant,
  type Activity,
  type CreateRelayInput,
  type Invoice,
  type Relay,
  type Tenant,
  type UpdateRelayInput,
} from "@/lib/api"
import { autopayConfigured } from "@/lib/paymentMethod"
import { RELAY_DOMAIN } from "@/lib/subdomain"
import { decidePostPaidFlow, type PaidFlowDecision } from "@/lib/relayPlanFlow"
import { account, eventStore, pool } from "@/lib/state"
import { useNostr } from "@/lib/nostr"

// Subscribes to the raw ProfileContent for a single pubkey from the event store,
// optionally priming it over the network. Call sites project the fields they need
// (name/display_name/nip05/picture) from the returned ProfileContent. Pass
// { prime: false } when a parent list already batch-primes these profiles.
export function useProfileMetadata(pubkey: () => string | undefined, opts?: { prime?: boolean }) {
  // Safe: hooks run inside a component/root reactive scope, so useNostr resolves.
  const nostr = useNostr()
  const prime = opts?.prime ?? true
  const [metadata, setMetadata] = createSignal<ProfileContent | undefined>()

  createEffect(() => {
    const pk = pubkey()

    if (!pk) {
      setMetadata(undefined)
      return
    }

    const profileSub = nostr.eventStore.profile(pk).subscribe(setMetadata)
    const reqSub = prime ? primeProfiles([pk], nostr) : undefined

    onCleanup(() => {
      profileSub.unsubscribe()
      reqSub?.unsubscribe()
    })
  })

  return metadata
}

// Batch variant of useProfileMetadata: subscribes to a list of pubkeys, priming
// them all in one request, and returns a Record keyed by pubkey. Call sites
// project the fields they need from each ProfileContent.
export function useProfileMetadataMap(pubkeys: () => string[]) {
  const nostr = useNostr()
  const [metadata, setMetadata] = createSignal<Record<string, ProfileContent | undefined>>({})

  createEffect(() => {
    const list = pubkeys()
    if (!list.length) return

    const reqSub = primeProfiles(list, nostr)
    const profileSubs = list.map((pubkey) =>
      nostr.eventStore.profile(pubkey).subscribe((profile) => {
        setMetadata((prev) => ({ ...prev, [pubkey]: profile }))
      }),
    )

    onCleanup(() => {
      reqSub.unsubscribe()
      for (const sub of profileSubs) sub.unsubscribe()
    })
  })

  return metadata
}

export function useProfilePicture(pubkey: () => string | undefined) {
  const md = useProfileMetadata(pubkey)
  return () => getProfilePicture(md())
}

// Accepts an optional context so callers inside a reactive scope can thread the
// injected eventStore/pool through; defaults to the module singletons because
// most callers run outside reactive scope (event handlers, plain effects) where
// useNostr() would be invalid.
export function primeProfiles(pubkeys: string[], ctx: { eventStore: EventStore; pool: RelayPool } = { eventStore, pool }) {
  const { eventStore: store, pool: relayPool } = ctx
  const uniquePubkeys = uniq(pubkeys.filter(Boolean))
  if (uniquePubkeys.length === 0) {
    return { unsubscribe() {} }
  }

  const seedRelays = Array.from(relayPool.relays.keys())
  const mailboxSeedSub = seedRelays.length
    ? relayPool.request(seedRelays, { kinds: [10002], authors: uniquePubkeys }).subscribe((event) => {
      store.add(event)
    })
    : undefined

  const outboxMap$ = of(uniquePubkeys.map((pubkey) => ({ pubkey }))).pipe(
    includeMailboxes(store),
    map((pointers) => (seedRelays.length > 0 ? setFallbackRelays(pointers, seedRelays) : pointers)),
    map((pointers) => selectOptimalRelays(pointers, { maxConnections: 8, maxRelaysPerUser: 3 })),
    map(createOutboxMap),
  )

  const profileSub = relayPool.outboxSubscription(outboxMap$, { kinds: [0] }).subscribe((message) => {
    if (message !== "EOSE") store.add(message)
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

export const useAdminInvoices = () => createResource(listInvoices)

export const useInvoice = (id: () => string) => createResource(id, getInvoice)

export const useAdminTenant = (pubkey: () => string) => createResource(pubkey, getTenant)

export const useAdminTenantRelays = (pubkey: () => string) => createResource(pubkey, listTenantRelays)

export const useAdminTenantInvoices = (pubkey: () => string) => createResource(pubkey, listTenantInvoices)

// This client is built for a single zooid domain, so it supplies its own
// VITE_RELAY_DOMAIN as the relay's zooid_domain; the backend validates it
// against the platform's ZOOID_DOMAINS.
export const createRelayForActiveTenant = (input: Omit<CreateRelayInput, "zooid_domain">) => {
  const defaults = {
    info_name: "",
    info_icon: "",
    info_description: "",
    policy_public_read: 0,
    policy_public_write: 0,
    policy_public_join: 0,
    policy_strip_signatures: 0,
    groups_enabled: 1,
    management_enabled: 1,
    members_can_invite: 1,
    push_enabled: 1,
  }

  const overrides = {
    tenant_pubkey: account()!.pubkey,
    zooid_domain: RELAY_DOMAIN,
    blossom_enabled: input.plan_id === "free" ? 0 : 1,
    livekit_enabled: input.plan_id === "free" ? 0 : 1,
  }

  return createRelay({...defaults, ...input, ...overrides})
}

export const updateActiveTenant = (input: { nwc_url?: string }) => updateTenant(account()!.pubkey, input)

export const updateRelayById = (id: string, input: UpdateRelayInput) => updateRelay(id, input)

export const updateRelayPlanById = (id: string, plan_id: string) => updateRelay(id, { plan_id })

export const deactivateRelayById = (id: string) => deactivateRelay(id)

export const reactivateRelayById = (id: string) => reactivateRelay(id)

export const purgeRelayById = (id: string) => purgeRelay(id)

export async function tenantNeedsPaymentSetup(): Promise<boolean> {
  const tenant = await getTenant(account()!.pubkey)
  return !autopayConfigured(tenant)
}

export async function getLatestOpenInvoice(): Promise<Invoice | null> {
  const invoices = await listTenantInvoices(account()!.pubkey)
  const open = invoices
    .filter(inv => invoiceStatus(inv) === "open" && inv.amount > 0)
    .sort((a, b) => b.period_start - a.period_start)
  return open[0] ?? null
}

// Resolve what to do after a paid create/upgrade succeeds: a tenant that already
// has autopay configured just navigates, otherwise we surface the freshly
// materialized open invoice to pay directly (or fall back to payment setup when
// none is available). Shared by RelayNew, Home's signup-and-create path, and the
// plan-upgrade toggle so the post-paid ladder stays identical across all three.
export async function resolvePostPaidFlow(): Promise<PaidFlowDecision> {
  const pubkey = account()!.pubkey
  // The reads below are pure GETs now, so explicitly materialize the just-created
  // invoice and pick up any portal-added card before deciding the post-paid ladder.
  await reconcileTenant(pubkey)

  const needsSetup = await tenantNeedsPaymentSetup()
  const invoice = needsSetup ? await getLatestOpenInvoice() : null
  return decidePostPaidFlow({ needsSetup, invoice })
}

export type { Activity, Invoice, Relay, Tenant }
export type { ProfileContent }
