# `pub struct Robot`

Robot is the nostr identity that acts on behalf of the application — it publishes the app's profile/relay lists and sends DMs to tenants. It signs with the robot key in `env.keys` and builds nostr clients on demand from the relay lists in `env`.

Members:

- `env: Env` - configuration; supplies the robot key and the outbox/indexer/messaging relay lists and profile metadata
- `outbox_cache` / `dm_cache` - per-recipient caches (5 minute TTL) of discovered outbox and messaging relays

## `pub async fn new(env: &Env) -> Result<Self>`

- Stores a clone of `env` and initializes the caches
- Calls `publish_identity`, which publishes a `kind 0` profile and a `kind 10002` relay list (the `ROBOT_OUTBOX_RELAYS`, as `r` tags) to the outbox relays, and a `kind 10050` DM relay list (the `ROBOT_MESSAGING_RELAYS`, as `relay` tags) via the indexer relays

## `pub async fn send_dm(&self, recipient: &str, message: &str) -> Result<()>`

- Resolves the recipient's outbox relays (`fetch_outbox_relays`), then their messaging relays from those outbox relays (`fetch_messaging_relays_from_outbox`)
- Sends a NIP-17 private message to the recipient via their messaging relays
- Errors if no outbox or messaging relays are found

## `pub async fn fetch_nostr_name(&self, pubkey: &str) -> Option<String>`

- Fetches the recipient's `kind 0` metadata from the indexer relays and returns its `display_name` (falling back to `name`), trimmed and non-empty
- Returns `None` on any failure — used to derive a Stripe customer display name

## `async fn fetch_outbox_relays(&self, recipient: &str) -> Result<Vec<String>>`

- Returns the `r` tags from the recipient's latest `kind 10002` event, fetched from the indexer relays; cached for 5 minutes

## `async fn fetch_messaging_relays_from_outbox(&self, recipient: &str, outbox_relays: &[String]) -> Result<Vec<String>>`

- Returns the `relay` tags from the recipient's latest `kind 10050` event, fetched from their outbox relays; cached for 5 minutes
