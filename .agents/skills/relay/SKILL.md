---
name: relay
description: How caravel serves its virtual relays — many Nostr relays in one process, each on its own hosts, all storing into one Postgres event table partitioned by relay. Covers host resolution and reloads, the websocket session and NIP-01/42/77 conversation, the relay policy chain (who may write, subscribe, and see which event, and who gets signatures), relay membership and NIP-43 joins with claims, NIP-86 management and its per-method grants, NIP-29 rooms, the events a relay publishes about its own state, Blossom media, NIP-9a push, LiveKit calls and their meter, NOTIFY messages, JSONL export and import, the event store's replaceable slots, deletion and vanish tombstones and expiration, fan-out, limits, and the history from zooid (the Go relay engine this replaces) that shaped the design. Use it whenever working on crates/relay, the core's events, relay_policy, media, calls, notifications or transfer modules, or anything that changes what a relay accepts or serves.
---

# Relays

Every relay is a row in the core's `relay` table. The relay feature (`crates/relay`) serves all of the live ones from this process. The event store and the policy rules live in the core (`events` and `relay_policy`), so the API and background work use the same rules as the websocket.

The test inventory in [references/zooid-tests.md](references/zooid-tests.md) maps every test in zooid's suite to the caravel test that covers the same behavior, or says why none does.

## Hosts and reloads

A request reaches a relay by its host. The app looks the host up in the relay registry before anything else: each live relay answers on its canonical host (`<subdomain>.<domain>`), on its verified custom domain, and on the hosts that redirect to it. A page requested on any host but the relay's primary one redirects there, while the protocol itself is served on all of them, since websocket clients don't follow redirects. A host no relay claims gets nothing but the health check and on-demand TLS's ask. Any origin may call a relay's HTTP endpoints and read the `X-Reason` media refusals carry.

Only active, unpurged relays are served. The registry loads them when the process starts, and reloads one whenever the core publishes that it changed. A reload builds the relay's new state and swaps it in, so open connections survive a settings change. A relay that stops being served closes its connections. Nothing between a request's host and its relay's rules reads the database.

## A connection

A connection is challenged the moment it opens (NIP-42), and may prove several pubkeys by answering once for each. An answer's relay tag must name the host the client connected to. Refusals carry NIP-01's machine-readable prefixes, and one that needs authentication re-sends the challenge with it.

A subscription is registered before its stored events are read, so nothing published meanwhile is missed. Stored events come back newest first with ties broken by id, at most a thousand per filter, without duplicates across filters, and then `EOSE`. NIP-77 reconciliation runs over the same rules, on a snapshot of the events the connection may see, capped at twenty thousand. A NIP-45 `COUNT` is admitted like a subscription and counts, once each, the events any of its filters match that the connection may see, whatever their limits; the database counts them in groups sharing a kind, an author and the tags naming a room, which is all the read rule looks at, so the rule decides per group rather than per event.

## The rules

`relay_policy` holds the rules as pure functions over a snapshot of the relay's settings and access lists, which the registry keeps in memory. They run in the order zooid established:

- A write passes NIP-70's protected-event check, then refuses kinds the relay writes itself (its membership records) and Blossom authorizations and events dated more than five minutes ahead, then banned events and authors. A zap receipt or gift wrap addressed to someone with access needs no authentication. Otherwise the publisher must be authenticated as the author (or be publishing the relay's own events), and must have access unless the relay takes public writes.
- A subscription needs authentication unless the relay allows anonymous reads. A private relay admits a connection if any of its pubkeys has access; a public one refuses a connection with a banned pubkey.
- Which events a reader sees is one function, used for stored queries, live delivery, reconciliation and counts alike. Invite codes, join and leave requests and Blossom authorizations are never served, and a push subscription goes back only to its author.
- A relay that strips signatures serves events without a `sig` field to everyone who doesn't manage it, as zooid did: Flotilla holds an unsigned event and asks its user whether to trust the relay, but drops one whose signature fails to verify. The owner and the relay's own key manage it.

Live delivery re-checks whether each subscriber may still read the relay at all, so a reader banned or removed after subscribing stops receiving at once.

## Access

A relay's members are rows in the core's `access` module, the single record of who belongs, along with how each joined: as the owner, added by hand, with a claim, by joining an open relay, by paying, or carried over from zooid. The tenant is a member from the moment the relay is created. Roles, NIP-86 method grants, banned pubkeys and events, and claims are tables beside it.

A NIP-43 join request is admitted for a member already, for anyone on a relay that takes open joins, or for the holder of a live claim; a relay that sells memberships takes no open joins, and says so to a joiner without a claim. A join runs under a lock on the relay row: the claim must still be live, a paid invite's claim gives the joiner its membership (the `memberships` skill covers this), and a new member must fit within the plan's member limit, counting the seats unused paid invites hold. Every join that presents a claim is logged against it, whether or not the claim is what let it in, and a claim's log outlives the claim. A leave request removes the member, except the owner.

NIP-86 calls arrive as `POST`s to the relay's url, signed with NIP-98 over the exact body. NIP-98 and Blossom authorizations may be encoded in standard or url-safe base64. The owner and the relay's own key hold every method; anyone else holds the methods granted to them, and every member of a relay whose members may invite also holds the two claim methods. A method is checked against the in-memory snapshot before it runs.

## Rooms

NIP-29 rooms keep their state in tables: each room's metadata tags and pins, its members with their roles, and its invite codes. A client's moderation event, join or leave is stored as history and applied to its room in the same transaction. An event that tags a room must name it: one with a missing or empty room id is refused, live and on import, and the schema allows no room without an id. The relay's owner and its own key moderate every room, and a room member holding the `admin` role moderates that room. Pubkeys granted relay methods are listed as room admins, as zooid listed them, but may not moderate rooms.

A room nobody created shows only its deletion. A hidden room shows nothing to outsiders and refuses their writes as if it didn't exist; a private room shows outsiders only its metadata; a closed room takes posts only from members, and joins only with an invite. A room's moderators may delete its events, but not the events describing the room.

## What a relay publishes about itself

The relay signs events describing its own state with its own key: its member list (13534), each member's addition and removal (8000, 8001), its role definitions (33534), and each room's metadata, admins, members and pins (39000, 39001, 39002, 39005), with a put-user or remove-user event (9000, 9001) for each join and leave. They are written from the tables in the transaction that changes them, so they never disagree with the state they describe, and no client tombstone refuses them. Rewriting them all from the tables (`rebuild_derived`) reproduces them exactly, which a test checks. After a change, the relay reloads its snapshot before answering, then delivers the change and these events to subscribers.

## Media

A relay with media on runs a Blossom server on its own hosts (BUD-01, 02, 04, 06 and 11). Members upload, mirror, list and delete with a kind 24242 authorization naming the verb, an expiration, and the hashes it covers; anyone downloads, unless the relay keeps its media to members, in which case a download needs a member's authorization too. A blob is at most 10 MiB.

A blob is stored once per relay however many members upload it, and each uploader owns it until they delete it; the last owner's deletion removes its bytes. The bytes live in the platform's blob store under `<relay id>/<sha256>`, in a directory or an S3 bucket. Storage is billed by the blobs a relay holds, each counted once.

A blob's content type is its uploader's choice and it is served on the relay's own origin, so every blob response forbids sniffing and runs under a sandboxing content security policy, and any type a browser could run is served as a download. Refusals carry their reason in `X-Reason`, which browsers may read. Mirroring fetches through the outbound client, which won't reach local or internal addresses.

## Push

A member registers a NIP-9a push subscription as a kind 30390 event naming this relay by any of its hosts, one or more filters, optional ignore filters, and one http(s) callback; a pubkey holds at most ten. Subscriptions are stored as events and indexed with the relay's state, which reloads when one is stored or deleted. Each new event, stored or ephemeral, is pushed to every subscription it matches whose author may read it, except the event's own author: the callback receives the event's id and the relay's url, and the event itself (without its signature, where the relay strips them) if the subscription asks. Deliveries run on a bounded pool. A callback answering 404 drops its subscription at once, and ten failures in a row drop it too.

## Calls

A room tagged `livekit` has calls, on the platform's LiveKit server, for relays with calls on. A member of the relay asks for a token at `/.well-known/nip29/livekit/<room>` with a NIP-98 signature for exactly that url; a room also tagged `restricted` gives tokens only to its own members and the relay's managers. LiveKit room names lead with the relay's id, so one webhook URL, on any relay host, serves every relay's calls. Each departure the webhook reports credits that participant's time to the relay's call clock, once per LiveKit session, before anything that could fail and even if the relay or room has since lost its calls; then the relay publishes who is in the call (39004), from LiveKit's own list. Call time is billed by how far the clock moves in a period.

## Notifications

The platform can leave one message per pubkey on a relay. The relay sends it as `["NOTIFY", message]` to every connection authenticated as that pubkey, at once and whenever the pubkey authenticates again, until it is replaced, cleared or expires.

## Moving events

A relay's events export as JSONL, one event per line, streamed a page at a time. An import stores each validly signed line and reports the ones it refused by line number. It skips the events the source relay published about its own members, roles, media and rooms, then rebuilds each room the dump creates or changes by replaying that room's moderation events, joins and leaves, oldest first; a room the relay doesn't hold is created only by a create event in the dump, and a deletion is replayed only if nothing follows it. Importing the same dump twice changes nothing.

A purged relay ("deleted" in the dashboards) keeps its row, but a worker erases its events, access lists, rooms, notifications and media, and its subdomain is free for a new relay at once.

Erasing a relay clears what it holds while it keeps running on its settings: its events, media, rooms, roles, method grants, bans, claims, notifications and members all go, except its owner, the members whose membership is paid for, and the claims of paid invites nobody has used yet, since those were bought. The relay then signs its member list again.

## The event store

All relays share one `event` table, and every query is scoped to one relay. Ids, pubkeys and signatures are stored as bytes, tags as the JSON they were signed with, and single-letter tags again as `name:value` strings for indexed matching.

Replaceable and addressable events hold one row per slot, which a newer version overwrites in place in a single statement. A version as old as the stored one replaces it, so the last of several written in one second wins. NIP-09 deletions and NIP-62 vanish requests are applied in the transaction that stores them (a vanish request only if it names one of the relay's hosts, or all relays), and stay stored as tombstones that refuse what they removed if it is published again. Expired events (NIP-40) are excluded from every read and swept every minute. An event is fanned out only if this write stored it, so a duplicate is never broadcast twice.

## History

The design keeps the lessons of zooid, the Go relay engine on khatru that served these relays before:

- In August 2026 a zooid change moved its ban checks ahead of the auth gate, where each was an uncached SQL query; unauthenticated spam then cost two queries per event and drove the server to 400% CPU. The rules here read only the in-memory snapshot, so no check before authentication can reach the database.
- zooid's broadcast first asked one shared question per connection, which leaked private group events to non-members; it then moved to a per-reader check but never re-checked bans for open subscriptions. Here every reader is checked on every delivery.
- zooid tore an instance down before rebuilding it on reload, so a failed rebuild left the relay unserved. Here a relay's new state is built first and swapped in.
- zooid kept its own state (members, bans, roles, claims, notifications) as replaceable app-data events, guarded by advisory locks and two tie-break orders. That state is now in tables, so there is one order for events.
- zooid accepted NIP-98 headers without a freshness window until an audit added one; caravel's API checks freshness on every signature it takes.
- zooid pushed every matching event to every subscription, so anyone could subscribe to a members-only relay's events. Here a subscription receives only what its author may read, without signatures where the relay strips them.
- zooid counted push failures per callback url rather than per subscription. Here each subscription keeps its own count.
- zooid delivered live events signed on relays that strip signatures, and broadcast a duplicate again each time it was published. Here live delivery strips like stored reads, and only a write that stored the event fans it out.
- zooid stored a deletion naming another author's event, then refused it and skipped the tags after that one. Here the deletion is accepted and applies to its author's events only.
- zooid's write check had a refusal for room events on a relay with rooms off that could never run, so it stored them as plain events. Here they are refused.
- zooid refused `COUNT`, since its read checks ran per event over query results and a database count couldn't apply them. Here a count applies the same rule, to groups of events it can't tell apart.
- zooid took a room event with a missing or empty `h` tag as naming the room `""`, so a manager's create made that room and the relay signed metadata and admins with `["d", ""]`, and a post with `["h", ""]` was stored outside every room. Here both are refused.
