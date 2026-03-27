The api allows the frontend to access the database. Most endpoints are authenticated using NIP 98.

## `class ApiError`

- This is a custom error class with an additional `status` property

## `function makeAuth<T>()`

- If the user is not currently logged in, returns undefined
- Otherwise, builds an NIP 98 auth header with no `method` and `u` set to `VITE_API_URL`. This is non-standard, but intentional in order to prevent repeated signer authorizations.
- This function is memoized over pubkey and expires after 10 minutes.
- Returns the full `Authorization` header value.

## `function callApi<T>(method: string, path: string, body?: T)`

- Uses `makeAuth` to obtain a NIP 98 authorization header.
- Calls the backend api and returns the decoded json or throws an `ApiError`.

## `function getIdentity()`

- Calls `GET /identity`
- Requires authentication
- Returns `{ pubkey, is_admin }` for the authorized user

## Plan methods

## `function listPlans()`

- Calls `GET /plans`
- Returns a list of plans

## `function getPlan(id: string)`

- Calls `GET /plans/:id`
- Returns a single plan

## Tenant methods

## `function listTenants()`

- Calls `GET /tenants`
- Admin only
- Returns a list of tenants

## `function getTenant(pubkey: string)`

- Calls `GET /tenants/:pubkey`
- Admin or matching tenant
- Returns a single tenant

## `function createTenant()`

- Calls `POST /tenants`
- Requires authentication
- Creates tenant for the authorized pubkey

## `function listTenantRelays(pubkey: string)`

- Calls `GET /tenants/:pubkey/relays`
- Admin or matching tenant
- Returns relays for the tenant

## `function listTenantInvoices(pubkey: string)`

- Calls `GET /tenants/:pubkey/invoices`
- Admin or matching tenant
- Returns invoices for the tenant

## `function updateTenantBilling(pubkey: string, billing: { nwc_url: string })`

- Calls `PUT /tenants/:pubkey/billing`
- Admin or matching tenant
- Updates billing configuration

## Relay methods

## `function listRelays()`

- Calls `GET /relays`
- Admin only
- Returns all relays

## `function getRelay(id: string)`

- Calls `GET /relays/:id`
- Admin or relay owner

## `function createRelay(input: CreateRelayInput)`

- Calls `POST /relays`
- Admin or matching tenant in payload
- Creates a relay

## `function updateRelay(id: string, input: UpdateRelayInput)`

- Calls `PUT /relays/:id`
- Admin or relay owner
- Updates a relay

## `function deactivateRelay(id: string)`

- Calls `POST /relays/:id/deactivate`
- Admin or relay owner

## Invoice methods

## `function listInvoices()`

- Calls `GET /invoices`
- Admin only
- Returns all invoices

## `function getInvoice(id: string)`

- Calls `GET /invoices/:id`
- Admin or invoice owner
- Returns a single invoice
