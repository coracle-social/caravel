# Data and state lifecycle

The API transport, the session tokens, how the signed-in account is restored and followed, lazy tenant provisioning, the post-paid prompts, and the optimistic write. It lives in `web/src/shared/lib/http.ts`, `api.ts`, `account.ts`, `state.ts`, `useRelayActions.ts` and `shared/components/PostPaidPrompts.tsx`.

## The transport

`http.ts` is shared by all three apps and depends on nothing but the configuration. Sending a request joins the API's url, without a trailing slash, to the path. A response that isn't a success throws an `ApiError` carrying the status and the `error` field of the API's JSON body, or a fallback message the caller names, such as "Failed to load plans". A successful response is unwrapped from the API's `{ data, code }` envelope; a `204` has no payload. `hasStatus` tells whether an error is an `ApiError` with a given status.

Buying a session signs one kind-27235 event for exactly `POST <API_URL>/session`, with a `u` tag and a `method` tag, sends it as a `Nostr` authorization header, and returns the pubkey, token and expiry the API issues. A session lasts thirty days.

## The dashboards' sessions

`api.ts` keeps one session token per pubkey in `localStorage` under `caravel.sessions`, with its expiry. A request for the signed-in account uses its token, signing a new session when there is none or it has expired; concurrent requests share one sign-in. Signing uses the account's own signer. A request that comes back `401` forgets the token and is sent once more with a fresh one. With no account signed in, requests go without authorization and authenticated routes refuse them. `signOut` ends the session on the server and forgets it here.

Blobs, exports and imports go through the same authenticated sending, because an `<img>` or `<a href>` can't carry the session; an export arrives as a blob, an upload preview as an object URL the caller revokes, and an import reports what it refused rather than failing.

The portal keeps its single member's token in `localStorage` under `caravel.portal.session`, and a notification's link can carry a fresh one, which the relay page adopts and removes from the address bar. A member's page that reads with a token the API no longer accepts signs out.

## The signed-in account

`account.ts` holds only the signed-in account's signal, so the API client can read it without depending on the rest of the state. `state.ts` restores the saved accounts and the active one from `localStorage` when it loads, before any app renders, and follows the account manager's active account from then on: it sets the signal, saves the accounts, refetches the identity, and provisions the tenant.

## Lazy tenant provisioning

`ensureSessionTenant` makes the signed-in pubkey a tenant (`POST /tenants`, idempotent) and then, if that pubkey is still signed in, unlocks the billing reads by setting the pubkey they read for. Concurrent calls for one pubkey share a run. Each account switch locks the billing reads first, so they never run against a tenant that doesn't exist yet. Login waits for provisioning and removes the account again if it fails.

The billing resources (the tenant, its invoices, its relays and its draft invoice) read for that gated pubkey. `refetchBilling` refetches all four and toasts if any fails. `autopayBilling` brings billing up to date, and when the tenant has a payment method and no page is prompting for an invoice itself, pays every open invoice oldest first, then refetches; concurrent calls for one pubkey share a run. The billing page runs it on landing, since Stripe returns there.

## Post-paid prompts

A page that lets a tenant pick a paid plan holds a post-paid flow and renders `PostPaidPrompts`. Starting the flow brings the tenant's billing up to date; a tenant with a payment method needs nothing more. Otherwise the tenant is shown the newest invoice they owe, then asked to set up automatic payments, or asked straight away when there's no invoice. While the flow runs it marks billing as handled by the page, which quiets the banner and keeps autopay from paying the invoice under it. Closing each dialog refetches billing, and the page may act when the flow ends, as creating a relay does by opening it.

## The optimistic write

`useRelayActions` gives a relay's page its actions. A setting toggle or plan change updates the page's copy of the relay first, writes, and refetches; on failure it restores the copy and toasts the error. A free plan also turns off media storage and calls. Deactivating, reactivating, erasing and deleting (purging) run one at a time, refetch, and toast a failure; erasing and deleting also toast their success, and a page refetches its members after an erase.
