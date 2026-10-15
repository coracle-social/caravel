---
name: relay
description: How caravel serves its virtual relays — many Nostr relays in one process, each on its own hosts, all storing into one Postgres event table partitioned by relay. Covers host resolution and reloads, the websocket session and NIP-01/42/77 conversation, the relay policy chain (who may write, subscribe, and see which event, and who gets signatures), the event store's replaceable slots, deletion and vanish tombstones and expiration, fan-out, limits, and the history from zooid (the Go relay engine this replaces) that shaped the design. Use it whenever working on crates/relay, the core's events or relay_policy modules, or anything that changes what a relay accepts or serves.
---

# Relays

Every relay is a row in the core's `relay` table. The relay feature (`crates/relay`) serves all of the live ones from this process. The event store and the policy rules live in the core (`events` and `relay_policy`), so the API and background work use the same rules as the websocket.

The test inventory in [references/zooid-tests.md](references/zooid-tests.md) maps every test in zooid's suite to the caravel test that covers the same behavior, or says why none does.

## Hosts and reloads

A request reaches a relay by its host. The app looks the host up in the relay registry before anything else: each live relay answers on its canonical host (`<subdomain>.<domain>`), on its verified custom domain, and on the hosts that redirect to it. A page requested on any host but the relay's primary one redirects there, while the protocol itself is served on all of them, since websocket clients don't follow redirects. A host no relay claims goes to the API.

Only active, unpurged relays are served. The registry loads them when the process starts, and reloads one whenever the core publishes that it changed. A reload builds the relay's new state and swaps it in, so open connections survive a settings change. A relay that stops being served closes its connections. Nothing between a request's host and its relay's rules reads the database.

## A connection

A connection is challenged the moment it opens (NIP-42), and may prove several pubkeys by answering once for each. An answer's relay tag must name the host the client connected to. Refusals carry NIP-01's machine-readable prefixes, and one that needs authentication re-sends the challenge with it.

A subscription is registered before its stored events are read, so nothing published meanwhile is missed. Stored events come back newest first with ties broken by id, at most a thousand per filter, without duplicates across filters, and then `EOSE`. NIP-77 reconciliation runs over the same rules, on a snapshot of the events the connection may see, capped at twenty thousand. `COUNT` is refused.

## The rules

`relay_policy` holds the rules as pure functions over a snapshot of the relay's settings and access lists, which the registry keeps in memory. They run in the order zooid established:

- A write passes NIP-70's protected-event check, then refuses kinds the relay writes itself (its membership records, blob descriptors) and events dated more than five minutes ahead, then banned events and authors. A zap receipt or gift wrap addressed to someone with access needs no authentication. Otherwise the publisher must be authenticated as the author (or be publishing the relay's own events), and must have access unless the relay takes public writes.
- A subscription needs authentication unless the relay allows anonymous reads. A private relay admits a connection if any of its pubkeys has access; a public one refuses a connection with a banned pubkey.
- Which events a reader sees is one function, used for stored queries, live delivery and reconciliation alike. Invite codes, join and leave requests and blob descriptors are never served, and a push subscription goes back only to its author.
- A relay that strips signatures serves events with a zeroed signature to everyone who doesn't manage it. The owner and the relay's own key manage it.

Live delivery re-checks whether each subscriber may still read the relay at all, so a reader banned or removed after subscribing stops receiving at once.

## The event store

All relays share one `event` table, and every query is scoped to one relay. Ids, pubkeys and signatures are stored as bytes, tags as the JSON they were signed with, and single-letter tags again as `name:value` strings for indexed matching.

Replaceable and addressable events hold one row per slot, which a newer version overwrites in place in a single statement. A version as old as the stored one replaces it, so the last of several written in one second wins. NIP-09 deletions and NIP-62 vanish requests are applied in the transaction that stores them, and stay stored as tombstones that refuse what they removed if it is published again. Expired events (NIP-40) are excluded from every read and swept every minute. An event is fanned out only if this write stored it, so a duplicate is never broadcast twice.

## History

The design keeps the lessons of zooid, the Go relay engine on khatru that served these relays before:

- In August 2026 a zooid change moved its ban checks ahead of the auth gate, where each was an uncached SQL query; unauthenticated spam then cost two queries per event and drove the server to 400% CPU. The rules here read only the in-memory snapshot, so no check before authentication can reach the database.
- zooid's broadcast first asked one shared question per connection, which leaked private group events to non-members; it then moved to a per-reader check but never re-checked bans for open subscriptions. Here every reader is checked on every delivery.
- zooid tore an instance down before rebuilding it on reload, so a failed rebuild left the relay unserved. Here a relay's new state is built first and swapped in.
- zooid kept its own state (members, bans, roles, claims, notifications) as replaceable app-data events, guarded by advisory locks and two tie-break orders. That state is now in tables, so there is one order for events.
- zooid accepted NIP-98 headers without a freshness window until an audit added one; caravel's API checks freshness on every signature it takes.
