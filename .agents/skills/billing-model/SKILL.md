---
name: billing-model
description: Conceptual, user-story-level overview of how Caravel bills tenants for their relays — plans, proration, invoices, automatic payment collection, payment-method errors, dunning, churn/delinquency, and reactivation. Use this when working on or reasoning about anything billing-related, to understand the intended domain behavior before touching the code.
---

# Caravel billing model

This explains *what* the billing system is supposed to do and *why*, in plain domain terms. It is deliberately free of implementation detail — no functions, tables, or fields — so it stays true as the code evolves. Reach for the code for the *how*; reach for this for the *intent*.

## The cast

- **Tenant** — a customer account, identified by a Nostr pubkey. Everything is billed to a tenant.
- **Relay** — a hosted relay a tenant runs. A tenant can have many. Each relay sits on one plan and is either active, inactive, or delinquent.
- **Plan** — a tier (e.g. free, basic, growth). The free tier costs nothing; paid tiers have a flat monthly price and unlock features.

Only active relays on a paid plan ever cost money. Free relays, inactive relays, and delinquent relays are not charged.

## The guiding idea: bill from history, not from "now"

Every meaningful change to a relay — created, plan changed, deactivated, reactivated — is recorded as a dated event that also captures what the relay looked like at that instant (its plan and status). Billing is computed by replaying these events, never by reading the relay's *current* settings.

Why this matters: if a customer was on the growth plan last week and downgraded today, last week must still be billed at the growth price. Pricing the past from the present would overcharge or undercharge. Treat the event history as the source of truth for money.

## Billing periods

Each tenant is billed in monthly cycles. The cycle is anchored to the moment of their first billable activity, and each period is a whole calendar month from that anchor. A customer who starts on the 7th is billed on the 7th, and so on.

## How charges arise (the user stories)

**"I created a paid relay mid-month."** The customer is charged immediately, but only for the slice of the current period that remains — a prorated charge, not a full month.

**"I upgraded (or downgraded) a relay mid-month."** The customer is charged (or credited) the prorated *difference* between the old and new plan for the rest of the period. Downgrades and removals can produce credits.

**"I deactivated a relay mid-month."** The customer receives a prorated credit for the unused remainder of the period.

**"A relay was purged mid-month."** Purging deletes a relay's data for good (a tenant can purge their own relay; an admin can purge anyone's). Billing treats it exactly like a deactivation, whoever triggered it — ban included: if the relay was active, the customer receives a prorated credit for the rest of the period; if it was already inactive or delinquent, there is nothing to refund.

**"A new month started."** Every relay that was active on a paid plan at the moment the new period began is charged a full month. A relay created partway through the previous month already paid its prorated slice, so the renewal and the proration compose to exactly one fair month — never double-charged.

Credits and charges accumulate together. A customer's net balance can be zero or negative; in that case nothing is billed and the credit simply carries forward to the next time there is something to pay.

## Invoices

When a tenant has a positive outstanding balance, the pending charges and credits are gathered into a single invoice for the period. An invoice moves through a simple lifecycle expressed as *when* things happened rather than as a status label:

- **Open** — issued and awaiting payment.
- **Paid** — settled (and we remember how: Lightning, card, or a manual/out-of-band payment).
- **Void** — forgiven and no longer collectible (used when an account churns).

## Collecting payment (the cascade)

When an invoice is open, the system tries to collect on the customer's behalf, in order of least friction:

1. **The customer's Lightning wallet**, if they've connected one for automatic payments.
2. **A saved card**, if they have one on file.
3. **A manual nudge** — if automatic methods don't go through, the customer is sent a direct message with a link to pay the invoice themselves, by Lightning or card.

The system also guards against double payment: if an invoice was already settled out of band (e.g. the customer paid the Lightning request directly), that is recognized and the invoice is not collected again.

## When automatic payment fails

**The failure reason is remembered and shown to the customer.** If the Lightning wallet or the card is declined, we keep the most recent error for each method so the UI can warn the customer that something is wrong with their payment setup — separately for the wallet and the card, since they can fail independently.

**A stored error never stops us from trying again.** Recording the problem is purely informational. The next collection attempt still runs; the relevant warning is cleared automatically the moment that method succeeds.

**Unpaid invoices are retried, within a grace period.** An open invoice is re-attempted on each billing cycle. The customer has a **7-day grace period** from when the invoice was issued to get payment working.

## Churn and delinquency

If an invoice is still unpaid once its grace period has elapsed, the account **churns**:

- The tenant's active relays are marked **delinquent**, pausing service.
- The outstanding balance is **forgiven** (the invoice is voided). We stop chasing money the customer clearly isn't going to pay; the unpaid amount is not carried as a debt.

Delinquency is the visible signal — to the customer and to admins — that the account lapsed for non-payment.

## Coming back (reactivation)

A churned customer who re-engages with the service is welcomed back automatically: their churn is cleared and their delinquent relays are restored to active. Critically, **old unpaid invoices do not have to be settled to return** — the past balance was already forgiven at churn, so reactivation is a clean slate rather than a debt collection.

## Banning (administrative, not billing)

An admin can **ban** a tenant for abuse. Banning is harsher than churn and is not part of the dunning cycle: it blocks the tenant from creating relays, cancels billing (open invoices are voided and outstanding charges forgiven, and the billing loop leaves the tenant alone while banned), and **purges** every relay the tenant has — the relays' data and config are deleted from the hosting backend for good. The purge credits still accrue, like any other: they sit on the tenant's ledger and offset future charges if the tenant is ever welcomed back. Purged relays are never billed, synced, or re-activated; the database rows remain only as a historical record. Lifting a ban restores nothing that was destroyed — the tenant simply starts over from no relays, and billing resumes naturally when they create new ones.

## Principles to preserve

- **Bill the past at its historical price.** Always price a change from the state captured when it happened, not from the relay's current settings.
- **Never double-charge.** Proration and renewal must compose to one fair month; collection must tolerate retries and out-of-band payments idempotently.
- **Errors inform, they don't block.** Surface payment problems to the customer, but keep retrying.
- **Forgive on churn, don't accrue debt.** A lapsed customer's old balance is written off, and returning never requires paying it.
