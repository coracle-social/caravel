---
name: web
description: Architecture and conventions for Caravel's web apps in web/ — three SolidJS + Vite + TypeScript single-page apps (the tenant dashboard, the admin dashboard, and the member portal each relay serves on its own host) built from one project and embedded in the binary. Covers the shared/tenant/admin/portal layout, how the server picks an app and writes its configuration into the page, links between the two dashboards, the `@/` import alias, strict TS rules (verbatimModuleSyntax), the API client and session tokens, createResource reads and optimistic writes, lazy tenant provisioning, the post-paid billing prompts, applesauce Nostr singletons, the Tailwind v4 brand remap (blue utilities render brown), the shared component kit, and the `pnpm run build` verification gate. Use this whenever working anywhere in web/ — adding a page, hook, API call, component, or style.
---

# Caravel's web apps

`web/` is one Vite project that builds three single-page apps, each with its own HTML entry:

- **the tenant dashboard** (`tenant.html`, `src/tenant`), on the platform host;
- **the admin dashboard** (`admin.html`, `src/admin`), on the platform host under `/admin`;
- **the member portal** (`portal.html`, `src/portal`), on each relay's own host, where members pay for, renew and manage memberships and buy invites.

`src/shared` holds what the apps share. The dashboards share nearly everything; the portal keeps its own pages, components and member API calls, and shares the configuration, the API transport, the wire types, the formatters and a few hooks. The deep, lookup-style topics live in `references/`.

## How the apps are served

The build lands in `web/dist`, which the `caravel` binary embeds. The server picks an app by host and path (the `architecture` skill describes host dispatch) and writes its runtime configuration into the page as JSON: the API's url, the relay domains, the platform's name and logo, Flotilla's url, and for the portal, the relay it is served for. `shared/lib/config.ts` reads it, falling back to the build's `VITE_*` variables under `vite dev`, where no server writes the page. Nothing is configured at build time, so one build runs anywhere.

The two dashboards are separate apps that look and navigate separately: the tenant dashboard frames its signed-in pages in a dark sidebar with the billing banner, and the admin dashboard in a dark bar across the top marked Admin. Their only link is the tenant sidebar's Administration item, shown to admins, which is marked `rel="external"` because each app's router takes over every same-origin link and has no route for the other's paths. Under `vite dev`, a middleware in `vite.config.ts` picks the app per path the way the server does, and the portal runs under `/portal` with `?relay=<id>` standing in for its relay.

## The stack

- **SolidJS, not React.** Use `class`, not `className`, and `Show`, `For`, `Switch` and `Match` for conditional and list rendering. Read props lazily as `props.x`; destructuring props breaks reactivity. State is signals, resources, memos and effects.
- **Reads are SolidJS resources** over the API client's functions; there is no query library.
- **Login is built on applesauce** accounts and signers. The portal signs in with applesauce signers alone and keeps only its session token.

## Where things live

Each app directory holds its `index.tsx` entry, its `App.tsx` route table, its `pages/`, and `components/` for UI only it uses. Shared UI lives in `shared/components`, with feature folders for login, payment, the account page and relays; page-level components both dashboards route to (login, editing a relay) live in `shared/views`. Non-UI code lives in `shared/lib`, one module per concept:

- **`config.ts`** — the runtime configuration.
- **`http.ts`** — the API transport every app uses: the error type, sending a request, unwrapping the response envelope, and buying a session token with one NIP-98 signature.
- **`api.ts`** — the dashboards' wire types, the session tokens they keep per pubkey, and one function per API route. The portal's member routes are in `portal/lib/api.ts`, which reuses these types.
- **`account.ts`** — the signed-in account, which the client reads for its session.
- **`state.ts`** — the applesauce singletons, restoring and following the signed-in account, the identity, plans and billing resources, and the toast.
- **`profiles.ts`** — reading Nostr profiles; **`relays.ts`** — relay resources and creating a relay; **`billing.ts`** — the billing prompts and status; **`format.ts`** — every display formatter, error messages included.

The rest of `shared/lib` is small pure modules and `use*` hooks. A concrete inventory and the route tables are in [references/file-inventory.md](references/file-inventory.md).

Where a new thing goes: a route page goes in its app's `pages/`; UI one app uses goes in its `components/`; UI several apps use goes in `shared/components/`; a backend call becomes a function in `api.ts`, or `portal/lib/api.ts` for member routes; a reusable stateful behavior becomes a `use*` hook; a rule or derivation becomes a function in the module that owns its concept.

## Naming, imports, and TypeScript rules

- **Files.** Components, pages and views are PascalCase `.tsx` with a default-exported function named after the file; a component file may also export small siblings it owns. Lib modules are camelCase `.ts` with named exports.
- **Imports always use the `@/` alias** for `src/`; there are no relative imports. The alias is configured in both `vite.config.ts` and `tsconfig.app.json`.
- **One export site per symbol.** There are no barrel files or re-export shims; when something moves, every import moves with it (root `AGENTS.md`).
- **Strict TS.** `verbatimModuleSyntax` is on, so type-only imports are marked as such. Unused locals and parameters fail the build.
- **One type per concept.** Wire types are declared once, in `api.ts`, and variants are derived from them: the relay settings a tenant may write are a partial pick of the relay, the portal's offer extends the relay profile the portal is served with, and a member's view of a membership is a pick of the membership. Relay settings are `0`/`1` flags on the wire, typed as such, and a relay's status, an activity's type and an invoice's status are unions.
- **`tenant_pubkey` naming**, and don't over-DRY (root `AGENTS.md`).
- **Comments** are one line where possible: a component or function says what it is, and a comment inside code explains only what the code can't.

## Talking to the API

Every request goes through `http.ts`, which throws the API's own error message with its status when a request fails, so callers can check for a status such as 401 or 404. The dashboards' client in `api.ts` adds the signed-in account's session token, signing in once when it has none and again once if the server rejects it; the portal adds its member's token. Both build urls by appending the path to the API's url, so an API served under a path on the platform host works. [references/data-and-state-lifecycle.md](references/data-and-state-lifecycle.md) has the session mechanics.

Pages read through resources, with the id as the resource's source so it refetches when the id changes, and render them with `ResourceState`, `useMinLoading` and `Show`. A resource throws when read after it failed, so code that only wants a value reads it through `loaded`. Writes to a relay go through `useRelayActions`: it updates the page's copy first, writes, refetches, and on failure restores the copy and reports in the toast. A redirect to a page the API names, such as Stripe Checkout or the billing portal, goes through `useRedirect`, which tracks the redirect in flight and its error.

## Global state and lazy tenant provisioning

A tenant row is created lazily. When an account becomes active, `state.ts` locks the billing reads, makes the pubkey a tenant (idempotently), and unlocks them, because reads of the tenant and its draft invoice fail until it exists. Any new tenant-scoped read is gated the same way, and code that changes billing calls `refetchBilling` when the banner should notice.

After a tenant picks a paid plan, whether creating a relay or upgrading one, `PostPaidPrompts` asks them to pay the invoice it opened and then to set up automatic payments, unless they already pay automatically; the banner stays quiet while it does. The billing domain itself is the `billing-model` skill's; the portal and member pricing are the `memberships` skill's.

## Nostr

`state.ts` holds three applesauce singletons: the event store, the relay pool and the account manager. The account signal follows the account manager; activate accounts through the manager, never by setting the signal. Profiles are read with the hooks in `profiles.ts`, which subscribe to the event store and fetch from each pubkey's outbox relays; a list that shows many profiles fetches them in one batch and its items subscribe without fetching. `profileName` and `Avatar` are how a profile is shown. Pubkeys are hex strings, shortened for display with the helpers in `pubkey.ts`.

## Routing and auth gating

Each dashboard declares its routes in its `App.tsx` under one router whose root is its own `Layout`. The tenant dashboard wraps its protected pages with `requireTenant`, which waits for the identity resource and sends everyone else home; the admin dashboard's `Layout` gates every page itself, showing the login when signed out and a notice to a signed-in account that isn't an admin. The gates only decide what renders; the API authorizes every request. Login is a component, shown as a dialog from the tenant dashboard's home page and as the admin dashboard's whole page, never a route. The portal's routes sit at the root of its relay's host.

## Styling and the component kit

Styling is Tailwind v4 utility classes inline, configured CSS-first in `shared/index.css` for the dashboards and `portal/index.css` for the portal.

**The pitfall:** the dashboards' stylesheet remaps the whole blue scale to the brown brand palette, so blue utilities render brown and are the intended brand accent. Don't replace them with a brown hex.

Build from the kit rather than bespoke markup: `PageContainer`, `ResourceState` and `LoadingState`; `Modal`, which is a centered dialog unless given other classes, with `ModalHeader` for a titled dialog; `ConfirmDialog`, which takes the confirmation to ask and keeps it on screen while closing; `SegmentedTabs`; `StatusBadge`, which knows the colors of every status the apps show; `Avatar`; and the form pieces (`Field`, `ToggleField`, `ToggleButton`, `SearchInput`, `RelayForm`). Transient errors and successes go to the toast with `setToastMessage`; errors that belong to a form or dialog are shown in it.

## Building and verifying a change

There is no lint or test step for the web apps. The gate is the build: a strict typecheck of every app, then a production bundle of all three. Run `just build-web`, or `pnpm run build` inside `web/`. pnpm is the package manager. Never open a browser to check a change. [references/build-and-config.md](references/build-and-config.md) covers the build and adding a configuration value.
