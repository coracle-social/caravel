import { createResource } from "solid-js"
import { Relay as NostrRelay, RelayManagement } from "applesauce-relay"
import {
  ApiError,
  createRelay,
  createTenant,
  deactivateRelay,
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
  type UpdateRelayInput,
} from "./api"
import { getActivePubkey, getActiveSigner } from "./nostr"

function requireActivePubkey() {
  const pubkey = getActivePubkey()
  if (!pubkey) throw new Error("Not logged in")
  return pubkey
}

async function ensureActiveTenant() {
  try {
    await createTenant()
  } catch (e) {
    if (e instanceof ApiError && e.status === 422) return
    throw e
  }
}

export function useTenant() {
  return createResource(async () => {
    const pubkey = requireActivePubkey()
    await ensureActiveTenant()
    return getTenant(pubkey)
  })
}

export function useTenantRelays() {
  return createResource(async () => {
    const pubkey = requireActivePubkey()
    await ensureActiveTenant()
    return listTenantRelays(pubkey)
  })
}

export function useTenantInvoices() {
  return createResource(async () => {
    const pubkey = requireActivePubkey()
    await ensureActiveTenant()
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

export function useAdminCheck(source: () => string | undefined) {
  return createResource(source, async () => {
    try {
      await listTenants()
      return { is_admin: true }
    } catch (e) {
      if (e instanceof ApiError && e.status === 403) return { is_admin: false }
      throw e
    }
  })
}

export async function createRelayForActiveTenant(input: CreateRelayInput) {
  const pubkey = requireActivePubkey()
  await ensureActiveTenant()
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
