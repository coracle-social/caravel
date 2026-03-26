import { createResource } from "solid-js"
import { Relay as NostrRelay, RelayManagement } from "applesauce-relay"
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
  type Identity,
  type Relay,
  type Tenant,
  type UpdateRelayInput,
} from "./api"
import { getActivePubkey, getActiveSigner } from "./nostr"

type IdentityCache = {
  pubkey: string
  value: Identity
}

let identityCache: IdentityCache | undefined
let identityInflight: Promise<Identity> | undefined
let identityInflightPubkey: string | undefined

function requireActivePubkey() {
  const pubkey = getActivePubkey()
  if (!pubkey) throw new Error("Not logged in")
  return pubkey
}

async function loadIdentity(pubkey?: string) {
  if (!pubkey) throw new Error("Not logged in")

  if (identityCache?.pubkey === pubkey) {
    return identityCache.value
  }

  if (identityInflight && identityInflightPubkey === pubkey) {
    return identityInflight
  }

  const request = getIdentity().then((identity) => {
    identityCache = { pubkey, value: identity }
    return identity
  }).finally(() => {
    if (identityInflightPubkey === pubkey) {
      identityInflight = undefined
      identityInflightPubkey = undefined
    }
  })

  identityInflight = request
  identityInflightPubkey = pubkey
  return request
}

export function useIdentity(source: () => string | undefined) {
  return createResource(source, loadIdentity)
}

export function useTenant() {
  return createResource(async () => {
    const pubkey = requireActivePubkey()
    return getTenant(pubkey)
  })
}

export function useTenantRelays() {
  return createResource(async () => {
    const pubkey = requireActivePubkey()
    return listTenantRelays(pubkey)
  })
}

export function useTenantInvoices() {
  return createResource(async () => {
    const pubkey = requireActivePubkey()
    return listTenantInvoices(pubkey)
  })
}

export function useRelay(relayId: () => string) {
  return createResource(relayId, getRelay)
}

export function useAdminTenants() {
  return createResource(listTenants)
}

export function useAdminRelays() {
  return createResource(listRelays)
}

export function useAdminTenantDetail(pubkey: () => string) {
  return createResource(pubkey, async (id) => {
    const [tenant, relays] = await Promise.all([getTenant(id), listTenantRelays(id)])
    return { tenant, relays }
  })
}

export async function createRelayForActiveTenant(input: CreateRelayInput) {
  const pubkey = requireActivePubkey()
  return createRelay({ ...input, tenant: pubkey })
}

export function updateActiveTenantBilling(nwc_url: string) {
  const pubkey = requireActivePubkey()
  return updateTenantBilling(pubkey, { nwc_url })
}

export function updateRelayById(id: string, input: UpdateRelayInput) {
  return updateRelay(id, input)
}

export function updateRelayPlanById(id: string, plan: string) {
  return updateRelay(id, { plan })
}

export function deactivateRelayById(id: string) {
  return deactivateRelay(id)
}

export async function getRelayMemberCount(relayUrl: string): Promise<number | undefined> {
  const signer = getActiveSigner()
  if (!signer) throw new Error("Not logged in")

  const management = new RelayManagement(new NostrRelay(relayUrl), signer)
  try {
    const members = await management.listAllowedPubkeys()
    return members.length
  } catch {
    return undefined
  }
}

export type { Relay, Tenant }
