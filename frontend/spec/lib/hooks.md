Hooks wrap data access and mutations so pages/components do not call API methods directly.

## `function useTenant()`

- Uses `createTenant()` then `getTenant(pubkey)` for the active account
- Returns a Solid `createResource` tuple

## `function useTenantRelays()`

- Uses `createTenant()` then `listTenantRelays(pubkey)` for the active account
- Returns a Solid `createResource` tuple

## `function useTenantInvoices()`

- Uses `createTenant()` then `listTenantInvoices(pubkey)` for the active account
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

## `function useAdminCheck(source)`

- Uses `listTenants()` as an admin capability check
- Returns `{ is_admin: true }` on success
- Returns `{ is_admin: false }` on `403`

## `function createRelayForActiveTenant(input)`

- Uses `createTenant()` then `createRelay({ tenant: activePubkey, ...input })`

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
