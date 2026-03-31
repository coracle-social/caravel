const API_URL = import.meta.env.VITE_API_URL

// Populated by state.ts after it initializes, breaking the circular dependency
let getAccount: () => unknown = () => undefined
export function registerAccountGetter(fn: () => unknown) {
  getAccount = fn
}

type ApiOk<T> = {
  data: T
  code: string
}

type BillingInput = {
  nwc_url: string
}

type AuthCache = {
  pubkey: string
  value: string
  expiresAt: number
}

export class ApiError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = "ApiError"
    this.status = status
  }
}

export type Plan = {
  id: string
  name: string
  sats: number
  members: number | null
  blossom: boolean
  livekit: boolean
}

export type PlanId = string

export type Relay = {
  id: string
  tenant: string
  schema: string
  subdomain: string
  plan: PlanId
  status: string
  sync_error: string
  info_name: string
  info_icon: string
  info_description: string
  policy_public_join: number
  policy_strip_signatures: number
  groups_enabled: number
  management_enabled: number
  blossom_enabled: number
  livekit_enabled: number
  push_enabled: number
}

export type CreateRelayInput = {
  tenant?: string
  subdomain: string
  plan: string
  info_name?: string
  info_icon?: string
  info_description?: string
  policy_public_join?: number
  policy_strip_signatures?: number
  groups_enabled?: number
  management_enabled?: number
  blossom_enabled?: number
  livekit_enabled?: number
  push_enabled?: number
}

export type UpdateRelayInput = {
  subdomain?: string
  plan?: string
  info_name?: string
  info_icon?: string
  info_description?: string
  policy_public_join?: number
  policy_strip_signatures?: number
  groups_enabled?: number
  management_enabled?: number
  blossom_enabled?: number
  livekit_enabled?: number
  push_enabled?: number
}

export type Tenant = {
  pubkey: string
  nwc_url: string
  created_at: number
  billing_anchor: number
}

export type Invoice = {
  id: string
  tenant: string
  status: string
  amount: number
  created_at: number
  attempted_at: number
  error: string
  closed_at: number
  sent_at: number
  paid_at: number
  bolt11: string
  period_start: number
  period_end: number
}

export type Activity = {
  id: string
  tenant: string
  created_at: number
  activity_type: string
  resource_type: string
  resource_id: string
}

export type Identity = {
  pubkey: string
  is_admin: boolean
}

let authCache: AuthCache | undefined

export async function makeAuth(): Promise<string | undefined> {
  const current = getAccount() as { pubkey: string; signer: { signEvent: (e: unknown) => Promise<{ pubkey: string; sig: string; [k: string]: unknown }> } } | undefined
  if (!current) return undefined

  const now = Date.now()
  if (authCache && authCache.pubkey === current.pubkey && authCache.expiresAt > now) {
    return authCache.value
  }

  const event = await current.signer.signEvent({
    kind: 27235,
    content: "",
    created_at: Math.floor(now / 1000),
    tags: [["u", API_URL]],
  })

  const value = `Nostr ${btoa(JSON.stringify(event))}`
  authCache = {
    pubkey: current.pubkey,
    value,
    expiresAt: now + 10 * 60 * 1000,
  }
  return value
}

export async function callApi<TRequest = unknown, TResponse = unknown>(
  method: string,
  path: string,
  body?: TRequest,
): Promise<TResponse> {
  const auth = await makeAuth()
  const url = new URL(path, API_URL).toString()

  const response = await fetch(url, {
    method,
    headers: {
      ...(auth ? { Authorization: auth } : {}),
      "Content-Type": "application/json",
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  })

  if (!response.ok) {
    let message = `Request failed (${response.status})`
    try {
      const payload = await response.json() as { error?: string }
      if (payload.error) message = payload.error
    } catch {
      // ignore invalid/non-json body
    }
    throw new ApiError(message, response.status)
  }

  if (response.status === 204) return undefined as TResponse

  const payload = await response.json() as ApiOk<TResponse>
  return payload.data
}

export async function listPlans(): Promise<Plan[]> {
  const url = new URL("/plans", API_URL).toString()
  const response = await fetch(url)
  if (!response.ok) throw new ApiError(`Failed to load plans (${response.status})`, response.status)
  const payload = await response.json() as { data: Plan[] }
  return payload.data
}

export function getIdentity() {
  return callApi<undefined, Identity>("GET", "/identity")
}

export function getPlan(id: string) {
  return callApi<undefined, Plan>("GET", `/plans/${id}`)
}

export function listTenants() {
  return callApi<undefined, Tenant[]>("GET", "/tenants")
}

export function getTenant(pubkey: string) {
  return callApi<undefined, Tenant>("GET", `/tenants/${pubkey}`)
}

export function createTenant() {
  return callApi<undefined, Tenant>("POST", "/tenants")
}

export function listTenantRelays(pubkey: string) {
  return callApi<undefined, Relay[]>("GET", `/tenants/${pubkey}/relays`)
}

export function listTenantInvoices(pubkey: string) {
  return callApi<undefined, Invoice[]>("GET", `/tenants/${pubkey}/invoices`)
}

export function updateTenantBilling(pubkey: string, billing: BillingInput) {
  return callApi<BillingInput, BillingInput>("PUT", `/tenants/${pubkey}/billing`, billing)
}

export function listRelays() {
  return callApi<undefined, Relay[]>("GET", "/relays")
}

export function getRelay(id: string) {
  return callApi<undefined, Relay>("GET", `/relays/${id}`)
}

export function listRelayActivity(id: string) {
  return callApi<undefined, Activity[]>("GET", `/relays/${id}/activity`)
}

export function createRelay(input: CreateRelayInput) {
  return callApi<CreateRelayInput, Relay>("POST", "/relays", input)
}

export function updateRelay(id: string, input: UpdateRelayInput) {
  return callApi<UpdateRelayInput, Relay>("PUT", `/relays/${id}`, input)
}

export function deactivateRelay(id: string) {
  return callApi<undefined, void>("POST", `/relays/${id}/deactivate`)
}

export function listInvoices() {
  return callApi<undefined, Invoice[]>("GET", "/invoices")
}

export function getInvoice(id: string) {
  return callApi<undefined, Invoice>("GET", `/invoices/${id}`)
}
