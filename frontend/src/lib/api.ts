import { accounts, API_URL } from "./nostr"
import { Relay as NostrRelay, RelayManagement } from "applesauce-relay"

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

type ApiOk<T> = {
  data: T
  code: string
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

function getActivePubkey(): string {
  const account = accounts.getActive() as { pubkey?: string } | undefined
  if (!account?.pubkey) throw new Error("Not logged in")
  return account.pubkey
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

  if (response.status === 204) return undefined as T
  const body = await response.json() as ApiOk<T>
  return body.data
}

async function ensureTenant() {
  try {
    await request<Tenant>("/tenants", { method: "POST" })
  } catch (e) {
    if (e instanceof ApiError && e.status === 422) return
    throw e
  }
}

export type Relay = {
  id: string
  tenant: string
  schema: string
  subdomain: string
  plan: string
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

export type TenantDetail = {
  tenant: Tenant
  relays: Relay[]
}

export type CreateRelayInput = {
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

export type AdminCheck = {
  is_admin: boolean
}

export function listTenantRelays() {
  return request<Relay[]>("/relays")
}

export async function getTenant() {
  const pubkey = getActivePubkey()
  await ensureTenant()
  return request<Tenant>(`/tenants/${pubkey}`)
}

export async function createTenantRelay(input: CreateRelayInput) {
  await ensureTenant()
  return request<Relay>("/relays", {
    method: "POST",
    body: JSON.stringify({ tenant: getActivePubkey(), ...input }),
  })
}

export function getTenantRelay(id: string) {
  return request<Relay>(`/relays/${id}`)
}

export function updateTenantRelay(id: string, input: UpdateRelayInput) {
  return request<Relay>(`/relays/${id}`, {
    method: "PUT",
    body: JSON.stringify(input),
  })
}

export function updateTenantRelayPlan(id: string, plan: string) {
  return updateTenantRelay(id, { plan })
}

export function deactivateTenantRelay(id: string) {
  return request<void>(`/relays/${id}/deactivate`, {
    method: "POST",
  })
}

export async function listTenantInvoices() {
  await ensureTenant()
  return request<Invoice[]>("/invoices")
}

export function updateTenantBilling(nwc_url: string) {
  return request<{ nwc_url: string }>(`/tenants/${getActivePubkey()}/billing`, {
    method: "PUT",
    body: JSON.stringify({ nwc_url }),
  })
}

export function adminListTenants() {
  return request<Tenant[]>("/tenants")
}

export async function adminCheck() {
  try {
    await request<Tenant[]>("/tenants")
    return { is_admin: true }
  } catch (e) {
    if (e instanceof ApiError && e.status === 403) return { is_admin: false }
    throw e
  }
}

export async function adminGetTenant(pubkey: string) {
  const [tenant, relays] = await Promise.all([
    request<Tenant>(`/tenants/${pubkey}`),
    request<Relay[]>(`/relays?tenant=${encodeURIComponent(pubkey)}`),
  ])
  return { tenant, relays }
}

export function adminUpdateTenantStatus(pubkey: string, status: string) {
  void pubkey
  void status
  throw new Error("Tenant status updates are not supported by this API")
}

export function adminListRelays() {
  return request<Relay[]>("/relays")
}

export function adminGetRelay(id: string) {
  return request<Relay>(`/relays/${id}`)
}

export function adminUpdateRelay(id: string, input: UpdateRelayInput) {
  return request<Relay>(`/relays/${id}`, {
    method: "PUT",
    body: JSON.stringify(input),
  })
}

export function adminDeactivateRelay(id: string) {
  return request<void>(`/relays/${id}/deactivate`, {
    method: "POST",
  })
}

export async function getRelayMemberCount(relayUrl: string): Promise<number | undefined> {
  const signer = getActiveSigner()
  const management = new RelayManagement(new NostrRelay(relayUrl), signer)

  try {
    const members = await management.listAllowedPubkeys()
    return members.length
  } catch {
    // ignore and fall back to unknown member count
  }

  return undefined
}
