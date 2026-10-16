import { createResource } from "solid-js"
import { account } from "@/shared/lib/account"
import { createRelay, getRelay, listRelayActivity, listRelayMembers, listTenantRelays, type NewRelay } from "@/shared/lib/api"
import { RELAY_DOMAIN } from "@/shared/lib/config"
import { FREE } from "@/shared/lib/plans"

export const useTenantRelays = () => createResource(() => listTenantRelays(account()!.pubkey))

export const useRelay = (relayId: () => string) => createResource(relayId, getRelay)

export const useRelayActivity = (relayId: () => string) => createResource(relayId, async (id) => {
  const result = await listRelayActivity(id)
  return result.activity
})

// A relay's member pubkeys, or none when they can't be read.
export const useRelayMembers = (relayId: () => string) => createResource(relayId, async (id) => {
  if (!id) return []

  try {
    return (await listRelayMembers(id)).members
  } catch {
    return []
  }
})

// Create a relay for the signed-in tenant, under this dashboard's relay domain.
export function createRelayForActiveTenant(input: Omit<NewRelay, "tenant_pubkey" | "zooid_domain">) {
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
  } as const

  const overrides = {
    tenant_pubkey: account()!.pubkey,
    zooid_domain: RELAY_DOMAIN,
    blossom_enabled: input.plan_id === FREE ? 0 : 1,
    livekit_enabled: input.plan_id === FREE ? 0 : 1,
  } as const

  return createRelay({...defaults, ...input, ...overrides})
}
