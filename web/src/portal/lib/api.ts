import { API_URL } from "@/shared/lib/config"
import { session } from "@/portal/lib/session"

export type Interval = "month" | "year"

export type MembershipStatus = "pending" | "active" | "canceled" | "lapsed"

// What a relay sells memberships for.
export type Offer = {
  relay_id: string
  host: string
  name: string
  icon: string
  description: string
  price: number
  interval: Interval
  accepting_payment: boolean
}

export type Membership = {
  status: MembershipStatus
  current_period_end: number | null
}

export type Invite = {
  offer: Offer
  status: MembershipStatus
  claimed: boolean
  invite_url: string | null
}

export class ApiError extends Error {
  status: number

  constructor(message: string, status: number) {
    super(message)
    this.name = "ApiError"
    this.status = status
  }
}

// Calls the API with the member's session when there is one, and unwraps the
// `{ data }` envelope.
async function call<T>(method: string, path: string, body?: unknown, authorization?: string): Promise<T> {
  const token = session()
  const auth = authorization ?? (token ? `Bearer ${token}` : undefined)

  const response = await fetch(`${API_URL}${path}`, {
    method,
    headers: {
      "Content-Type": "application/json",
      ...(auth ? { Authorization: auth } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  })

  const payload = await response.json().catch(() => ({})) as { data?: T; error?: string }

  if (!response.ok) {
    throw new ApiError(payload.error ?? `Request failed (${response.status})`, response.status)
  }

  return payload.data as T
}

export function getOffer(relayId: string) {
  return call<Offer>("GET", `/relays/${relayId}/offer`)
}

// Trade a NIP-98 header for a session standing for the signer.
export function createSession(authorization: string) {
  return call<{ pubkey: string; token: string }>("POST", "/session", undefined, authorization)
}

export function getAccount() {
  return call<{ pubkey: string; memberships: { offer: Offer; membership: Membership }[] }>("GET", "/account")
}

export function getIdentity() {
  return call<{ pubkey: string }>("GET", "/identity")
}

export function getMembership(relayId: string) {
  return call<{ membership: Membership | null }>("GET", `/relays/${relayId}/membership`)
}

export function createCheckout(relayId: string, returnUrl?: string) {
  return call<{ url: string }>("POST", `/relays/${relayId}/membership/checkout`, { return_url: returnUrl })
}

export function reconcileCheckout(relayId: string, checkout: string) {
  return call<{ paid: boolean }>("POST", `/relays/${relayId}/membership/reconcile`, { session: checkout })
}

export function cancelRenewal(relayId: string) {
  return call<Membership>("POST", `/relays/${relayId}/membership/cancel`)
}

// Buy an invite to a relay without a Nostr account.
export function createInvite(relayId: string, returnUrl?: string) {
  return call<{ url: string }>("POST", `/relays/${relayId}/invites`, { return_url: returnUrl })
}

export function getInvite(code: string) {
  return call<Invite>("GET", `/invites/${code}`)
}

export function reconcileInvite(code: string, checkout: string) {
  return call<{ paid: boolean }>("POST", `/invites/${code}/reconcile`, { session: checkout })
}
