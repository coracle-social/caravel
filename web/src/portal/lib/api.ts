import type { Identity, MemberInterval, Membership, MembershipStatus, RedirectUrl, RelayProfile } from "@/shared/lib/api"
import { request, unwrap } from "@/shared/lib/http"
import { session } from "@/portal/lib/session"

// What a relay sells memberships for.
export type Offer = RelayProfile & {
  price: number
  interval: MemberInterval
  accepting_payment: boolean
}

// The part of a membership its member sees.
export type MembershipView = Pick<Membership, "status" | "current_period_end">

// Every membership a member holds, with what each relay offers.
export type MemberAccount = {
  pubkey: string
  memberships: { offer: Offer; membership: MembershipView }[]
}

export type Invite = {
  offer: Offer
  status: MembershipStatus
  bound: boolean
  invite_url: string | null
}

// Calls the API with the member's session when there is one.
async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const token = session()
  const response = await request(path, {
    method,
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  return unwrap<T>(response)
}

export function getOffer(relayId: string) {
  return call<Offer>("GET", `/relays/${relayId}/offer`)
}

export function getAccount() {
  return call<MemberAccount>("GET", "/account")
}

export function getIdentity() {
  return call<Identity>("GET", "/identity")
}

export function getMembership(relayId: string) {
  return call<{ membership: MembershipView | null }>("GET", `/relays/${relayId}/membership`)
}

export function createCheckout(relayId: string, returnUrl?: string) {
  return call<RedirectUrl>("POST", `/relays/${relayId}/membership/checkout`, { return_url: returnUrl })
}

export function reconcileCheckout(relayId: string, checkout: string) {
  return call<{ paid: boolean }>("POST", `/relays/${relayId}/membership/reconcile`, { checkout })
}

export function cancelRenewal(relayId: string) {
  return call<MembershipView>("POST", `/relays/${relayId}/membership/cancel`)
}

// Buy an invite to a relay without a Nostr account.
export function createInvite(relayId: string, returnUrl?: string) {
  return call<RedirectUrl>("POST", `/relays/${relayId}/invites`, { return_url: returnUrl })
}

export function getInvite(code: string) {
  return call<Invite>("GET", `/invites/${code}`)
}

export function reconcileInvite(code: string, checkout: string) {
  return call<{ paid: boolean }>("POST", `/invites/${code}/reconcile`, { checkout })
}
