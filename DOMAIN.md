# Domain

Caravel's domain language. The code is organized around this vocabulary first; imports, the call graph and the crate layout follow it.

## Actors

A **tenant** is a person or organization, identified by a nostr pubkey. A tenant has a single billing cycle, to which usage, invoices and payouts accrue.

An **admin** is someone authorized to manage the entire list of tenants.

A **member** is a member of a tenant's relay.

## Layers

**Interfaces** are the frontends that certain types of users use to access the app.

**Services** are the entry points into the application. They are a thin layer concerned with routing, scheduling and resource management.

**The core** sits between services and domain. It is an adapter layer that translates business logic from service terms to domain terms.

**Domain modules** are well-demarcated areas of responsibility. A module provides the models, utilities, queries and commands that cover its area. Modules are directories; a crate groups related modules.

**Database, config and other utilities** sit at the bottom.

## Interfaces

- **Admin app** — the web frontend for admins, in `web/src/admin`.
- **Tenant app** — the web frontend for tenants, in `web/src/tenant`.
- **Portal** — the web frontend for members, in `web/src/portal`.

## Services

- **API** (`crates/services/api`) — serves tenant, admin and member use cases.
- **Worker** (`crates/services/worker`) — operates in the background to manage async tasks.
- **Relay** (`crates/services/relay`) — an instance of a relay, actively managing websocket traffic.

## Domain modules

### Billing

Billing (`crates/domain/billing`) is concerned with tenants and relays as billable entities, and with the services being billed for: relays and their settings, memberships, and so on.

- **Tenants** — invoices, plans, usage, payouts.
- **Relays** — relay settings, relay activity, member billing, custom domains.

### Relays

Relays (`crates/domain/relays`) is the relay itself. It knows nothing about the layers above it; its concern is correctly implementing a nostr relay that fits our use case.

- **Profiles** — NIP-11 documents.
- **Access** — bans, members, roles, claims.
- **Management** — NIP-86 methods.
- **Rooms** — room-level bans, membership, metadata.
- **Media** — blossom file storage.
- **Events** — nostr event storage.
- **Calls** — LiveKit calls.
- **Transfer** — imports and exports.
- **Policy** — the complex read/write controls, kept in a module of their own.
- **Notify** — messages the relay sends members over their connections.

### Operations

Operations (`crates/domain/operations`) is what the platform's operators need to run it.

- **Moderation** — the feed of events and uploads moderated relays saw, for admins to review.
- **Sessions** — web sessions for tenants and admins.

## Dependencies

The domain crates depend only on the bottom layer, and billing and operations may depend on relays. The core depends on the domain; services depend on the core; the app wires it all together.
