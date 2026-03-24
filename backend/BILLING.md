# Billing Architecture (Agreed)

This document summarizes the agreed billing architecture for Caravel backend.

## Billing model

- Usage-based billing: **sats/hour** for relay operation.
- A relay is billable when it is provisioned/active in lifecycle terms.
- Billing is **monthly**, with a **rolling cycle anchored to tenant signup**.
- One **consolidated invoice per tenant** per billing period.

## Metering and lifecycle

- Add an append-only lifecycle event table in the backend database.
- Events are the source for usage computation.
- Canonical event timestamp field name: `created_at` (UTC).
- Lifecycle behavior is treated as a state machine for billing math (idempotent outcomes for repeated/no-op transitions).
- Transition validation is permissive (any transition can be recorded); billing logic interprets sequences.
- Billable time behavior:
  - Start on `provisioned`
  - Pause on `suspended`
  - Stop on `deactivated`
  - Resume immediately on unsuspend

## Pricing

- Price is per relay plan/tier in **sats/hour**.
- Rates are stored in a mutable `plans` table (current rate only).
- Mid-cycle plan changes are billed by time spent in each plan.
- Plan rate changes are retroactive for un-invoiced usage in the current open period.

## Rounding and minimums

- Round usage up to the next full hour.
- Minimum charge: **1 billable hour per relay per month**.

## Invoice generation

- A periodic worker creates invoices at billing boundaries.
- Existing relays at launch start billing from launch timestamp only (no historical backfill).
- Avoid duplicate invoices with a DB unique constraint on:
  - `(tenant, period_start, period_end)`

## Invoice status and attempts

- `invoice_attempts` is the canonical history/state source.
- `invoices.status` is a synchronous projection updated in the same transaction as attempt writes.
- Each payment method attempt is its own row in `invoice_attempts`.
- Attempts in a single retry pass share a `run_id` UUID.

## Collection order and fallback

For each invoice collection run:

1. Try **NWC** auto-pay
2. If not paid, try **Stripe** auto-pay
3. If still unpaid/unavailable, create Lightning invoice and show QR in-app
4. If neither NWC nor Stripe is configured, send a one-time **NIP-17 DM** with invoice/subscription status

Notes:

- Retry cadence: every 24 hours (NWC/Stripe retries).
- Do **not** resend DMs on retries.
- Lightning invoice refresh is in-app only when prior invoice expires.
- DM send is recorded as an `invoice_attempts` row (same `run_id` as triggering run).

## Due dates, grace, and enforcement

- Invoice due time is derived as: `invoice.created_at + 7 days`.
- Grace period: 7 days, relay service remains fully active during grace.
- If still unpaid after grace, billing flow marks tenant/account past due and performs billing-side handling.
- Full outstanding balance must be paid before billing status is considered clear.

## Tenant and integration storage

- Store billing cycle anchor on `tenants` (e.g., `billing_anchor_at`).
- Anchor can be reset when tenant goes from no non-free relays to having one again.
- Determine “has billable relays” by querying relays on demand (no counter cache).
- Keep NWC config in `tenants.nwc_url`.
- Store Stripe IDs directly on `tenants`.

## Worker and runtime model

- Scheduler runs inside backend service process.
- Multiple instances may run; correctness relies on DB idempotency and unique constraints.

## Repository impact

- Add migration(s) for lifecycle events and billing-related schema changes.
- Add repository methods in `backend/src/repo.rs` for:
  - writing lifecycle events
  - reading lifecycle events by relay/tenant/time window
  - creating/fetching invoices with period boundaries
  - writing invoice attempts and projecting invoice status atomically
