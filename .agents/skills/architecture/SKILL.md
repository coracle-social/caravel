---
name: architecture
description: How Caravel's Rust workspace is put together — the layered crates (db, adapters, core, api, app) and what belongs in each, the Core handle every operation runs against, how persistence and transactions work on Postgres, the change bus, session-token authentication and load-then-authorize, background workers, configuration, and how the tests run. Use it whenever adding or changing backend code — a route, a query, a model, a migration, a worker, an integration or a config variable — to know where the code goes and which conventions it follows.
---

# Architecture

Caravel's backend is one binary built from a Cargo workspace. The crates form layers, and each depends only on the layers below it. A test reads `cargo metadata` and fails if a crate reaches up or sideways, or if a third-party protocol client escapes the layer that owns it.

## The layers

**db** owns Postgres: connecting a pool, running the migrations, and running a transaction. A transaction that Postgres aborts over a conflict with another one (a deadlock or serialization failure) runs again from the start, so the work inside one must touch only the database. Pre-release, the schema is a single squashed migration.

**adapters** are clients for everything outside the database: Stripe, Nostr Wallet Connect wallets, the robot's Nostr identity (profile publishing and NIP-17 DMs), DNS over HTTPS, the Bitcoin price, and zooid's management API. Each speaks one protocol and returns plain values. Nothing in an adapter knows what a tenant or a relay is, and only adapters may use rust-nostr or make HTTP requests.

**core** is the domain, partitioned by concern: plans, tenants, relays and their activity log, billing (the ledger, periods, reconciliation and collection), usage metering, custom domains, sessions, and hosting, which orchestrates operations that span several concerns such as creating a relay or banning a tenant. Each concern owns its models, documented where they are defined, and the SQL that reads and writes them. The core uses coracle-lib for Nostr types and cryptography.

**api** is the JSON API. Handlers load what the request names through the core, check the caller against it with the core's rules, and call the core. Routes are grouped by who may call them: public, tenant, and admin. The API reference is the crate's README.

**app** builds the `caravel` binary. It reads and checks the environment, connects two pools (one for requests, one for background work, so workers can't starve requests), builds the core, schedules the background workers, serves the API and shuts down gracefully.

## The core handle

Every core operation takes a `Core`, which carries the database pool, the configuration, the clock, the change bus and the adapters. Functions that only touch the database take a database executor instead, so the same function works on the pool or inside a caller's transaction. Functions that stamp a time take it as an argument; whatever orchestrates them reads the clock once.

The clock is the only source of the current time, and tests pin it. Configuration comes only from the app's config module. Clippy forbids reading either anywhere else.

## Writing to the database

A write that must be atomic runs in one transaction. Billing relies on row locks rather than on a single writer: every billing write that must happen once per tenant and period (renewal, overage, cutting an invoice, churn) first locks the tenant's row and re-checks its marker under the lock. Claims such as marking an activity billed are conditional updates, so a second pass does nothing. The concurrency tests race billing passes against one tenant to keep this true.

A unique constraint a caller can violate (a taken subdomain or custom domain) is named in the migration and translated by name into a validation error.

## The change bus

After a write commits, the core publishes a change, such as a relay changing, on an in-process bus. Subscribers treat a change as a hint to re-read the database; one that falls behind re-reads everything it watches. Until the relay runtime moves into this binary, the bus drives keeping zooid in step with each relay's row.

## Authentication and authorization

The API's own routes authenticate with session tokens. A client signs one NIP-98 request for exactly `POST /session` and gets back an opaque bearer token, good for thirty days, which it sends from then on. Only a hash of the token is stored, so sessions can be ended individually or all at once for a pubkey. Clients from before sessions may instead send a NIP-98 header signed for the API's base url, which is accepted for fifteen minutes after signing.

An authenticated request runs as an actor: a pubkey, and whether it is an admin. Authorization is load-then-authorize: a handler loads the tenant, relay or invoice the request names, then asks the actor whether it may act as that resource's tenant. An extractor gates the admin-only routes. A test calls every route without credentials, and admin routes as a tenant, to make sure none is left open.

## Background work

Workers are scheduled in the app and implemented in the core: hourly billing (which samples usage first), custom domain verification every thirty seconds and re-verification every three hours, a daily sweep of expired sessions, the zooid sync on every relay change and every minute for whatever is pending, and publishing the robot's identity, retried until it succeeds. Each runs on the background pool and stops on shutdown.

## Tests

Tests run against Postgres: each test binary creates its own database from `TEST_DATABASE_URL` on first use (`just pg` provides one), and each test gets its own core and pinned clock, so tests in a binary run in parallel on separate tenants. Adapters in tests point at closed ports, so nothing leaves the machine. The billing tests replay months by moving the clock; the API tests drive the router in process and pin the wire shapes clients depend on.
