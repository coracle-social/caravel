# File inventory and route wiring

A concrete map of `frontend/src`, so the SKILL.md "where things live" section can stay prose-only. Use this when you need to find the right existing module instead of guessing.

## Entry points

- `index.tsx` — bootstrap. Imports `index.css`, dynamically imports Preline, then `render`s `<App />`.
- `App.tsx` — `Router` + central route table + the auth-gate HOCs (see "Routes" below).
- `index.css` — Tailwind v4 CSS-first config and the brand color remap.
- `global.d.ts` — ambient declarations (`ImportMetaEnv`, `Window.HSStaticMethods`, `Window.nostr`).

## `pages/` — route components

- top level: `Home.tsx`, `Account.tsx`
- `relays/` (tenant relay screens): `RelayList`, `RelayNew`, `RelayDetail`, `RelayEdit`
- `admin/` (admin console): `AdminTenantList`, `AdminTenantDetail`, `AdminRelayList`, `AdminRelayDetail`, `AdminRelayEdit`, `AdminInvoiceList`, `AdminInvoiceDetail`, `AdminModeration`
  - `AdminModeration` is the one list screen that doesn't wrap a `createResource` hook from `hooks.ts`: it accumulates pages of the cross-relay content feed in its own signals so "load more" can append, and its search/type filters therefore run over what has been loaded rather than over the whole window. It reads a bounded time window (the range chips) whose `since` is frozen when the range is picked, then pages backwards inside it by lowering `until` to the last item's `seen_at` (when the relay received it, which is what the feed orders on and what each row displays, rather than the event's `created_at`); because switching ranges restarts the feed mid-flight, it carries a `generation` counter and discards responses belonging to a superseded window.

## `views/` — the lone exception

- `Login.tsx` — the login flow, the only thing under `views/` (not `pages/`).

## `components/` — shared UI

- shared kit: `PageContainer`, `ResourceState`, `LoadingState`, `Modal`, `ConfirmDialog`, `PaymentDialog`, `Field`, `ToggleField`, `Checkbox`, `SearchInput`, `ToggleButton`, `RelayForm`, `Toast`, `PromptBanner`, `PricingTable`, `BackLink`, plus small icon components
- `AppShell` — the authenticated layout shell (note: carries a duplicate local `shortenPubkey`; prefer `lib/pubkey.ts`)
- feature subfolders:
  - `login/` — `LoginTabsScreen`, `LoginKeyScreen`, `LoginSignerScreen`, `QrScannerOverlay`
  - `payment/` — `InvoiceItemsList`, `LightningPayBody`
  - `account/` — `InvoiceListItem`, `PaymentMethodRow`
  - `relay/` — `PlanGatedToggle`, `RelayCardHeader`, `CustomDomainModal`, `DataTransferModal`
  - plus billing/payment/relay cards at the components top level (`BillingPrompts`, `InvoiceDetailCard`, `RelayDetailCard`, `RelayListItem`, `PaymentSetup*`, `ActivityFeed`, `AdminInvoiceListItem`, `ModerationListItem`)
  - `NoteContent` renders nostr note content — the only place the app displays text authored by strangers. It parses with `@welshman/content` and builds JSX from the parsed segments rather than using that package's own `renderAsHtml`, specifically so hostile content never reaches `innerHTML`; it also only linkifies `http:`/`https:` and renders custom emoji as their shortcode rather than fetching an image from a poster-controlled host. It `truncate()`s as well as CSS line-clamping, because the clamp only hides overflow while a feed page's DOM cost is real.
  - `DataTransferModal` exports and imports a relay's events as JSONL, and is the second place the API is called outside `callApi` for the same reason `ModerationListItem` is: neither side of either request is JSON, and an `<a href>` can't carry the NIP-98 header, so the export is fetched as a blob and clicked through a detached anchor. It reports what an import refused rather than throwing — a dump with a tampered line is a partial success, not a failure.
  - `ModerationListItem`'s upload preview is the only place the app renders an image it can't just point an `<img>` at: blobs are proxied through the API for authorization, and `<img src>` can't carry the NIP-98 header, so it fetches the bytes and renders an object URL. That trades away native `loading="lazy"`, which it wins back with an `IntersectionObserver`; the object URL is revoked on cleanup, including when the row unmounts while the fetch is still in flight.

## `lib/` — non-UI logic

- **pillars:** `api.ts` (wire types + fetch wrappers), `state.ts` (singletons + global signals/resources), `hooks.ts` (`use*` read hooks + active-tenant action helpers), `nostr.ts` (`useNostr` DI seam)
- **`use*` hooks:** `useMinLoading`, `useRelayToggles`, `usePaymentSetup`, `useInvoicePdf`
- **pure helpers:** `relayPlanFlow`, `relayFlags`, `paymentMethod`, `loginInput`, `subdomain`, `slugify`, `search`, `format`, `clipboard`, `validation`, `pubkey`, `billing`

## Routes (`App.tsx`)

All declared under one `Router root={Layout}`:

- `/` → `Home` (ungated)
- `/relays`, `/relays/new`, `/relays/:id`, `/relays/:id/edit` → `requireTenant(...)`
- `/account` → `requireTenant(Account)`
- `/admin/tenants`, `/admin/tenants/:id`, `/admin/relays`, `/admin/relays/:id`, `/admin/relays/:id/edit`, `/admin/invoices`, `/admin/invoices/:id` → `requireAdmin(...)`

`requireTenant(Page)` and `requireAdmin(Page)` are both built on a `requireCondition(Page, condition)` HOC: it waits for the `identity` resource to finish loading, redirects to `/` when the condition is false, and otherwise renders the page inside a `<Show>`. `requireTenant` checks `Boolean(identity())`; `requireAdmin` checks `Boolean(identity()?.is_admin)`. `Layout` wraps the page in `AppShell` when the path matches `/^\/(relays|account|admin)/` and identity has loaded.

There is **no `/login` route** — login is rendered as a modal/embedded component (`views/Login.tsx`, mounted as a `<Modal>` in `pages/Home.tsx`), contrary to `frontend/README.md` (not the repo-root README), which lists a `/login` route.

## Sources

- src tree — verified live listing of `frontend/src`
- route table + guards — `frontend/src/App.tsx:43-81`
- entry bootstrap — `frontend/src/index.tsx:3-13`
- `AppShell` duplicate `shortenPubkey` — `frontend/src/components/AppShell.tsx:11-14`
