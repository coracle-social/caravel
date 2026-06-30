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
  plan_id: PlanId
  status: string
  sync_error: string
  synced: number
  custom_domain: string
  custom_domain_verified: number
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
}

export type CreateRelayInput = {
  tenant_pubkey?: string
  subdomain: string
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
  nwc_is_set: boolean
  created_at: number
  billing_anchor: number | null
  stripe_customer_id: string
  stripe_payment_method_id: string | null
  nwc_error: string | null
  stripe_error: string | null
  churned_at: number | null
}

// Internal aliases derived from the wire shapes below — pure naming, no payload
// change. InvoiceMethod is the non-null members of Invoice.method; InvoiceStatus
// is the lifecycle status derived from the paid_at/voided_at timestamps.
export type InvoiceMethod = "nwc" | "stripe" | "oob"
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

// The single invoice autopay collects and the dashboard surfaces as "Pay now":
// the OLDEST open invoice with a positive balance, matching the backend's
// dunning order so the UI pays the same one collection targets. undefined when
// nothing is due. Canonical pick shared by useBillingStatus and autopayBilling.
export function selectPayableInvoice(invoices: Invoice[]): Invoice | undefined {
  return invoices
    .filter(inv => invoiceStatus(inv) === "open" && inv.amount > 0)
    .sort((a, b) => a.created_at - b.created_at)[0]
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

export function createTenant() {
  return callApi<undefined, Tenant>("POST", "/tenants")
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

export function reactivateRelay(id: string) {
  return callApi<undefined, void>("POST", `/relays/${id}/reactivate`)
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
// reconciled by reconcileInvoice once the tenant returns (or by the poll). The
// return URL is fixed to the account page server-side.
export function createInvoiceCheckout(invoiceId: string) {
  return callApi<undefined, { url: string }>("POST", `/invoices/${invoiceId}/checkout`)
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
