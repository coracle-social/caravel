---
name: architecture
description: How Caravel's Rust workspace is put together — the layered crates (db, adapters, core, relay, api, app) and what belongs in each, the layers inside the core (models, domain, queries, commands, subsystems) and where new code goes, the Core handle every operation runs against, how persistence and transactions work on Postgres, the change bus, session-token authentication and load-then-authorize, background workers, configuration, and how the tests run. Use it whenever adding or changing backend code — a route, a query, a model, a migration, a worker, an integration or a config variable — to know where the code goes and which conventions it follows.
---

# Architecture

Caravel's backend is one binary built from a Cargo workspace. The crates form layers, and each depends only on the layers below it. A test reads `cargo metadata` and fails if a crate reaches up or sideways, or if a third-party protocol client escapes the layer that owns it.

## The layers

**db** owns Postgres: connecting a pool, running the migrations, and running a transaction. A transaction that Postgres aborts over a conflict with another one (a deadlock or serialization failure) runs again from the start, so the work inside one must touch only the database. Pre-release, the schema is a single squashed migration.

**adapters** are clients for everything outside the database: Stripe, a client for other people's Nostr relays, Nostr Wallet Connect wallets and the robot's Nostr identity (profile publishing and NIP-17 DMs) on top of it, DNS over HTTPS, the Bitcoin price, blob storage (a directory, S3, or memory in tests), LiveKit (participant tokens, its room service, and checking its webhooks), and an outbound HTTP client for urls other people chose. It and the Nostr client refuse to reach local or internal addresses however they are named. Each speaks one protocol and returns plain values, and anything they sign takes the time from the caller. Nothing in an adapter knows what a tenant or a relay is, and only adapters may make HTTP requests or open websockets to other servers. Nostr types, encryption and event kinds come from coracle-lib and coracle-kinds throughout.

**core** is the domain, in layers of its own, described below. The core uses coracle-lib for Nostr types and cryptography.

**relay** serves the virtual relays: the websocket protocol, each relay's information document and NIP-86 management, Blossom media, push delivery, LiveKit tokens and webhooks, and resolving a host to its relay. The `relay` skill covers it.

**api** is the JSON API. Handlers load what the request names through the core, check the caller against it with the core's rules, and call the core. Routes are grouped by who may call them: public, member, tenant, and admin. The API reference is the crate's README.

**app** builds the `caravel` binary. It reads and checks the environment, connects two pools (one for requests, one for background work, so workers can't starve requests), builds the core, schedules the background workers, and serves each request by its host. A relay's hosts get that relay's protocols, and its member portal for a browser asking for a page; a page on any of its hosts but the primary one redirects there. The API host gets the API, and the platform host the dashboards (the admin one under `/admin`), and the API too when it shares that host under a path. Any other host gets nothing but the health check and on-demand TLS's ask. The web apps are embedded in the binary, and the app writes each one's runtime configuration into its page; the `web` skill covers them. A routing test checks every kind of host against every kind of request. On shutdown it closes every relay connection with code 1001 and gives workers ten seconds to stop.

## Inside the core

The core is layered like the workspace, and a test reads its source to keep it that way. Each layer uses only the ones before it:

- **Foundations**: errors, configuration, the platform's and relays' keys, and the serde helpers. The clock and the change bus are foundations too, but only subsystems use them.
- **Models** are the data: the types every layer above reads and writes, documented where they are defined, with methods that read only their own fields.
- **Domain** is the rules, as pure functions over models: the relay policy, which events belong to rooms and who may moderate them, validating a relay's settings, pricing a relay's activity, decoding NIP-98 headers and NIP-86 calls, and the tags of the events a relay publishes about itself. Nothing in it is async, reads the database or the clock, or makes up ids.
- **Queries** read. Each takes a database executor and returns models, so it runs on the pool or inside a caller's transaction, and none writes.
- **Commands** write. Each makes one change on a caller's connection, at a time the caller gives, including the events the relay signs to publish it, which it returns for the caller to deliver once the transaction commits. Commands read through queries, and never open a transaction, read the clock, publish on the bus or call an adapter.
- **Subsystems** run those against a `Core`: they read the clock, open transactions, call adapters and publish changes on the bus. Hosting runs tenants and relays; management runs a relay's members, NIP-86 calls and room moderation from the dashboards; ingest acts on what clients publish to a relay; and the others are memberships, billing, usage, calls, media, notifications, sessions, payouts, custom domains, transfer, and the event store's sweeps. Subsystems may use each other, but not in a cycle.

The four layers are split by the same concerns (relays, tenants, members, roles, bans, claims, method grants, rooms, events, the ledger, memberships and so on), so a concern's types, rules, reads and writes sit under the same name in each. A new type goes in models, a decision in the domain, a read in queries, a write in commands, and an operation a route or worker performs in a subsystem.

Above the core, the relay, API and app crates read through queries and change state only through subsystems. A workspace test checks that they never call a command or open a transaction.

## The core handle

Every core operation takes a `Core`, which carries the database pool, the configuration, the clock, the change bus and the adapters. Functions that only touch the database take a database executor instead, so the same function works on the pool or inside a caller's transaction. Functions that stamp a time take it as an argument; whatever orchestrates them reads the clock once.

The clock is the only source of the current time, and tests pin it. Configuration comes only from the app's config module. Clippy forbids reading either anywhere else.

## Writing to the database

A write that must be atomic runs in one transaction. Billing relies on row locks rather than on a single writer: every billing write that must happen once per tenant and period (renewal, overage, cutting an invoice, churn) first locks the tenant's row and re-checks its marker under the lock. Claims such as marking an activity billed are conditional updates, so a second pass does nothing. The concurrency tests race billing passes against one tenant to keep this true.

A unique constraint a caller can violate (a taken subdomain) is named in the migration and translated by name into a validation error. A custom domain is unique only among verified ones, so a relay that merely claims a domain can't lock its owner out: setting one another relay has verified is refused up front, and verification never takes one from the relay that holds it.

## The change bus

After a write commits, the subsystem that made it publishes a change on an in-process bus: a relay's row changed, its access or rooms changed along with the events it published about them, or a pubkey's notification on a relay was replaced. Subscribers treat a change as a hint to re-read the database; one that falls behind re-reads everything it watches. The relay runtime is the bus's subscriber, reloading relays and delivering what they published. The runtime's own writes go through ingest, and its NIP-86 calls through a management entry point of their own; both return what the relay published instead, since the runtime applies the change itself before answering the client.

## Authentication and authorization

The API's own routes authenticate with session tokens. A client signs one NIP-98 request for exactly `POST /session` and gets back an opaque bearer token, good for thirty days, which it sends from then on. Only a hash of the token is stored, so sessions can be ended individually or all at once for a pubkey.

An authenticated request runs as an actor: a pubkey, and whether it is an admin. Authorization is load-then-authorize: a handler loads the tenant, relay or invoice the request names, then asks the actor whether it may act as that resource's tenant. An extractor gates the admin-only routes. A test calls every route without credentials, and admin routes as a tenant, to make sure none is left open.

## Background work

Workers are scheduled in the app and implemented in the core: hourly billing (which first samples each paid relay's stored media and call clock), custom domain verification every thirty seconds and re-verification every three hours, a daily sweep of expired sessions, a sweep of expired events every minute, a membership sweep every five minutes, erasing purged relays' events, access lists, rooms and media every ten minutes, and publishing the robot's identity, retried until it succeeds. Each runs on the background pool and stops on shutdown. Push deliveries run on a pool of their own in the relay runtime.

## Tests

Tests run against Postgres: each test binary creates its own database from `TEST_DATABASE_URL` on first use (`just pg` provides one), and each test gets its own core and pinned clock, so tests in a binary run in parallel on separate tenants. Adapters in tests point at closed ports or keep blobs in memory, so nothing leaves the machine; tests that need LiveKit or a push callback serve a fake on a local port. The billing tests replay months by moving the clock; the API tests drive the router in process and pin the wire shapes clients depend on.
