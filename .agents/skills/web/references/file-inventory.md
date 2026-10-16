# File inventory and route wiring

A concrete map of `web/src`, so the SKILL.md "where things live" section can stay prose-only. Use this when you need to find the right existing module instead of guessing.

## Entry points

Each app has an HTML entry at the root of `web/` and a matching directory under `src`:

- `tenant.html` → `src/tenant/index.tsx` — imports `shared/index.css`, dynamically imports Preline, then renders the tenant `App`.
- `admin.html` → `src/admin/index.tsx` — the same, for the admin `App`.
- `portal.html` → `src/portal/index.tsx` — imports `portal/index.css` and renders the portal `App`, titled after its relay.
- `src/global.d.ts` — ambient declarations (`ImportMetaEnv`, `Window.HSStaticMethods`, `Window.nostr`).

## `tenant/` — the tenant dashboard

- `App.tsx` — the route table (below).
- `pages/`: `Home.tsx`, `Account.tsx`, and `relays/` (`RelayList`, `RelayNew`, `RelayDetail`, `RelayEdit`).
- `components/PayoutsSection.tsx` — the account page's Stripe Connect payout account: its status, and links to onboarding or the Stripe payouts dashboard.

## `admin/` — the admin dashboard

- `App.tsx` — the route table (below).
- `pages/`: `AdminTenantList`, `AdminTenantDetail`, `AdminRelayList`, `AdminRelayDetail`, `AdminRelayEdit`, `AdminInvoiceList`, `AdminInvoiceDetail`, `AdminModeration`.
  - `AdminModeration` is the one list screen that doesn't wrap a `createResource` hook from `hooks.ts`: it accumulates pages of the cross-relay content feed in its own signals so "load more" can append, and its search/type filters therefore run over what has been loaded rather than over the whole window. It reads a bounded time window (the range chips) whose `since` is frozen when the range is picked, then pages backwards inside it by lowering `until` to the last item's `seen_at` (when the relay received it, which is what the feed orders on and what each row displays, rather than the event's `created_at`); because switching ranges restarts the feed mid-flight, it carries a `generation` counter and discards responses belonging to a superseded window.
- `components/`: `AdminInvoiceListItem`, `InvoiceDetailCard`, `ModerationListItem`, `NoteContent`.
  - `NoteContent` renders nostr note content — the only place the dashboards display text authored by strangers. It parses with `@welshman/content` and builds JSX from the parsed segments rather than using that package's own `renderAsHtml`, so hostile content never reaches `innerHTML`; it only linkifies `http:`/`https:` and renders custom emoji as their shortcode rather than fetching an image from a poster-controlled host. It `truncate()`s as well as CSS line-clamping, because the clamp only hides overflow while a feed page's DOM cost is real.
  - `ModerationListItem`'s upload preview is the only place the dashboards render an image they can't just point an `<img>` at: blobs are read through the API for authorization, and `<img src>` can't carry the session header, so it fetches the bytes and renders an object URL. That trades away native `loading="lazy"`, which it wins back with an `IntersectionObserver`; the object URL is revoked on cleanup, including when the row unmounts while the fetch is still in flight.

## `portal/` — the member portal

- `App.tsx` — the route table (below), under `/portal` under `vite dev`.
- `pages/`: `RelayPage` (the relay's own page: its membership offer, sign-in, checkout and renewal, or for a relay that doesn't sell memberships, what it is and a link to Flotilla), `JoinPage` (buying an invite), `InvitePage` (an invite after payment, with the link that accepts it), `AccountPage` (every membership the member holds, linking to each relay's own page), `NotFound`.
- `components/`: `Page`, `Card`, `Notice`, `RelayHeader`, `SignIn` (a browser extension or a remote signer signs one NIP-98 request for a session).
- `lib/`: `api.ts` (the member routes and their wire types), `session.ts` (the session token, kept in `localStorage`), `nostr.ts` (signing in), `relay.ts` (the relay the portal is served for), `params.ts` (query parameters and the client to return to), `format.ts`.

## `shared/` — what the dashboards share

- `index.css` — Tailwind v4 CSS-first config and the brand color remap.
- `views/Login.tsx` — the login flow, the only thing under `views/`.
- `components/`:
  - `Layout` — the dashboards' frame (`AppShell` once signed in, the toast) and the `requireTenant`/`requireAdmin` guards; `AppShell` — the signed-in navigation; `AppLink` — a link that loads the page when it crosses between the dashboards.
  - shared kit: `PageContainer`, `ResourceState`, `LoadingState`, `Modal`, `ConfirmDialog`, `PaymentDialog`, `Field`, `ToggleField`, `Checkbox`, `SearchInput`, `ToggleButton`, `RelayForm`, `Toast`, `PromptBanner`, `PricingTable`, `BackLink`, plus small icon components.
  - feature subfolders: `login/` (`LoginTabsScreen`, `LoginKeyScreen`, `LoginSignerScreen`, `QrScannerOverlay`), `payment/` (`InvoiceItemsList`, `LightningPayBody`), `account/` (`InvoiceListItem`, `PaymentMethodRow`), `relay/` (`PlanGatedToggle`, `RelayCardHeader`, `CustomDomainModal`, `DataTransferModal`, `MemberPayments`, `MemberPriceModal`).
  - cards at the top level: `BillingPrompts`, `RelayDetailCard`, `RelayListItem`, `PaymentSetup*`, `ActivityFeed`.
  - `DataTransferModal` exports and imports a relay's events as JSONL, and calls the API outside `callApi` because neither side of either request is JSON and an `<a href>` can't carry the session header: the export is fetched as a blob and clicked through a detached anchor. It reports what an import refused rather than throwing — a dump with a tampered line is a partial success, not a failure.
  - `MemberPayments`, on a relay's detail card, shows its member price, the link to its portal, and who is paying; `MemberPriceModal` sets the price, once the owner's payouts are ready.
- `lib/`:
  - **pillars:** `config.ts` (runtime configuration), `api.ts` (wire types + fetch wrappers), `state.ts` (singletons + global signals/resources), `hooks.ts` (`use*` read hooks + active-tenant action helpers), `nostr.ts` (`useNostr` DI seam).
  - **`use*` hooks:** `useMinLoading`, `useRelayToggles`, `usePaymentSetup`, `useInvoicePdf`, `usePayoutRedirect`.
  - **pure helpers:** `relayPlanFlow`, `relayFlags`, `paymentMethod`, `loginInput`, `subdomain`, `slugify`, `search`, `format`, `clipboard`, `validation`, `pubkey`, `billing`.

## Routes

The tenant dashboard (`tenant/App.tsx`), under one `Router root={Layout}`:

- `/` → `Home` (ungated)
- `/relays`, `/relays/new`, `/relays/:id`, `/relays/:id/edit` → `requireTenant(...)`
- `/account` → `requireTenant(Account)`

The admin dashboard (`admin/App.tsx`), under one `Router root={Layout}`:

- `/admin` → redirects to `/admin/tenants`
- `/admin/tenants`, `/admin/tenants/:id`, `/admin/relays`, `/admin/relays/:id`, `/admin/relays/:id/edit`, `/admin/invoices`, `/admin/invoices/:id`, `/admin/moderation` → `requireAdmin(...)`

`requireTenant(Page)` and `requireAdmin(Page)` are both built on a `requireCondition(Page, condition)` HOC: it waits for the `identity` resource to finish loading, sends a failed check to the tenant dashboard's `/` (a page load from the admin dashboard), and otherwise renders the page inside a `<Show>`. `Layout` wraps the page in `AppShell` when the path matches `/^\/(relays|account|admin)/` and identity has loaded.

There is **no `/login` route** — login is rendered as a modal/embedded component (`shared/views/Login.tsx`, mounted as a `<Modal>` in `tenant/pages/Home.tsx`).

The portal (`portal/App.tsx`), at the root of its relay's host: `/` → `RelayPage`, `/join` → `JoinPage`, `/invites/:code` → `InvitePage`, `/account` → `AccountPage`, anything else → `NotFound`.
