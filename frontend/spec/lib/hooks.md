Hooks wrap data access and mutations so pages/components do not call API methods directly.

## `function useIdentity(source)`

- Calls `getIdentity()` for the active pubkey
- Caches identity in module memory (not localStorage)
- Returns a Solid `createResource` tuple

## `function useTenant()`

- Uses `getTenant(pubkey)` for the active account
- Returns a Solid `createResource` tuple

## `function useTenantRelays()`

- Uses `listTenantRelays(pubkey)` for the active account
- Returns a Solid `createResource` tuple

## `function useTenantInvoices()`

- Uses `listTenantInvoices(pubkey)` for the active account
- Returns a Solid `createResource` tuple

## `function useRelay(relayId)`

- Uses `getRelay(id)`
- Returns a Solid `createResource` tuple

## `function useAdminTenants()`

- Uses `listTenants()`
- Returns a Solid `createResource` tuple

## `function useAdminRelays()`

- Uses `listRelays()`
- Returns a Solid `createResource` tuple

## `function useAdminTenantDetail(pubkey)`

- Uses `getTenant(pubkey)` and `listTenantRelays(pubkey)`
- Returns a Solid `createResource` tuple with `{ tenant, relays }`

## `function createRelayForActiveTenant(input)`

- Uses `createRelay({ tenant: activePubkey, ...input })`

## `function updateActiveTenantBilling(nwc_url)`

- Uses `updateTenantBilling(activePubkey, { nwc_url })`

## `function updateRelayById(id, input)`

- Uses `updateRelay(id, input)`

## `function updateRelayPlanById(id, plan)`

- Uses `updateRelay(id, { plan })`

## `function deactivateRelayById(id)`

- Uses `deactivateRelay(id)`

## `function getRelayMemberCount(relayUrl)`

- Uses relay management (not backend API) to fetch allowed pubkeys and return count
