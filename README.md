# Nostr Community Relay Hosting Platform

A multi-tenant platform for hosting Nostr community relays, built on top of [zooid](./ref/zooid) — a multi-tenant relay implementation configured via TOML and an HTTP API.

---

## Overview

The platform has three main components:

1. **Backend** — Manages the zooid instance via its HTTP API, along with tenants (users), virtual relays, invoices, and billing.
2. **Tenant Dashboard** — A web app where anyone can sign up, pay for a relay via Lightning, and manage their relay's membership, metadata, access, and policies.
3. **Super Admin Dashboard** — A restricted interface for the infrastructure owner to manage tenants and their relays.

A proof of concept exists in [`ref/metamanager`](./ref/metamanager) (backend) and [`ref/website`](./ref/website) (frontend). Zooid is included for reference in ./ref/zooid.

---

## Tech Stack

**Frontend**
- Bun
- TypeScript
- SolidJS
- shadcn/ui

**Backend**
- Rust
- Sqlite

---

## Database Schema

### `tenants`
Users of the platform. Identified by their Nostr public key.

| Field | Description |
|---|---|
| `pubkey` | Primary key — Nostr public key |
| `status` | Account status (active, suspended, etc.) |

### `relays`
Virtual relays linked to a tenant.

| Field | Description |
|---|---|
| `id` | Primary key |
| `tenant` | Foreign key → tenants |
| `name` | Human-readable relay name |
| `subdomain` | Subdomain slug (e.g. `my-relay` → `my-relay.spaces.coracle.social`) |
| `schema` | zooid DB schema name (subdomain with dashes replaced by underscores; stored for reference only) |
| `description` | Relay description |
| `plan` | Pricing tier: `free`, `basic`, or `growth` |
| `status` | Relay status (pending, active, suspended, etc.) |

### `invoices`
Invoices generated for tenants, potentially covering multiple relays in a single payment.

| Field | Description |
|---|---|
| `id` | Primary key |
| `tenant` | Foreign key → tenants |
| `amount` | Total invoice amount (sats) |
| `status` | Payment status (pending, paid, expired, etc.) |
| `created_at` | Timestamp |
| `invoice` | Lightning invoice string |

### `invoice_items`
Line items linking an invoice to a specific relay charge.

| Field | Description |
|---|---|
| `id` | Primary key |
| `invoice` | Foreign key → invoices |
| `relay` | Foreign key → relays |
| `amount` | Amount for this relay (sats) |
| `period_start` | Start of billing period |
| `period_end` | End of billing period |

---

## Pricing Tiers

| Tier | Price | Members | Blossom | LiveKit |
|---|---|---|---|---|
| **Free** | 0 sats/mo | Up to 10 | No | No |
| **Basic** | 10,000 sats/mo | Up to 100 | Yes | Yes |
| **Growth** | 50,000 sats/mo | Unlimited | Yes | Yes |

---

## Backend

> Do not modify zooid directly. All relay management goes through its HTTP API (`zooid/api.go`).

### Authentication

- All endpoints are authenticated using NIP 98

### Responsibilities
- Manage zooid virtual relays via the HTTP API
- Tenant and relay CRUD
- Lightning invoice generation and collection
- Billing: recurring charges, grace periods, NIP-17 DM notifications
- Async job runner for billing checks, invoice collection, and relay provisioning

### Relay Provisioning
When a relay is created, an async worker is spawned that sends the appropriate API request to zooid to set up the virtual relay.

### Billing Logic
- Billing is monthly. Invoices batch all of a tenant's relay charges into a single payment.
- Tenants can enable **recurring billing** by providing their own NWC URL on the account page. The platform uses this to pull payments automatically.
- If recurring billing is off, invoices are sent via **NIP-17 DMs** (from the platform's Nostr key) when a subscription is due. If recurring billing is on, still send notifications when a payment is made.
- A **7-day grace period** applies before access is restricted for non-payment.

### Environment Variables
| Variable | Description |
|---|---|
| `HOSTING_ADMIN_PUBKEYS` | Comma-separated Nostr pubkeys with super admin access |
| `PLATFORM_SECRET` | Nostr private key used by the platform to send NIP-17 DMs |
| `NWC_URL` | Nostr Wallet Connect URL used by the platform to generate Lightning invoices |

---

## Frontend

### Marketing Page (`/`)
- Describes the value proposition, features, and brand
- Prominent **"Get Started"** button linking to the new relay form
- A "Log In" button, replaced by a "Dashboard" button when the user is logged in


### Login Page (`/login`)
- Uses nonboard to log the user in

### Tenant dashboard
- All requests are authenticated using NIP 98 against the tenant pubkey

#### Relays Page (`/relays`)
- **My Relays** — searchable, filterable list of the user's relays
- **Add Relay** button (top right)
- Link to account page

#### Relay Detail Page (`/relays/{id}`)
- Shows relay information (name, url, description, plan)
- Actions: edit relay, deactivate

### New Relay Form (`/relays/new`)
- Requires Nostr login (via the `nonboard` plugin — see [`ref/website`](./ref/website)); the NIP-98 auth token is used to identify the tenant
- Fields:
  - Relay name
  - Subdomain (auto-generated from name, editable; displayed as `<subdomain>.spaces.coracle.social`)
  - Description
  - Plan selection — shown as a comparison table (see Pricing Tiers)
- On submit (if plan is not free): generates a Lightning invoice
  - User can scan QR code, click to open in wallet, or copy invoice string
  - **"I've paid the invoice"** button — checks invoice status and reports to user
  - On confirmed payment: redirect to tenant dashboard

### Account Page (`/account`)
- Account status
- Complete invoice history
- Toggle for recurring billing via Nostr Wallet Connect (tenant provides their own NWC URL)

### Super Admin Dashboard (`/admin`)
- Authenticated via NIP-98 against `HOSTING_ADMIN_PUBKEYS`
- No link to this page from any other page

#### Tenants Page (`/admin/tenants`)
- Searchable, filterable list of all tenants

#### Tenant Detail Page (`/admin/tenants/{id}`)
- All their relays
- Account status
- Actions: deactivate tenant and all their relays

#### Relays Page (`/admin/relays`)
- Searchable, filterable list of all relays
- Link to owning tenant
- Actions: update or deactivate any relay

#### Relay Detail Page (`/admin/relays/{id}`)
- Show relay information
- Actions: edit or deactivate relay

#### Relay Edit Form (`/admin/relays/{id}/edit`)
- Edit relay name, subdomain, description, and plan

### Relay Edit Form (`/relays/{id}/edit`)
- Edit relay name, subdomain, description, and plan
