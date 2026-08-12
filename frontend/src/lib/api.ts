import { sortBy } from "@welshman/lib"

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

type CreateTenantInput = {
  return_url: string
}

type UpdateTenantInput = {
  nwc_url?: string
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
  amount: number
  hidden: boolean
  members: number | null
  blossom: boolean
  livekit: boolean
}

export type PlanId = string

export type Relay = {
  id: string
  tenant_pubkey: string
  subdomain: string
  zooid_domain: string
  plan_id: PlanId
  status: string
  sync_error: string
  synced: number
  custom_domain: string
  custom_domain_verified: number
  redirect_from: string[]
  info_name: string
  info_icon: string
  info_description: string
  policy_public_read: number
  policy_public_write: number
  policy_public_join: number
  policy_strip_signatures: number
  groups_enabled: number
  management_enabled: number
  blossom_enabled: number
  livekit_enabled: number
  push_enabled: number
  purged_at: number | null
}

// The backend keeps a purged relay's original status, so derive the display
// status: purged once purged_at is set (the relay's data is deleted for good),
// otherwise the stored status.
export function relayStatus(relay: Pick<Relay, "status" | "purged_at">): string {
  if (relay.purged_at != null) return "purged"
  return relay.status
}

export type CreateRelayInput = {
  tenant_pubkey?: string
  subdomain: string
  // Which of the platform's ZOOID_DOMAINS the relay's canonical host sits under.
  // The backend rejects anything it doesn't serve.
  zooid_domain: string
  plan_id: string
  info_name?: string
  info_icon?: string
  info_description?: string
  policy_public_read?: number
  policy_public_write?: number
  policy_public_join?: number
  policy_strip_signatures?: number
  groups_enabled?: number
  management_enabled?: number
  blossom_enabled?: number
  livekit_enabled?: number
  push_enabled?: number
  custom_domain?: string
}

export type UpdateRelayInput = {
  subdomain?: string
  zooid_domain?: string
  plan_id?: string
  info_name?: string
  info_icon?: string
  info_description?: string
  policy_public_read?: number
  policy_public_write?: number
  policy_public_join?: number
  policy_strip_signatures?: number
  groups_enabled?: number
  management_enabled?: number
  blossom_enabled?: number
  livekit_enabled?: number
  push_enabled?: number
  custom_domain?: string
}

export type Tenant = {
  pubkey: string
  return_url: string
  nwc_is_set: boolean
  created_at: number
  billing_anchor: number | null
  stripe_customer_id: string
  stripe_payment_method_id: string | null
  nwc_error: string | null
  stripe_error: string | null
  churned_at: number | null
  banned_at: number | null
}

// A tenant's display status, derived from its lifecycle timestamps: banned
// (admin action, terminal for its relays) takes precedence over delinquent
// (billing churn), which takes precedence over active.
export function tenantStatus(tenant: Pick<Tenant, "churned_at" | "banned_at">): string {
  if (tenant.banned_at != null) return "banned"
  if (tenant.churned_at != null) return "delinquent"
  return "active"
}

// Internal aliases derived from the wire shapes below — pure naming, no payload
// change. InvoiceMethod is the non-null members of Invoice.method; InvoiceStatus
// is the lifecycle status derived from the paid_at/voided_at timestamps.
export type InvoiceMethod = "nwc" | "stripe" | "oob" | "comp"
export type InvoiceStatus = "open" | "paid" | "void"

export type Invoice = {
  id: string
  tenant_pubkey: string
  amount: number
  period_start: number
  period_end: number
  created_at: number
  paid_at: number | null
  voided_at: number | null
  method: InvoiceMethod | null
}

export type InvoiceItem = {
  id: string
  invoice_id: string | null
  activity_id: string | null
  tenant_pubkey: string
  relay_id: string
  plan_id: string
  amount: number
  description: string
  created_at: number
  voided_at: number | null
}

export type Bolt11 = {
  id: string
  invoice_id: string
  lnbc: string
  msats: number
  created_at: number
  expires_at: number
  settled_at: number | null
}

// The backend models an invoice's lifecycle as timestamps rather than a status
// field, so derive the display status from them: paid once paid_at is set, void
// once voided_at is set, otherwise still open.
export function invoiceStatus(invoice: Pick<Invoice, "paid_at" | "voided_at">): InvoiceStatus {
  if (invoice.paid_at != null) return "paid"
  if (invoice.voided_at != null) return "void"
  return "open"
}

// Every invoice that can be collected — open with a positive balance — oldest
// first, matching the backend's dunning order so the UI pays the same invoices
// collection targets, in the same order. Canonical filter shared by
// useBillingStatus and autopayBilling, which collects all of them.
export function listPayableInvoices(invoices: Invoice[]): Invoice[] {
  return sortBy(
    inv => inv.created_at,
    invoices.filter(inv => invoiceStatus(inv) === "open" && inv.amount > 0),
  )
}

export type Activity = {
  id: string
  tenant_pubkey: string
  created_at: number
  activity_type: string
  resource_type: string
  resource_id: string
}

export type Identity = {
  pubkey: string
  is_admin: boolean
}

export type NostrEvent = {
  id: string
  pubkey: string
  created_at: number
  kind: number
  tags: string[][]
  content: string
  sig: string
}

export type Upload = {
  sha256: string
  url: string
  type: string
  size: number
  pubkey: string
  created_at: number
}

// One reviewable item from a relay. `tenant` is the relay id it was published
// to, so it links straight to that relay's admin page.
export type ModerationItem =
  | { type: "event"; tenant: string; data: NostrEvent }
  | { type: "upload"; tenant: string; data: Upload }

export type ModerationPage = {
  items: ModerationItem[]
  truncated: boolean
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

// Create the calling pubkey's tenant. `return_url` is where the robot's DMs
// send them, so it's this app's account page. Idempotent — an existing tenant
// comes back with the return_url it was first created with.
export function createTenant(returnUrl: string) {
  return callApi<CreateTenantInput, Tenant>("POST", "/tenants", { return_url: returnUrl })
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

export function listTenantRelays(pubkey: string) {
  return callApi<undefined, Relay[]>("GET", `/tenants/${pubkey}/relays`)
}

export function listTenantInvoices(pubkey: string) {
  return callApi<undefined, Invoice[]>("GET", `/tenants/${pubkey}/invoices`)
}

// Reconcile a tenant's billing: sync the Stripe payment method (picking up a
// card added via the portal), fold billable activity into invoice items, renew
// the current period, and cut an invoice for any outstanding balance. Does not
// attempt payment. Returns the reconciled tenant.
export function reconcileTenant(pubkey: string) {
  return callApi<undefined, Tenant>("POST", `/tenants/${pubkey}/reconcile`)
}

// The draft is a synthetic Invoice (id "draft", empty lifecycle fields) summing
// the current period's not-yet-billed items, or null when there's nothing due.
export function getDraftInvoice(pubkey: string) {
  return callApi<undefined, Invoice | null>("GET", `/tenants/${pubkey}/invoices/draft`)
}

// The draft's line items, fetched by tenant since its sentinel id has no row in
// the per-invoice items endpoint. Lets the draft render an itemized PDF.
export function listDraftInvoiceItems(pubkey: string) {
  return callApi<undefined, InvoiceItem[]>("GET", `/tenants/${pubkey}/invoices/draft/items`)
}

export function updateTenant(pubkey: string, input: UpdateTenantInput) {
  return callApi<UpdateTenantInput, Tenant>("PUT", `/tenants/${pubkey}`, input)
}

export function listRelays() {
  return callApi<undefined, Relay[]>("GET", "/relays")
}

export function listInvoices() {
  return callApi<undefined, Invoice[]>("GET", "/invoices")
}

export function getRelay(id: string) {
  return callApi<undefined, Relay>("GET", `/relays/${id}`)
}

export function listRelayMembers(id: string) {
  return callApi<undefined, { members: string[] }>("GET", `/relays/${id}/members`)
}

export function listRelayActivity(id: string) {
  return callApi<undefined, { activity: Activity[] }>("GET", `/relays/${id}/activity`)
}

// A page of the newest events and uploads across every relay within a window,
// newest first. Page backwards inside a window by moving `until` down to the
// created_at of the last item received; `truncated` reports whether the window
// still holds more than what came back.
export function listModeration(params: { since?: number; until?: number } = {}) {
  const query = new URLSearchParams()
  if (params.since !== undefined) query.set("since", String(params.since))
  if (params.until !== undefined) query.set("until", String(params.until))

  const search = query.toString()

  return callApi<undefined, ModerationPage>("GET", `/moderation${search ? `?${search}` : ""}`)
}

// Blobs come through the API rather than straight from the relay, which serves
// its own blobs only to its members — not something an admin reviewing other
// people's relays is. Since <img> can't carry the NIP-98 auth header, fetch the
// bytes here and hand back an object URL; the caller owns revoking it.
export async function fetchModerationBlob(relayId: string, sha256: string): Promise<string> {
  const auth = await makeAuth()
  const url = new URL(`/moderation/blob/${relayId}/${sha256}`, API_URL).toString()

  const response = await fetch(url, {headers: auth ? {Authorization: auth} : {}})

  if (!response.ok) {
    throw new ApiError(`Failed to load blob (${response.status})`, response.status)
  }

  return URL.createObjectURL(await response.blob())
}

export function reactivateRelay(id: string) {
  return callApi<undefined, void>("POST", `/relays/${id}/reactivate`)
}

// Ban a tenant (admin only): blocks relay creation, cancels billing, and
// purges all of the tenant's relays. Returns the updated tenant.
export function banTenant(pubkey: string) {
  return callApi<undefined, Tenant>("POST", `/tenants/${pubkey}/ban`)
}

// Lift a tenant's ban (admin only). Purged relays are not restored.
export function unbanTenant(pubkey: string) {
  return callApi<undefined, Tenant>("POST", `/tenants/${pubkey}/unban`)
}

// Purge a relay (admin only): permanently deletes its data and config from the
// hosting backend. Terminal — a purged relay can't be re-activated.
export function purgeRelay(id: string) {
  return callApi<undefined, void>("POST", `/relays/${id}/purge`)
}

export function createPortalSession(pubkey: string, returnUrl?: string) {
  const query = returnUrl ? `?return_url=${encodeURIComponent(returnUrl)}` : ""
  return callApi<undefined, { url: string }>("GET", `/tenants/${pubkey}/stripe/session${query}`)
}

// Idempotently create a payable bolt11 for an invoice (reusing a valid existing
// one) and return it. No reconciliation — settlement is detected by
// reconcileInvoice. The lnbc string is the data the QR needs.
export function ensureInvoiceBolt11(invoiceId: string) {
  return callApi<undefined, Bolt11>("POST", `/invoices/${invoiceId}/bolt11`)
}

// Open a hosted Stripe Checkout session to pay a single invoice by card,
// reusing a valid pending one. Unlike the off-session charge, Checkout can
// satisfy a 3D Secure challenge. Returns the URL to redirect to; the payment is
// reconciled by reconcileInvoice once the tenant returns (or by the poll) to
// returnUrl, which Stripe sends them back to on success or cancel.
export function createInvoiceCheckout(invoiceId: string, returnUrl: string) {
  const query = `?return_url=${encodeURIComponent(returnUrl)}`
  return callApi<undefined, { url: string }>("POST", `/invoices/${invoiceId}/checkout${query}`)
}

// Reconcile and collect an open invoice: ensure a payable bolt11 exists, then
// run the payment cascade (NWC, then an out-of-band Lightning settle, then a
// saved card). Caller-initiated, so no dunning DM and no churn. Returns the
// refreshed invoice (paid_at set once collected).
export function reconcileInvoice(invoiceId: string) {
  return callApi<undefined, Invoice>("POST", `/invoices/${invoiceId}/reconcile`)
}

export function listInvoiceItems(invoiceId: string) {
  return callApi<undefined, InvoiceItem[]>("GET", `/invoices/${invoiceId}/items`)
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

export function getInvoice(id: string) {
  return callApi<undefined, Invoice>("GET", `/invoices/${id}`)
}
