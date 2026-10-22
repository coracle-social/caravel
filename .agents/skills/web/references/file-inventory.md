# File inventory and route wiring

A map of `web/src`, for finding the existing module that owns something instead of writing another.

## Entry points

Each app has an HTML entry at the root of `web/` and a matching directory under `src`:

- `tenant.html` → `src/tenant/index.tsx` — imports `shared/index.css`, titles the page after the platform, and renders the tenant `App`.
- `admin.html` → `src/admin/index.tsx` — the same, for the admin `App`.
- `portal.html` → `src/portal/index.tsx` — imports `portal/index.css` and renders the portal `App`, titled after its relay.
- `src/global.d.ts` — ambient declarations: the `VITE_*` variables and `window.nostr`.

## `tenant/` — the tenant dashboard

- `App.tsx` — the route table (below).
- `pages/`: `Home` (the landing page, which also creates a first relay and signs in), `Account` (billing, payment methods, payouts and payment history), and `relays/` (`RelayList`, `RelayNew`, `RelayDetail`).
- `components/`: `Layout` (the frame, and the `requireTenant` gate), `TenantShell` (the signed-in navigation: the sidebar, with a link to the admin dashboard for admins, the phone's bottom bar with relay search, and the billing banner), and `PayoutsSection` (the tenant's Stripe Connect payout account: its status, and links to onboarding or Stripe's payouts dashboard).

## `admin/` — the admin dashboard

- `App.tsx` — the route table (below).
- `pages/`: `AdminTenantList`, `AdminTenantDetail` (with deleting and banning), `AdminRelayList`, `AdminRelayDetail`, `AdminInvoiceList`, `AdminInvoiceDetail`, `AdminModeration`.
  - `AdminModeration` accumulates pages of the cross-relay feed of events and uploads in its own signals so "load more" can append, and filters what has loaded. It reads a time window whose start is fixed when the range is picked, pages back inside it by `seen_at` (when the relay received an item), and drops responses for a window it has left.
- `components/`: `Layout` (the frame, gating every page to admins with its own login), `AdminShell` (the top bar with the sections, and the signed-in admin's menu for signing out), `AdminInvoiceListItem`, `InvoiceDetailCard`, `ModerationListItem`, `NoteContent`.
  - `NoteContent` is the only place the dashboards show text strangers wrote. It builds the page from `@welshman/content`'s parsed segments, never from HTML, links only `http` and `https`, shows custom emoji as their shortcode, and truncates long notes.
  - `ModerationListItem` previews an upload by fetching its bytes through the API, since relays serve blobs only to their members, and fetches only what scrolls into view.

## `portal/` — the member portal

- `App.tsx` — the route table (below), under `/portal` under `vite dev`.
- `pages/`: `RelayPage` (the relay's own page: its offer, sign-in, checkout, renewal and cancellation, or for a relay that doesn't sell memberships, what it is and a link to Flotilla), `JoinPage` (buying an invite without a Nostr account), `InvitePage` (an invite after payment, with the link that accepts it), `AccountPage` (every membership the member holds), `NotFound`.
- `components/`: `Page`, `Card`, `Notice`, `RelayHeader`, `SignIn` (an extension or a remote signer signs one request for a session).
- `lib/`: `api.ts` (the member routes, and the offer, membership view, member account and invite types), `session.ts` (the member's session token), `nostr.ts` (the remote signer's relays and the short npub), `relay.ts` (the relay the portal is served for), `params.ts` (query parameters and the client to return to).

## `shared/` — what the apps share

- `index.css` — the dashboards' Tailwind v4 configuration and the brand color remap.
- `views/`: `Login` (the dashboards' login, a dialog or a whole page), `RelayEdit` (editing a relay, under the tenant's or the admin's relay pages).
- `components/`:
  - kit: `PageContainer`, `ResourceState`, `LoadingState`, `Modal`, `ModalHeader`, `ConfirmDialog`, `SegmentedTabs`, `StatusBadge`, `Avatar`, `Field`, `ToggleField`, `ToggleButton`, `SearchInput`, `Toast`, `BackLink`, `ActionsMenu`, `CopyNpub`, and small icons.
  - relays: `RelayForm`, `RelayListItem`, `RelayDetailCard` (a relay's settings as switch lists, its custom domain, members, member payments and plan, and the confirmations for deactivating, reactivating, erasing and deleting, which the API calls purging), `ActivityFeed`, `PricingTable`, and in `relay/`: `RelayCardHeader`, `PlanGatedToggle`, `CustomDomainModal`, `DataTransferModal` (exporting and importing events as JSONL), `MemberPayments` (the member price, the portal link and who is paying), `MemberPriceModal`.
  - billing: `BillingPrompts` (the banner and its dialogs, and the `?invoice=` link), `PromptBanner`, `PostPaidPrompts`, `PaymentDialog` (paying an invoice by Lightning or Stripe Checkout), `PaymentSetup`, `PaymentSetupCard`, `PaymentSetupNWC` and their shared `PaymentSetupShell`, and in `payment/` and `account/`: `LightningPayBody`, `InvoiceItemsList`, `InvoiceListItem`, `PaymentMethodRow`.
  - `login/`: `LoginTabsScreen`, `LoginSignerScreen`, `LoginKeyScreen`, `QrScannerOverlay`.
- `lib/`:
  - core: `config`, `http`, `api`, `account`, `state`.
  - concepts: `profiles` (profiles and their names), `relays` (relay resources and creating a relay), `billing` (prompts and status), `paymentMethod`, `plans` (plan ids and labels), `pubkey`, `format`, `resource` (`loaded`).
  - hooks: `useRelayActions`, `useRedirect`, `useNwcSetup`, `useCustomDomain`, `useInvoicePdf`, `useMinLoading`.
  - helpers: `loginInput`, `subdomain`, `slugify`, `search`, `clipboard`.

## Routes

The tenant dashboard (`tenant/App.tsx`):

- `/` → `Home`, ungated.
- `/relays`, `/relays/new`, `/relays/:id`, `/relays/:id/edit`, `/account` → gated by `requireTenant`.

The admin dashboard (`admin/App.tsx`):

- `/admin` → redirects to `/admin/tenants`.
- `/admin/tenants`, `/admin/tenants/:id`, `/admin/relays`, `/admin/relays/:id`, `/admin/relays/:id/edit`, `/admin/invoices`, `/admin/invoices/:id`, `/admin/moderation` → gated by the admin `Layout`.

Each gate waits for the identity resource to load. `requireTenant` then renders the page or sends the visitor to `/`, and the tenant `Layout` frames the relay and account pages in `TenantShell`. The admin `Layout` shows the login, a notice for a non-admin, or the page in `AdminShell`. There is no login route.

The portal (`portal/App.tsx`), at the root of its relay's host: `/` → `RelayPage`, `/join` → `JoinPage`, `/invites/:code` → `InvitePage`, `/account` → `AccountPage`, and anything else → `NotFound`.
