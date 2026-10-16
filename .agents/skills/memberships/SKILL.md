---
name: memberships
description: How caravel sells memberships of relays that charge for them — member pricing on the relay, tenants' Stripe Connect payout accounts and the platform's fee, member checkouts and renewals, invites bought without an account and bound to whoever joins with them, how a membership drives the relay's access list, seats under the plan's member limit, lapses with a renewal notification, releasing memberships when a relay stops charging, the sweep that keeps everything in step with Stripe, NIP-11's payment fields, and the member API the portal uses. Use it whenever working on the core's memberships or payouts modules, the API's member routes, or anything about paid access to a relay.
---

# Memberships

A tenant can charge for membership of a relay: a price in cents, renewing monthly or yearly. A relay with a price must not take public joins, so its only ways in are a claim, being added by hand, or paying. Its NIP-11 document says payment is required, gives the relay's own host as `payments_url`, and states the fee.

Money moves through Stripe Connect. A tenant onboards a Connect Express account from their dashboard; once Stripe says it can take charges and pay out, the relay accepts payments. Each membership pays as its own Stripe customer, through a subscription created by Checkout on the platform's account, which transfers to the tenant's account less `MEMBER_FEE_PERCENT`. None of this touches the tenant's own billing ledger.

The test inventory in [references/merchant-tests.md](references/merchant-tests.md) maps every test from merchant, the sidecar this replaces, to the caravel test that covers it.

## A membership

A membership is pending until paid, then active, or canceled once its member stops renewal (it keeps access until its paid period ends), and finally lapsed, which is history: paying again opens a new membership. A relay and pubkey hold one live membership at a time.

A membership drives the relay's access list in the same transaction that changes it. Paying admits the member, recorded as a member by payment, and clears any lapse notification. A lapse removes the member only if payment is how they got in, so someone added by hand or let in by a claim keeps their place.

## Invites

Someone without a Nostr account can buy an invite. Its code is unguessable, and once paid it is registered as a claim on the relay. The first pubkey that joins with it, and holds no membership of the relay already, gets the membership: the binding happens inside the join's transaction, under a lock on the relay row, and the claim is revoked there, so two joins racing on one code bind it once. A joiner who already holds a membership is let in by the claim without spending it. An invite's page offers the Flotilla link that accepts it only while it is paid for and unused.

## Seats

A relay's plan may limit its members. A paid invite nobody has used holds a seat, so checkouts and joins count members plus unused invites against the limit. A checkout that couldn't be granted is refused before it is sold; a payment that completes anyway is admitted.

## The sweep

Every five minutes a sweep brings memberships in step with Stripe, so no webhook is needed and none can be missed. A paying membership is read from Stripe once per period, after its period ends: a renewed subscription carries on, a renewal paid through a newer checkout takes over, and anything else lapses it. A lapsed member gets a NOTIFY on the relay, for thirty days, linking to the relay's portal with a fresh session token, which proves they received it. Checkouts nobody came back from are settled from Stripe, or lapse after two days.

When a relay stops charging (its price set to zero, paused, or purged), the sweep releases its memberships: open checkouts expire, subscriptions are canceled with no refund, members stay, and unused invites stop working.

## The member API

The member portal and any other client use the API's member routes, signed in with a session token like every other API route: the caller's memberships, a relay's offer, checkout, settling a checkout on return, canceling renewal, and buying and settling invites. A checkout returns to the portal on the relay's own host, carrying the Checkout session for the page to settle and any `return_url` the client asked to be sent back to.
