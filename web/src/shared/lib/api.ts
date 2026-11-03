import { sortBy } from "@welshman/lib"
import type { IAccount } from "applesauce-accounts"
import { account } from "@/shared/lib/account"
import { createSession, hasStatus, request, unwrap } from "@/shared/lib/http"

export type Plan = {
  id: string
  name: string
  amount: number
  members: number | null
  blossom: boolean
  livekit: boolean
  storage_gb: number
  storage_overage: number
  av_hours: number
  av_overage: number
}

export type PlanId = string

// A relay setting the API reads and writes as 0 or 1.
export type Flag = 0 | 1

export type RelayFlag =
  | "policy_public_read"
  | "policy_anonymous_read"
  | "policy_public_write"
  | "policy_public_join"
  | "policy_strip_signatures"
  | "groups_enabled"
  | "management_enabled"
  | "members_can_invite"
  | "blossom_enabled"
  | "blossom_authenticated_read"
  | "livekit_enabled"
  | "push_enabled"

export type RelayStatus = "active" | "inactive" | "delinquent"

export type MemberInterval = "month" | "year"

export type Relay = Record<RelayFlag, Flag> & {
  id: string
  tenant_pubkey: string
  subdomain: string
  zooid_domain: string
  plan_id: PlanId
  status: RelayStatus
  custom_domain: string
  custom_domain_verified: Flag
  redirect_from: string[]
  info_name: string
  info_icon: string
  info_description: string
  // Cents per member_interval, or 0 if joining is free.
  member_price: number
  member_interval: MemberInterval
  pubkey: string
  created_at: number
  purged_at: number | null
  erased_at: number | null
}

// What a tenant may set on a relay.
export type RelaySettings = Partial<
  Pick<
    Relay,
    | RelayFlag
    | "subdomain"
    | "zooid_domain"
    | "plan_id"
    | "info_name"
    | "info_icon"
    | "info_description"
    | "custom_domain"
    | "member_price"
    | "member_interval"
  >
>

export type NewRelay = RelaySettings & Pick<Relay, "tenant_pubkey" | "subdomain" | "zooid_domain" | "plan_id">

// What a relay's member portal shows of it.
export type RelayProfile = {
  relay_id: string
  host: string
  name: string
  icon: string
  description: string
}

// A deleted relay keeps the status it had, so deleting shows only in purged_at.
export function relayStatus(relay: Pick<Relay, "status" | "purged_at">): RelayStatus | "deleted" {
  if (relay.purged_at != null) return "deleted"
  return relay.status
}

export function relayName(relay: Pick<Relay, "info_name" | "subdomain">) {
  return relay.info_name || relay.subdomain
}

// The host every relay has, under the platform's domain.
export function canonicalHost(relay: Pick<Relay, "subdomain" | "zooid_domain">) {
  return `${relay.subdomain}.${relay.zooid_domain}`
}

// The page a relay's members pay on: its portal, on its own host.
export function relayPortalUrl(relay: Pick<Relay, "subdomain" | "zooid_domain" | "custom_domain" | "custom_domain_verified">) {
  const host = relay.custom_domain_verified && relay.custom_domain ? relay.custom_domain : canonicalHost(relay)
  return `https://${host}/`
}

export type MembershipStatus = "pending" | "active" | "canceled" | "lapsed"

// A paid membership of a relay, whose invite has no pubkey until someone joins with it.
export type Membership = {
  id: string
  relay_id: string
  pubkey: string | null
  invite_code: string | null
  status: MembershipStatus
  current_period_end: number | null
  bound_at: number | null
  created_at: number
}

// Whether a tenant's members can pay them, and the platform's cut.
export type PayoutStatus = {
  account_id: string | null
  ready: boolean
  // Stripe's code for what it still needs, such as `requirements.past_due`.
  reason: string | null
  fee_percent: number
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

// Banned outranks delinquent (churned), which outranks active.
export function tenantStatus(tenant: Pick<Tenant, "churned_at" | "banned_at">): "banned" | "delinquent" | "active" {
  if (tenant.banned_at != null) return "banned"
  if (tenant.churned_at != null) return "delinquent"
  return "active"
}

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

export function invoiceStatus(invoice: Pick<Invoice, "paid_at" | "voided_at">): InvoiceStatus {
  if (invoice.paid_at != null) return "paid"
  if (invoice.voided_at != null) return "void"
  return "open"
}

// The invoices collection would charge, in the order it charges them.
export function listPayableInvoices(invoices: Invoice[]): Invoice[] {
  return sortBy(
    inv => inv.created_at,
    invoices.filter(inv => invoiceStatus(inv) === "open" && inv.amount > 0),
  )
}

export type ActivityType =
  | "create_relay"
  | "update_relay"
  | "activate_relay"
  | "deactivate_relay"
  | "purge_relay"
  | "mark_relay_delinquent"
  | "unmark_relay_delinquent"

export type Activity = {
  id: string
  tenant_pubkey: string
  created_at: number
  activity_type: ActivityType
  relay_id: string
  snapshot: { plan_id: PlanId; status: RelayStatus }
  billed_at: number | null
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

// How a member got into a relay.
export type MemberSource = "owner" | "manual" | "claim" | "public_join" | "payment" | "migrated"

// A relay member (NIP-43), with the ids of the roles they hold.
export type RelayMember = {
  pubkey: string
  source: MemberSource
  roles: string[]
  created_at: number
}

// A role a relay gives some of its members (NIP-43 kind 33534).
export type RelayRole = {
  id: string
  label: string
  description: string
  hue: number
  position: number
}

export type RelayMembers = {
  members: RelayMember[]
  roles: RelayRole[]
}

export type BannedPubkey = { pubkey: string; reason: string }
export type BannedEvent = { id: string; reason: string }

// Who may call which of a relay's NIP-86 methods.
export type MethodAssignee = { pubkey: string; methods: string[] }

// A member of a room (NIP-29): roles are labels, and permissions the moderation kinds they may publish.
export type RoomMember = {
  room_id: string
  pubkey: string
  roles: string[]
  permissions: number[]
  created_at: number
}

// A room on a relay: its metadata tags, pinned events, members and invite codes.
export type Room = {
  id: string
  tags: string[][]
  pins: string[][]
  created_at: number
  members: RoomMember[]
  invites: string[]
}

export type Upload = {
  sha256: string
  url: string
  type: string
  size: number
  pubkey: string
  created_at: number
}

// One reviewable item from a relay, and when that relay first stored it.
export type ModerationItem =
  | { type: "event"; relay_id: string; seen_at: number; data: NostrEvent }
  | { type: "upload"; relay_id: string; seen_at: number; data: Upload }

export type ModerationPage = {
  items: ModerationItem[]
  truncated: boolean
}

// One line of an imported JSONL file the relay could not store.
export type ImportProblem = {
  line?: number
  event_id?: string
  message: string
}

export type ImportResult = {
  imported: number
  invalid: number
  skipped?: number
  rooms_rebuilt?: number
  problems?: ImportProblem[]
}

// Where the API sends the browser next, such as a Stripe page.
export type RedirectUrl = {
  url: string
}

// A session token kept in this browser, expiring at `expiresAt` milliseconds.
type StoredSession = {
  token: string
  expiresAt: number
}

const SESSIONS_KEY = "caravel.sessions"

function readSessions(): Record<string, StoredSession> {
  try {
    return JSON.parse(localStorage.getItem(SESSIONS_KEY) || "{}") as Record<string, StoredSession>
  } catch {
    return {}
  }
}

function writeSession(pubkey: string, session: StoredSession | undefined) {
  const sessions = readSessions()
  if (session) {
    sessions[pubkey] = session
  } else {
    delete sessions[pubkey]
  }
  localStorage.setItem(SESSIONS_KEY, JSON.stringify(sessions))
}

const pendingSessions = new Map<string, Promise<StoredSession>>()

async function signIn(current: IAccount): Promise<StoredSession> {
  const { token, expires_at } = await createSession(current.signer)
  const session = { token, expiresAt: expires_at * 1000 }
  writeSession(current.pubkey, session)
  return session
}

// The signed-in account's bearer token, signing in once when it has none.
async function authorization(): Promise<string | undefined> {
  const current = account()
  if (!current) return undefined

  const cached = readSessions()[current.pubkey]
  if (cached && cached.expiresAt > Date.now()) return `Bearer ${cached.token}`

  let pending = pendingSessions.get(current.pubkey)
  if (!pending) {
    pending = signIn(current).finally(() => pendingSessions.delete(current.pubkey))
    pendingSessions.set(current.pubkey, pending)
  }

  return `Bearer ${(await pending).token}`
}

function forgetSession() {
  const current = account()
  if (current) writeSession(current.pubkey, undefined)
}

// End the current account's session on the server and here.
export async function signOut() {
  const current = account()
  const session = current && readSessions()[current.pubkey]
  if (session) {
    await request("/session", { method: "DELETE", headers: { Authorization: `Bearer ${session.token}` } }).catch(() => undefined)
  }
  forgetSession()
}

// Send an authenticated request, signing in again once if the session was revoked or expired.
async function authed(path: string, init: RequestInit = {}, failure?: string): Promise<Response> {
  const send = (auth?: string) =>
    request(path, { ...init, headers: { ...init.headers, ...(auth ? { Authorization: auth } : {}) } }, failure)

  const auth = await authorization()
  try {
    return await send(auth)
  } catch (e) {
    if (!hasStatus(e, 401) || !account()) throw e
    forgetSession()
    return send(await authorization())
  }
}

async function callApi<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await authed(path, {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  return unwrap<T>(response)
}

export async function listPlans(): Promise<Plan[]> {
  return unwrap<Plan[]>(await request("/plans", {}, "Failed to load plans"))
}

export function getIdentity() {
  return callApi<Identity>("GET", "/identity")
}

// Become a tenant, idempotently. `returnUrl` is where the robot's DMs link to.
export function createTenant(returnUrl: string) {
  return callApi<Tenant>("POST", "/tenants", { return_url: returnUrl })
}

export function listTenants() {
  return callApi<Tenant[]>("GET", "/tenants")
}

export function getTenant(pubkey: string) {
  return callApi<Tenant>("GET", `/tenants/${pubkey}`)
}

export function listTenantRelays(pubkey: string) {
  return callApi<Relay[]>("GET", `/tenants/${pubkey}/relays`)
}

export function listTenantInvoices(pubkey: string) {
  return callApi<Invoice[]>("GET", `/tenants/${pubkey}/invoices`)
}

// Bring a tenant's billing up to date without charging, returning the tenant.
export function reconcileTenant(pubkey: string) {
  return callApi<Tenant>("POST", `/tenants/${pubkey}/reconcile`)
}

// What's owed so far this period, as an unsaved invoice with the id "draft", or null.
export function getDraftInvoice(pubkey: string) {
  return callApi<Invoice | null>("GET", `/tenants/${pubkey}/invoices/draft`)
}

export function listDraftInvoiceItems(pubkey: string) {
  return callApi<InvoiceItem[]>("GET", `/tenants/${pubkey}/invoices/draft/items`)
}

export function updateTenant(pubkey: string, input: { nwc_url?: string }) {
  return callApi<Tenant>("PUT", `/tenants/${pubkey}`, input)
}

export function listRelays() {
  return callApi<Relay[]>("GET", "/relays")
}

export function listInvoices() {
  return callApi<Invoice[]>("GET", "/invoices")
}

export function getRelay(id: string) {
  return callApi<Relay>("GET", `/relays/${id}`)
}

export function listRelayMemberships(id: string) {
  return callApi<Membership[]>("GET", `/relays/${id}/memberships`)
}

export function getPayoutStatus(pubkey: string) {
  return callApi<PayoutStatus>("GET", `/tenants/${pubkey}/payouts`)
}

// A one-time Stripe onboarding link, which sends the tenant back to `returnUrl`.
export function openPayoutOnboarding(pubkey: string, returnUrl: string) {
  return callApi<RedirectUrl>("POST", `/tenants/${pubkey}/payouts/onboard`, { return_url: returnUrl })
}

export function openPayoutDashboard(pubkey: string) {
  return callApi<RedirectUrl>("POST", `/tenants/${pubkey}/payouts/dashboard`)
}

export function listRelayMembers(id: string) {
  return callApi<RelayMembers>("GET", `/relays/${id}/members`)
}

// Call one of the relay's NIP-86 methods, as its owner may call any of them.
export function callRelayMethod<T = true>(id: string, method: string, params: unknown[] = []) {
  return callApi<T>("POST", `/relays/${id}/management`, { method, params })
}

export function listRelayRooms(id: string) {
  return callApi<Room[]>("GET", `/relays/${id}/rooms`)
}

// Publish a NIP-29 moderation event about one of the relay's rooms, which the relay signs.
export function moderateRoom(id: string, kind: number, tags: string[][]) {
  return callApi<void>("POST", `/relays/${id}/rooms/moderate`, { kind, tags })
}

// A page of the relay's events, newest first, of some kinds or ids; the last event's created_at and id, as `until` and `after`, page back.
export function listRelayEvents(
  id: string,
  params: { kinds?: number[]; ids?: string[]; until?: number; after?: string; exclude_own?: boolean; limit?: number } = {},
) {
  const query = new URLSearchParams()
  if (params.kinds?.length) query.set("kinds", params.kinds.join(","))
  if (params.ids?.length) query.set("ids", params.ids.join(","))
  if (params.until !== undefined) query.set("until", String(params.until))
  if (params.after) query.set("after", params.after)
  if (params.exclude_own) query.set("exclude_own", "true")
  if (params.limit !== undefined) query.set("limit", String(params.limit))
  return callApi<NostrEvent[]>("GET", `/relays/${id}/events?${query}`)
}

export function listRelayActivity(id: string) {
  return callApi<{ activity: Activity[] }>("GET", `/relays/${id}/activity`)
}

// Events and uploads every relay saw in a window of seen_at, newest first; `until` pages back.
export function listModeration(params: { since?: number; until?: number } = {}) {
  const query = new URLSearchParams()
  if (params.since !== undefined) query.set("since", String(params.since))
  if (params.until !== undefined) query.set("until", String(params.until))

  const search = query.toString()

  return callApi<ModerationPage>("GET", `/moderation${search ? `?${search}` : ""}`)
}

// A blob as an object URL the caller revokes, read through the API since relays serve only members.
export async function fetchModerationBlob(relayId: string, sha256: string): Promise<string> {
  const response = await authed(`/moderation/blob/${relayId}/${sha256}`, {}, "Failed to load blob")
  return URL.createObjectURL(await response.blob())
}

// A relay's events as JSONL.
export async function exportRelayData(id: string): Promise<Blob> {
  const response = await authed(`/relays/${id}/export`, {}, "Failed to export relay data")
  return response.blob()
}

// Add the events in a JSONL file to a relay, which reports what it refused.
export async function importRelayData(id: string, file: Blob): Promise<ImportResult> {
  const response = await authed(
    `/relays/${id}/import`,
    { method: "POST", headers: { "Content-Type": "application/x-ndjson" }, body: file },
    "Failed to import relay data",
  )
  return unwrap<ImportResult>(response)
}

export function reactivateRelay(id: string) {
  return callApi<void>("POST", `/relays/${id}/reactivate`)
}

// Ban a tenant, deleting their relays and voiding their open invoices.
export function banTenant(pubkey: string) {
  return callApi<Tenant>("POST", `/tenants/${pubkey}/ban`)
}

export function unbanTenant(pubkey: string) {
  return callApi<Tenant>("POST", `/tenants/${pubkey}/unban`)
}

// Delete a tenant's relays and void their open invoices, without banning them.
export function deleteTenant(pubkey: string) {
  return callApi<Tenant>("POST", `/tenants/${pubkey}/delete`)
}

// Delete a relay for good, freeing its subdomain: its data is erased soon after.
export function purgeRelay(id: string) {
  return callApi<void>("POST", `/relays/${id}/purge`)
}

// Erase a relay's events, media, rooms and access lists, keeping its settings.
export function eraseRelay(id: string) {
  return callApi<void>("POST", `/relays/${id}/erase`)
}

export function createBillingPortalSession(pubkey: string, returnUrl?: string) {
  const query = returnUrl ? `?return_url=${encodeURIComponent(returnUrl)}` : ""
  return callApi<RedirectUrl>("GET", `/tenants/${pubkey}/stripe/session${query}`)
}

// A payable Lightning invoice for an invoice, reusing a valid one.
export function ensureInvoiceBolt11(invoiceId: string) {
  return callApi<Bolt11>("POST", `/invoices/${invoiceId}/bolt11`)
}

// A Stripe Checkout url for paying one invoice by card, which returns to `returnUrl`.
export function createInvoiceCheckout(invoiceId: string, returnUrl: string) {
  const query = `?return_url=${encodeURIComponent(returnUrl)}`
  return callApi<RedirectUrl>("POST", `/invoices/${invoiceId}/checkout${query}`)
}

// Collect an open invoice now, returning it.
export function reconcileInvoice(invoiceId: string) {
  return callApi<Invoice>("POST", `/invoices/${invoiceId}/reconcile`)
}

export function listInvoiceItems(invoiceId: string) {
  return callApi<InvoiceItem[]>("GET", `/invoices/${invoiceId}/items`)
}

export function createRelay(input: NewRelay) {
  return callApi<Relay>("POST", "/relays", input)
}

export function updateRelay(id: string, settings: RelaySettings) {
  return callApi<Relay>("PUT", `/relays/${id}`, settings)
}

export function deactivateRelay(id: string) {
  return callApi<void>("POST", `/relays/${id}/deactivate`)
}

export function getInvoice(id: string) {
  return callApi<Invoice>("GET", `/invoices/${id}`)
}
