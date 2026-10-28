# Style guide

## Comments

Keep comments minimal, one line if possible.

There is one right place to document any given information:

- Functions should have a doc comment explaining the purpose of the function, not its implementation.
- Only very strange behavior should be documented using non-doc comments.
- Models and their fields are documented where the model is defined, in `crates/domain` (e.g. `Relay` in `crates/domain/billing/src/relays/model.rs`), not in migrations, implementations, or anywhere in the web apps.
- Database indexes are documented in migrations.

## Data Modeling

When naming a foreign key, always use `{model}_{pk}`, for example `relay.tenant_pubkey`.

When referring to a tenant's pubkey, always name it `tenant_pubkey`, not `tenant` or `pubkey`. The exception to this is when we're in a context where we're already talking about tenants, e.g. `tenant.pubkey`, `get_tenant(pubkey)` or any tenant-related routes.

## Migrations

Pre-release: squash schema changes into `0001_init.sql` rather than adding new migration files. Once released, migrations become append-only.

Document indexes (what use cases they support), but not tables (those are documented on their model in the core).

## Markdown

Do not hard-break markdown files at a certain number of characters. Allow readers to implement line wrapping naturally instead.

## Rust

Prefer `&str` over `&String` for function parameters — `&str` accepts both `&String` (via deref coercion) and string literals, so it's strictly more flexible. Only take `String` when you need ownership (storing in a struct, mutating, or transferring ownership).

Avoid passing `&mut` to functions. The performance improvement often comes at the cost of poor abstraction boundaries and error prone business logic. Instead, return results to the caller which can manage mutability itself, or re-calculate/fetch mutable data.

Don't be overly DRY. Deep call trees are harder to read; factoring functions into many tiny pieces means that function boundaries are defined less by the domain or the responsibility of a given piece of code than by coincidental similarity. New functions should be created when 1. they represent a different concern that is the responsibility of a different part of the codebase, 2. the contained logic is repeated 3+ times, or 3. the contents of the function are complex and naming them makes the logical flow of the code easier to follow.

## Typescript

Don't add re-export shims to preserve old import paths. When something moves, import it from its new canonical location at every call site and update all the imports. Each symbol has exactly one place it's exported from.

## Architecture

The backend is one Cargo workspace under `crates/`, layered so each crate depends only on the ones below it: `util` at the bottom (the clock and bus, the errors, configuration, Postgres plumbing, and the clients for systems outside the database), then the domain crates in `crates/domain` (`relays`, `billing`, `operations`, each a group of modules holding its models, the pure rules over them, its queries and its commands), then `core` (the `Core` handle and the subsystems that adapt service calls to the domain), then the services in `crates/services` (`api`, `relay`, `worker`), then `app`, which builds the `caravel` binary. `crates/app/tests/layers.rs` enforces the layering, and `crates/core/tests/layers.rs` that models stay pure, that only queries and commands touch the database, and that services reach it only through the core. `DOMAIN.md` is the vocabulary the code is organized around.

Read configuration from the environment only in `crates/app/src/config.rs`, and the time only through `caravel_util::clock::Clock`; clippy enforces both.

## Verification

Check justfile and web/package.json for common commands for linting/building. Tests need Postgres: `just pg` starts one with podman, and each test binary creates its own database on it.

Pull requests run `.gitea/workflows/ci.yml`, which is the workspace's fmt/clippy/test against a Postgres service plus the web build. Keep it in step with the justfile.
