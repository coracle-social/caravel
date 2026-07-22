---
name: frontend
description: Architecture and conventions for the Caravel frontend — a SolidJS + Vite + TypeScript app (NOT React, NOT TanStack Query, NOT nonboard despite the README). Covers the pages/components/lib layout, the `@/` import alias, strict TS rules (verbatimModuleSyntax), the createResource + api.ts data layer, lazy tenant provisioning, applesauce Nostr singletons, the Tailwind v4 brand-remap (blue utilities render brown), the shared component/modal/toast kit, and the `bun run build` verification gate. Use this whenever working anywhere in frontend/ — adding a page, hook, API call, component, or style — to follow house conventions and avoid stale-README traps.
---

# Caravel frontend

This is the map of the Caravel frontend: a SolidJS + Vite + TypeScript single-page app living under `frontend/src`. It explains how the frontend is organized and *why*, and points you at the code for the *how* — so reach for this for orientation and conventions, and reach for the modules it names for implementation detail. The deep, lookup-style topics live in `references/`.

One warning up front, because it is the single most dangerous thing about this codebase: **`frontend/README.md` is stale and lies about the stack.** Do not trust it for what libraries are in use, how to run the app, or what routes exist. Its specific traps are called out below.

## It's SolidJS, not React — and the README lies about the stack

Before touching anything, internalize the actual stack, because the wrong defaults here are exactly the ones an agent reaches for:

- **SolidJS, not React.** Use `class`, not `className`. Use `<Show>`/`<For>` for conditional and list rendering, not ternaries and `.map()`. Access props lazily as `props.foo` — never destructure props at the top, because that breaks reactivity (see `src/components/relay/PlanGatedToggle.tsx`, whose own header comment says so). State is `createSignal`/`createResource`/`createMemo`/`createEffect`.
- **Data fetching is hand-rolled `fetch` in `lib/api.ts` wrapped in SolidJS `createResource` — NOT TanStack Query.** `@tanstack/solid-query` is in `package.json` and the README, but it is imported *nowhere* in `src` (verified: zero references). Do not introduce it.
- **Login is built directly on `applesauce-accounts` / `applesauce-signers` — NOT "nonboard".** There is no such dependency; the README invented it.
- **`applesauce-wallet-connect` and `@tailwindcss/forms` are declared dependencies that are never imported.** NWC on the frontend is just a text input whose value is POSTed to the backend; form inputs are styled with explicit utility classes.

Bottom line: follow the `createResource` + applesauce patterns the codebase actually uses.

## Where things live

There are four top-level code directories under `src`, each with one job (plus an `assets/` dir and loose root files `App.tsx`, `index.tsx`, `index.css`, `global.d.ts`):

- **`pages/`** — route components. Subfolders group screens: `relays/` (tenant relay screens) and `admin/` (the admin console).
- **`components/`** — shared, reusable UI, with feature subfolders `login/`, `payment/`, `account/`, `relay/`.
- **`lib/`** — all non-UI logic.
- **`views/`** — holds only `Login.tsx` (see the quirk below).

Within `lib/`, four modules are the load-bearing pillars:

- **`api.ts`** — the *only* place backend wire types and `fetch` wrappers live.
- **`state.ts`** — app-lifetime singletons plus global signals/resources.
- **`hooks.ts`** — the `use*` `createResource` read hooks and the active-tenant action helpers.
- **`nostr.ts`** — the `useNostr()` DI seam for reaching the Nostr singletons.

The rest of `lib/` is camelCase pure-helper modules and `use*` hook files. One quirk to know: **Login lives in `src/views/Login.tsx`, not `pages/`** — it is the lone occupant of `views/`.

Where a *new* thing goes: a route page → `pages/`; shared UI → `components/`; a hook → `lib/use*.ts`; a backend call → an `api.ts` wrapper; pure decision logic → its own `lib` module. A concrete file inventory and the entrypoint/route wiring are in [references/file-inventory.md](references/file-inventory.md) if you need it.

## Naming, imports, and TypeScript rules

To compile and fit in, an edit must satisfy these:

- **File naming.** Components/pages/views are PascalCase `.tsx` with one default-exported function named after the file. Lib modules are camelCase `.ts` (no default exports). Hooks are camelCase `use*.ts`. Files that export multiple hooks/symbols use named exports (e.g. `PaymentSetupShell.tsx` exports `PaymentSetupShell` plus sibling components). Single-hook files are inconsistent: `useRelayToggles.ts` and `useMinLoading.ts` default-export, but `useInvoicePdf.ts` uses a named export — so don't assume a single-purpose hook default-exports.
- **Imports always use the `@/` alias** (maps to `src/`), e.g. `import RelayList from "@/pages/relays/RelayList"`. There is not a single relative `./`/`../` cross-module import in the tree (the only relative import is `index.tsx`'s `import "./index.css"`). The alias is configured in **two places that must stay in sync**: `vite.config.ts` and `tsconfig.app.json`.
- **No barrel `index` files.** The only `index.{ts,tsx,js,jsx}` under `src/` is the entrypoint `index.tsx`, which renders the app rather than re-exporting a module. Import each symbol from its concrete module. (`index.css` also exists, so `index.tsx` isn't literally the only `index.*` file.)
- **One canonical export per symbol — no re-export shims.** When code moves, update every call site (root `AGENTS.md`). Note one existing wrinkle: `hooks.ts` re-exports four `api.ts` wire types (`Activity`, `Invoice`, `Relay`, `Tenant`), so each is importable from two paths — **prefer `@/lib/api`** for wire types. `hooks.ts` also re-exports `ProfileContent`, but that type comes from `applesauce-core/helpers/profile` (not `api.ts`), so import it from there.
- **Strict TS.** `verbatimModuleSyntax` is on, so type-only symbols need `import type` or an inline `type` specifier, e.g. `import { invoiceStatus, type Invoice } from "@/lib/api"`. `noUnusedLocals`/`noUnusedParameters` and `erasableSyntaxOnly` are also on.
- **`tenant_pubkey` naming.** Name a tenant's pubkey `tenant_pubkey` on FK fields/inputs; the exception is already-tenant-scoped contexts (`Tenant.pubkey`, `getTenant(pubkey)`). See root `AGENTS.md`.
- **Don't over-DRY.** Extract a helper only when it's a distinct concern, repeated 3+ times, or complex enough that naming it clarifies the flow (root `AGENTS.md`). `relayFlags.ts` is the canonical example — it was extracted because the 0/1↔bool conversion recurred across toggle UI and mutations.

A strong idiom: keep **pure decision logic** (payload shapes, validation ladders, discriminated-union decisions) in dedicated `lib` modules with no signals/effects/awaits (`relayPlanFlow.ts`, `loginInput.ts`), and keep the effect layer in the importing hook/component. These modules carry a header comment stating they are pure.

## The data layer: api.ts + createResource

Every backend call goes through `lib/api.ts`. A generic `callApi(method, path, body)` attaches a cached NIP-98 auth header, unwraps the `{ data, code }` envelope (returning `.data`), and throws `ApiError(message, status)` on a non-2xx response. Each endpoint is a thin exported wrapper with explicit generics; add new endpoints the same way (use an `undefined` request type for bodyless requests — both GETs and parameterless POSTs). The lone exception is `listPlans`, which bypasses `callApi` and does its own raw `fetch` with no auth machinery. (The public/unauthed routes are `/plans` and `/plans/:id` — the two backend handlers that omit the `AuthedPubkey` extractor every other route requires; `getPlan` still goes through `callApi`, which simply omits the `Authorization` header when logged out.)

Wire types are snake_case. Two modeling choices to respect: relay boolean settings are **numeric 0/1 flags**, not JS booleans — toggle them through the `relayFlags` helpers; and an invoice's lifecycle is **derived from nullable timestamps** via `invoiceStatus`, not read from a status column.

Screen reads expose a `use*` `createResource` hook in `hooks.ts` (again: not TanStack Query). Pass an accessor as the resource source when it should refetch on id change; pass a thunk reading `account()!.pubkey` for active-tenant reads. Render with `ResourceState` + `useMinLoading` + `Show`/`For`, defaulting the value with `?? []`.

Mutations are optimistic and follow one shape: `mutate(next)` → `await update` → `await refetch()`, and on error `mutate(previous)` + `setToastMessage(...)` to roll back and report.

The deeper mechanics — the `ApiOk` envelope and `ApiError` shape, NIP-98 auth caching, and the `api.ts`↔`state.ts` circular-import DI seam — are in [references/data-and-state-lifecycle.md](references/data-and-state-lifecycle.md).

## Global state, the session, and lazy tenant provisioning

`state.ts` holds app-lifetime singletons and global reactive state: the `account` signal, the `plans`/`identity` resources, the `billing*` resources, the toast signals, and `PLATFORM_NAME`.

The one ordering rule you must respect: **a tenant row is provisioned lazily.** `ensureSessionTenant()` does a `POST /tenants`, run by the account-activation subscriber on every account switch (it's idempotent — later runs just return the existing row). Some tenant-scoped reads 404 before the row exists: `getTenant` and `getDraftInvoice` 404 via `get_tenant_or_404`. (Note `listTenantInvoices` and `listTenantRelays` do *not* 404 — they query directly and return empty results — so the gate mainly protects the tenant/draft reads.) That is why the billing resources are gated on a separate `billingPubkey` signal rather than directly on `account()` — `billingPubkey` is cleared (set to `undefined`) on every account switch and re-set only once the tenant is ensured. Convention: gate any new tenant-scoped resource behind `billingPubkey` (or run `ensureSessionTenant()` first), and after a billing-affecting mutation call `refetchBilling()` where the flow needs it.

For the billing *domain* (why proration, dunning, churn, and reactivation behave as they do), read the [billing-model skill](../billing-model/SKILL.md) — that skill owns the intent. This skill only documents the frontend data-layer wiring. The detailed account-switch lifecycle is in [references/data-and-state-lifecycle.md](references/data-and-state-lifecycle.md).

## Nostr integration (applesauce)

Most agents touch this rarely. Three app-lifetime applesauce singletons live in `state.ts`: `eventStore`, `pool` (a `RelayPool`), and `accountManager`. The reactive `account` signal mirrors `accountManager.active$` — never set `account` directly; activate accounts through `accountManager`.

To reach the singletons: inside a SolidJS reactive scope use `const nostr = useNostr()` (the DI seam in `nostr.ts`, which falls back to the module singletons when no provider is mounted); outside one (event handlers, plain async, `api.ts`) import the module singletons directly.

Render profiles through the `useProfileMetadata` / `useProfileMetadataMap` / `useProfilePicture` hooks, which subscribe to `eventStore.profile(...)` and optionally prime over the network — never hand-roll relay queries. When a parent batch-primes a list, children subscribe with `{ prime: false }` so they don't each re-prime.

Pubkeys are hex strings; shorten them for display with the canonical `shortenPubkey` in `lib/pubkey.ts`. Gotcha: `AppShell.tsx` has its own *duplicate* `shortenPubkey` with a different format (8/6 split with a `…` character vs. the canonical 8/8 split with `...`) — **prefer the canonical `lib/pubkey.ts` one.** Auth is a session-style NIP-98 variant: a kind-27235 event signed by the active account, with a `u`-tag equal to `VITE_API_URL`, cached ~10 minutes.

## Routing and auth gating

All routes are declared centrally in `App.tsx` under one `Router` with `root={Layout}`. Protected pages are wrapped in `requireTenant()` (logged in) or `requireAdmin()` (`identity().is_admin`), both built on a `requireCondition` HOC that redirects to `/` when the check fails and waits on the `identity` resource to finish loading. A new gated page must be wired into `App.tsx` with the right wrapper. `AppShell` wraps the authenticated paths (matched by a path regex in `Layout`).

The gate is **rendering-only** (it just conditionally renders the page and client-redirects). Real security is server-side in two distinct layers: (1) **authentication** — the backend decodes the NIP-98 `Authorization` header, verifies the kind-27235 event's signature, and checks the `u`-tag equals `SERVER_URL` for host affinity, recovering the signer pubkey; (2) **authorization** — each handler then calls `require_admin` / `require_tenant` / `require_admin_or_tenant` to compare that pubkey against the configured admin pubkeys or the tenant's pubkey. The `u`-tag/`server_url` check is authentication/host-binding, not the authorization decision. And note: the README's `/login` route does not exist — login is a modal/embedded component, not a route.

## Styling: Tailwind v4 + the brand remap + shared kit

Styling is inline Tailwind v4 utility strings in `class=` — no CSS modules, no `@apply`, no `tailwind.config.js` (config is CSS-first, in `src/index.css`, wired via the `@tailwindcss/vite` plugin).

**THE pitfall:** `index.css` remaps the entire `--color-blue-*` scale to a brown brand palette, so `bg-blue-600`, `text-blue-700`, and `focus:ring-blue-500` render **brown** (`#c18254`) and *are* the intended brand accent. Do not "fix" them to a brown hex, and do reach for blue utilities for any primary accent.

Compose from the shared kit rather than bespoke markup: `PageContainer` (page wrapper), `ResourceState`/`LoadingState` (paired with `createResource` + `useMinLoading`), `Modal` plus `ConfirmDialog`/`PaymentDialog`, and the form primitives `Field`/`ToggleField`/`Checkbox`/`SearchInput`/`ToggleButton`/`RelayForm`. Most transient/global errors and successes go through `setToastMessage(message, variant)` (a single bottom-right `Toast` in `components/Toast.tsx`, mounted once in `App.tsx`; default variant `"error"` = red, `"success"` = green) — prefer it over ad-hoc alert UI. It is not the only error surface, though: contextual/inline errors bypass the toast — e.g. Login's local `error()` signal (`views/Login.tsx`), the payment flow (`PaymentDialog` bolt11 error, `PaymentSetupShell`), resource-load failures (`ResourceState` errorText), relay provisioning/sync errors (`RelayCardHeader`), and the `PromptBanner` banner.

Preline's `HSStaticMethods.autoInit()` is re-run on pathname changes by a `createEffect` in `Layout` (`App.tsx`). However, this codebase does not actually use any `data-hs-*` markup — all dropdowns, tabs, and overlays are hand-rolled with SolidJS signals and manual listeners (see `RelayCardHeader.tsx`, `LoginTabsScreen.tsx`, `PaymentSetup.tsx`). Don't assume `data-hs-*` needs no wiring: the effect fires only on full-path changes (not same-path query/hash navigation), and because SolidJS renders DOM lazily via `<Show>`, any Preline markup that mounts after the effect last ran will be uninitialized. If you do use Preline `data-hs-*` components, wire initialization explicitly rather than relying on this route-level effect.

## Building and verifying a change

There is no lint or test step for the frontend. The verification gate is the **build**, which runs a strict typecheck (`tsc -b`, project references, `noEmit`) then a production bundle (`vite build`). Verify with `just build-frontend` (which runs `bun i && bun run build`) or, inside `frontend/`, `bun run build`. **Bun is the package manager** (`bun.lock`; no npm/yarn/pnpm lockfile) — ignore the README's `npm` instructions. `just build` also compiles the Rust backend, so prefer `build-frontend` to verify only frontend changes.

Three `VITE_*` vars (`VITE_API_URL`, `VITE_RELAY_DOMAIN` — the domain this client creates relays under, sent as each new relay's `zooid_domain` — and optional `VITE_PLATFORM_NAME`) are read once into named lib constants (`API_URL`, `RELAY_DOMAIN`, `PLATFORM_NAME`) and baked at build time. A fourth, `VITE_PORT`, is read only by `vite.config.ts` and is undocumented (absent from both `.env.template` and `global.d.ts`). Adding a new client env var means: prefix it `VITE_`, declare it in `src/global.d.ts`, add it to `.env.template`, and (for Docker) wire the placeholder + entrypoint `sed` substitution. The full env/Docker substitution matrix is in [references/build-env-and-docker.md](references/build-env-and-docker.md).
