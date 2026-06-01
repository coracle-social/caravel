# Style guide

## Comments

Keep comments minimal, one line if possible.

There is one right place to document any given information:

- Functions should have a doc comment explaining the purpose of the function, not its implementation.
- Only very strange behavior should be documented using non-doc comments.
- Models and their fields are documented in models.rs, not in migrations, implementations, or anywhere on the frontend.
- Database indexes are documented in migrations.

## Data Modeling

When naming a foreign key, always use `{model}_{pk}`, for example `relay.tenant_pubkey`.

When referring to a tenant's pubkey, always name it `tenant_pubkey`, not `tenant` or `pubkey`. The exception to this is when we're in a context where we're already talking about tenants, e.g. `tenant.pubkey`, `get_tenant(pubkey)` or any tenant-related routes.

## Migrations

Pre-release: squash schema changes into `0001_init.sql` rather than adding new migration files. Once released, migrations become append-only.

## Markdown

Do not hard-break markdown files at a certain number of characters. Allow readers to implement line wrapping naturally instead.

## Rust

Prefer `&str` over `&String` for function parameters — `&str` accepts both `&String` (via deref coercion) and string literals, so it's strictly more flexible. Only take `String` when you need ownership (storing in a struct, mutating, or transferring ownership).

Avoid passing `&mut` to functions. The performance improvement often comes at the cost of poor abstraction boundaries and error prone business logic. Instead, return results to the caller which can manage mutability itself, or re-calculate/fetch mutable data.

Don't be overly DRY. Deep call trees are harder to read; factoring functions into many tiny pieces means that function boundaries are defined less by the domain or the responsibility of a given piece of code than by coincidental similarity. New functions should be created when 1. they represent a different concern that is the responsibility of a different part of the codebase, 2. the contained logic is repeated 3+ times, or 3. the contents of the function are complex and naming them makes the logical flow of the code easier to follow.

## Verification

Check justfile and frontend/package.json for common commands for linting/building.

## Skills

Skills should be created and maintained when updating the codebase. Before creating a skill, check to see if a relevant one already exists.

Avoid including code in skills unless the purpose of the skill is to illustrate coding principles; architecture and domain should be explained in plain English and provide a high-level overview of the topic without going into implementation details.
