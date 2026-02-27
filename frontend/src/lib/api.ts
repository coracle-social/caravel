import { accounts, API_URL } from "./nostr"

type NostrTag = string[]

type UnsignedEvent = {
  kind: number
  content: string
  created_at: number
  tags: NostrTag[]
}

type SignedEvent = UnsignedEvent & {
  id: string
  pubkey: string
  sig: string
}

type EventSigner = {
  signEvent(event: UnsignedEvent): Promise<SignedEvent>
}

export class ApiError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = "ApiError"
    this.status = status
  }
}

function getActiveSigner(): EventSigner {
  const account = accounts.getActive() as { signer?: EventSigner } | undefined
  if (!account?.signer) throw new Error("Not logged in")
  return account.signer
}

async function createNip98Header(url: string, method: string): Promise<string> {
  const signer = getActiveSigner()
  const event = await signer.signEvent({
    kind: 27235,
    content: "",
    created_at: Math.floor(Date.now() / 1000),
    tags: [
      ["u", url],
      ["method", method.toUpperCase()],
    ],
  })

  const encoded = btoa(JSON.stringify(event))
  return `Nostr ${encoded}`
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const method = (init?.method ?? "GET").toUpperCase()
  const url = new URL(path, API_URL).toString()
  const auth = await createNip98Header(url, method)

  const response = await fetch(url, {
    ...init,
    method,
    headers: {
      ...(init?.headers ?? {}),
      Authorization: auth,
      "Content-Type": "application/json",
    },
  })

  if (!response.ok) {
    let message = `Request failed (${response.status})`
    try {
      const body = await response.json() as { error?: string }
      if (body.error) message = body.error
    } catch {
      // ignored
    }
    throw new ApiError(message, response.status)
  }

  if (response.status === 204) {
    return undefined as T
  }

  return response.json() as Promise<T>
}

export type Relay = {
  id: string
  tenant: string
  name: string
  subdomain: string
  schema: string
  icon: string
  description: string
  plan: string
  status: string
}

export type Tenant = {
  pubkey: string
  status: string
  tenant_nwc_url: string
}

export type Invoice = {
  id: string
  tenant: string
  amount: number
  status: string
  created_at: string
  invoice: string
}

export type TenantDetail = {
  tenant: Tenant
  relays: Relay[]
}

export type UpdateRelayInput = {
  name: string
  subdomain: string
  icon: string
  description: string
  plan: string
}

export type AdminCheck = {
  is_admin: boolean
}

export function listTenantRelays() {
  return request<Relay[]>("/tenant/relays")
}

export function getTenant() {
  return request<Tenant>("/tenant")
}

export type CreateRelayInput = {
  name: string
  subdomain: string
  icon: string
  description: string
  plan: string
}

export function createTenantRelay(input: CreateRelayInput) {
  return request<Relay>("/tenant/relays", {
    method: "POST",
    body: JSON.stringify(input),
  })
}

export function getTenantRelay(id: string) {
  return request<Relay>(`/tenant/relays/${id}`)
}

export function updateTenantRelay(id: string, input: UpdateRelayInput) {
  return request<Relay>(`/tenant/relays/${id}`, {
    method: "PUT",
    body: JSON.stringify(input),
  })
}

export function deactivateTenantRelay(id: string) {
  return request<Relay>(`/tenant/relays/${id}/deactivate`, {
    method: "POST",
  })
}

export function listTenantInvoices() {
  return request<Invoice[]>("/tenant/invoices")
}

export function updateTenantBilling(tenant_nwc_url: string) {
  return request<Tenant>("/tenant/billing", {
    method: "PUT",
    body: JSON.stringify({ tenant_nwc_url }),
  })
}

export function adminListTenants() {
  return request<Tenant[]>("/admin/tenants")
}

export function adminCheck() {
  return request<AdminCheck>("/admin/check")
}

export function adminGetTenant(pubkey: string) {
  return request<TenantDetail>(`/admin/tenants/${pubkey}`)
}

export function adminUpdateTenantStatus(pubkey: string, status: string) {
  return request<Tenant>(`/admin/tenants/${pubkey}`, {
    method: "PUT",
    body: JSON.stringify({ status }),
  })
}

export function adminListRelays() {
  return request<Relay[]>("/admin/relays")
}

export function adminGetRelay(id: string) {
  return request<Relay>(`/admin/relays/${id}`)
}

export function adminUpdateRelay(id: string, input: UpdateRelayInput) {
  return request<Relay>(`/admin/relays/${id}`, {
    method: "PUT",
    body: JSON.stringify(input),
  })
}

export function adminDeactivateRelay(id: string) {
  return request<Relay>(`/admin/relays/${id}/deactivate`, {
    method: "POST",
  })
}
