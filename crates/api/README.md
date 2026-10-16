# API

The headless JSON API, which the platform's dashboards, relays' member portals and other clients such as Flotilla use to manage tenants, relays, billing and paid memberships.

Responses wrap their payload as `{"data": ..., "code": "ok"}`, and errors as `{"error": "...", "code": "..."}`. A `401` means the caller isn't authenticated, `403` that they may not do this, `404` that the thing named doesn't exist, `400` that it is in a state that doesn't allow the request (such as `relay-is-purged`), and `422` that the input isn't acceptable (such as `subdomain-exists`).

Boolean relay settings are written and read as `0` or `1`, and a relay's domain is carried as `zooid_domain`, as clients from before the monolith expect.

## Authentication

A client signs in once and then sends a bearer token:

- `POST /session` takes a NIP-98 `Authorization` header signed for exactly that request: its `u` tag is the API's base url followed by `/session`, its `method` tag is `POST`, and it was signed within a minute. It returns `{pubkey, token, expires_at}`. A session lasts thirty days.
- Every other authenticated route takes `Authorization: Bearer <token>`.
- `DELETE /session` ends the session whose token it is sent with.

For clients that predate sessions, an authenticated route also accepts a NIP-98 header whose `u` tag is the API's base url, with no `method` check, for fifteen minutes after it was signed.

Admins are the pubkeys in `ADMIN_PUBKEYS`. Unless marked otherwise, a route that names a tenant, relay or invoice may be called by that tenant or by an admin.

## Routes

Public:

- `GET /plans`, `GET /plans/{id}`
- `GET /domains/check?domain=` — `200` if the platform serves the host, for on-demand TLS
- `GET /relays/{id}/offer` — what a relay sells memberships for: `{relay_id, host, name, icon, description, price, interval, accepting_payment}`, with `price` in cents per `interval` (`month` or `year`); `404` for a relay that doesn't sell them
- `POST /relays/{id}/invites` — `{return_url?}`: buy an invite, for someone who may have no Nostr account yet, returning `{url}`, a Stripe Checkout url
- `GET /invites/{code}` — `{offer, status, claimed, invite_url}`; `invite_url` is the Flotilla link that accepts the invite, while it is paid for and unused
- `POST /invites/{code}/reconcile` — `{session}`: settle the checkout the buyer came back from, returning `{paid}`

Any authenticated pubkey:

- `GET /identity` — `{pubkey, is_admin}`
- `GET /account` — `{pubkey, memberships}`, each as `{offer, membership: {status, current_period_end}}`
- `GET /relays/{id}/membership` — `{membership}`, the caller's, or null
- `POST /relays/{id}/membership/checkout` — `{return_url?}`: join, or renew, returning `{url}`, a Stripe Checkout url
- `POST /relays/{id}/membership/reconcile` — `{session}`: settle the checkout the caller came back from, returning `{paid}`
- `POST /relays/{id}/membership/cancel` — stop renewing; access lasts until the paid period ends
- `POST /tenants` — become a tenant, idempotently: `{return_url}`, where the robot's DMs link to

Tenants:

- `GET`, `PUT /tenants/{pubkey}` — `PUT` takes `{nwc_url}`, stored encrypted; an empty string removes it
- `GET /tenants/{pubkey}/relays`, `GET /tenants/{pubkey}/invoices`
- `GET /tenants/{pubkey}/invoices/draft` — an unsaved invoice for what is owed so far this period, or null
- `GET /tenants/{pubkey}/invoices/draft/items`
- `POST /tenants/{pubkey}/reconcile` — bring billing up to date without charging
- `GET /tenants/{pubkey}/stripe/session?return_url=` — a Stripe billing portal url (the tenant only)
- `GET /tenants/{pubkey}/payouts` — `{account_id, ready, fee_percent}`: whether the tenant's members can pay them, and the platform's cut
- `POST /tenants/{pubkey}/payouts/onboard` — `{return_url?}`: a Stripe onboarding url, creating the tenant's payout account the first time (the tenant only)
- `POST /tenants/{pubkey}/payouts/dashboard` — a url into the tenant's Stripe payouts dashboard (the tenant only)
- `POST /relays` — `{tenant_pubkey, subdomain, zooid_domain, plan_id, ...settings}`
- `GET`, `PUT /relays/{id}` — `PUT` takes any of the settings
- `GET /relays/{id}/members` — `{members}`, the member pubkeys, the owner among them
- `PUT /relays/{id}/members/{pubkey}`, `DELETE /relays/{id}/members/{pubkey}` — add or remove a member; the owner can't be removed
- `GET /relays/{id}/memberships` — the relay's paid memberships, newest first
- `GET /relays/{id}/activity`
- `POST /relays/{id}/deactivate`, `/reactivate`, `/purge` — a purged relay's data is erased soon after
- `GET /relays/{id}/export` — the relay's events as JSONL
- `POST /relays/{id}/import` — a JSONL body of events, returning `{imported, invalid, skipped, rooms_rebuilt, problems}`
- `GET /invoices/{id}`, `GET /invoices/{id}/items`
- `POST /invoices/{id}/reconcile` — collect an open invoice now
- `POST /invoices/{id}/bolt11` — a payable Lightning invoice
- `POST /invoices/{id}/checkout?return_url=` — a Stripe Checkout url

Admins:

- `GET /tenants`, `GET /relays`, `GET /invoices`
- `POST /tenants/{pubkey}/ban`, `POST /tenants/{pubkey}/unban`
- `DELETE /sessions/{pubkey}` — end every session a pubkey holds
- `GET /moderation?since=&until=` — `{items, truncated}`: up to a thousand events and uploads every relay not owned by an admin saw in an inclusive window of `seen_at`, newest first, each as `{type, relay_id, seen_at, data}`
- `GET /moderation/blob/{relay_id}/{sha256}` — a blob's bytes

Relay settings are `subdomain`, `zooid_domain`, `plan_id`, `info_name`, `info_icon`, `info_description`, `policy_public_read`, `policy_anonymous_read`, `policy_public_write`, `policy_public_join`, `policy_strip_signatures`, `groups_enabled`, `management_enabled`, `members_can_invite`, `blossom_enabled`, `blossom_authenticated_read`, `livekit_enabled`, `push_enabled`, `custom_domain`, `member_price` (cents, `0` or at least `50`) and `member_interval` (`month` or `year`). A relay with a member price can't take public joins.
