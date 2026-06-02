# Data layer and schema (deep detail)

This is the lookup-depth companion to the SKILL.md "data layer" section: read-helper assembly, transaction-helper conventions, the idempotency idioms in full, the `Snapshot` enum, the i64/timestamp modeling, the strict-`<` historical reads, and the schema/migration rules. All of it lives in `query.rs`, `command.rs`, `models.rs`, `db.rs`, and `migrations/0001_init.sql`.

## Read assembly

Reads use `query_as::<_, T>` with `T` deriving `sqlx::FromRow`, returning typed structs (`Tenant`, `Relay`, `Activity`, `Invoice`, `InvoiceItem`, `Bolt11`). The `SELECT *` body is built by per-table `select_tenant`/`select_relay`/`select_activity` string helpers that append a trailing `WHERE`/`ORDER` clause; one-off reads inline the SQL but still go through `query_as`. Tenant-scoped reads take a `tenant_pubkey` param and filter on `tenant_pubkey`, but the `_for_tenant` suffix is **not** a reliable marker of tenant scoping: only two reads carry it (`list_relays_for_tenant`, `list_invoices_for_tenant`), while `list_open_invoices`, `list_unbilled_invoice_items`, and `list_billable_activity` are tenant-scoped without the suffix. (Note also that `list_plans`/`get_plan` are synchronous, not async.) Source: `query.rs:6-16,58-262`.

## Transaction conventions

`with_tx` is the only primitive: it begins a tx on the pool, runs the async closure with a `&mut Transaction`, commits on `Ok`, and relies on `Transaction`'s `Drop` to roll back on `Err` — there is no explicit `rollback()` call, so a closure that swallows an error and returns `Ok` will commit a partial write. Source: `db.rs:60-68`.

The `*_tx` helpers are private, suffixed `_tx`, take `&mut Transaction` as their first param, and run via `.execute(&mut **tx)`. Public commands compose them inside one `with_tx` closure and never take a transaction. A `*_tx` that records a state change *returns* the constructed `Activity`, and the public command publishes it after commit. Some single-statement writes run directly on `pool()` with no transaction at all: `create_tenant`, `update_tenant`, the `set_tenant_*` setters, `mark_invoice_notified`, and `insert_bolt11`. Source: `command.rs:14-82,146-183,466-704`, `db.rs:60-68`.

## The idempotency idioms in full

- `mark_activity_billed_tx` runs `UPDATE ... WHERE id = ? AND billed_at IS NULL` and returns `rows_affected() > 0` — a bool you must honor, because ignoring a `false` and inserting the invoice item anyway hits the `UNIQUE(activity_id)` index and fails.
- `insert_invoice_item_for_activity` claims the activity first and only inserts the line item when the claim won, so a concurrent reconcile never double-bills.
- Conditional monotonic `UPDATE`s are guarded on null markers: `mark_invoice_paid_tx`, `mark_bolt11_settled_tx`, and `void_open_invoices_tx`.
- `insert_intent_tx` records the Stripe `PaymentIntent` with `INSERT ... ON CONFLICT(id) DO NOTHING`, making settlement idempotent on retried webhooks.
- Renewal re-reads `renewed_at` inside the tx and advances it, so it is idempotent per period.
- `UNIQUE(invoice_item.activity_id)` is the database backstop behind the claim.
- `insert_activity_tx` requires the relay row to already exist in the same tx — it fetches the relay's `tenant_pubkey` — so insert/update the relay before logging.

Source: `command.rs:279-335,563-704`, `migrations/0001_init.sql:111-112`.

## The Snapshot type

`Snapshot` is a serde-tagged enum keyed on `resource_type`, with one variant per resource that logs activity, wrapped in `sqlx::types::Json` on insert. Each `Activity` carries a JSON snapshot of the resource's plan+status. Add a variant (and its `resource_type()`) when a new resource type starts logging activity. Source: `models.rs:8-24`, `command.rs:174-178`.

## Timestamp-vs-enum modeling and the i64 convention

Lifecycle is nullable timestamp markers, not status enums: an invoice is open while `paid_at` and `voided_at` are both null, a tenant is churned once `churned_at` is set, a bolt11 is settled once `settled_at` is set; `invoice.method` records provenance only when paid. Filter on the timestamps. Relay status is the exception — a free-form `TEXT` column with **no** `CHECK` constraint and no Rust enum, guarded only by the `RELAY_STATUS_*` consts (by contrast `invoice.method` *does* have a `CHECK`), so a typo'd status string would persist silently. Boolean-ish columns are `i64` 0/1, not Rust `bool` — `list_relays_pending_sync` uses `synced = 0 OR TRIM(sync_error) != ''`. Source: `models.rs:54-94,120-142`, `query.rs:81-121,206-240`, `migrations/0001_init.sql:32,48-60`.

## Strict-`<` historical lookups

`get_relay_plan_before` / `get_latest_relay_activity_before` use `created_at < before` (strict `<`, not `<=`) when reconstructing historical relay state from the activity log. A relay created exactly at a period boundary is intentionally not counted active in the prior period (its own creation/change charge covers that period); using `<=` would double-charge the creation period. `list_billable_activity` does **not** use a timestamp boundary at all — it has no `before` param and selects a tenant's unbilled activity via the `billed_at IS NULL` marker (plus an `activity_type` filter), reconciling off a precise marker rather than a timestamp watermark. Source: `query.rs:108-121,206-218,225-240`.

## Schema and migration rules

The whole schema lives in a single migration, `0001_init.sql`. Pre-release, schema changes are **squashed** into that file rather than appended as new files; migrations become append-only only after release. `db::init` runs create-if-missing plus WAL plus `./migrations`. Relative `sqlite://` `DATABASE_URL` paths are rewritten against `CARGO_MANIFEST_DIR` (the compile-time backend crate dir), not the process cwd. FK columns are named `{model}_{pk}`. Plans are hardcoded in-memory and synchronous — `list_plans`/`get_plan` are not a DB table, so a new plan is a code edit, not a migration. Source: `db.rs:70-108`, `migrations/0001_init.sql:1-115`, `query.rs:20-54`, root `AGENTS.md:20-22`.

## A few "Ok with no write" cases

`create_invoice` and `insert_invoice_items_for_renewal` can legitimately return `Ok` with no write: a non-positive outstanding balance returns `Ok(None)` (credit carries forward) and empty renewal items returns early `Ok(())`. Don't treat a missing invoice as an error. `insert_bolt11` uses `INSERT ... RETURNING *` with `fetch_optional` and returns `Option<Bolt11>`, so handle the `Option` rather than unwrapping. `set_relay_status_tx` (and `update_relay`) always reset `synced = 0` as a side effect of any status/field change, re-queuing the relay for the infra reactor. Source: `command.rs:185-223,300-335,443-464`.

## Sources

- read assembly + `_for_tenant` suffix — `backend/src/query.rs:6-16,58-262`
- transaction conventions — `backend/src/command.rs:14-82,146-183,466-704`, `backend/src/db.rs:60-68`
- idempotency idioms — `backend/src/command.rs:279-335,563-704`, `backend/migrations/0001_init.sql:111-112`
- `Snapshot` — `backend/src/models.rs:8-24`, `backend/src/command.rs:174-178`
- timestamp/i64/status modeling — `backend/src/models.rs:54-94,120-142`, `backend/src/query.rs:81-121`, `backend/migrations/0001_init.sql:32,48-60`
- strict-`<` historical reads (and the marker-based `list_billable_activity`) — `backend/src/query.rs:108-121,206-218,225-240`
- schema/migration rules — `backend/src/db.rs:70-108`, `backend/migrations/0001_init.sql:1-115`, `backend/src/query.rs:20-54`, root `AGENTS.md:20-22`
- Ok-with-no-write + side effects — `backend/src/command.rs:185-223,300-335,443-464`
