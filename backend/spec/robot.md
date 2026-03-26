# `pub struct Robot`

Robot is a nostr identity which acts on behalf of the application.

Members:

- `secret: String` - a nostr secret key, from `ROBOT_SECRET`
- `name: String` - the name of the bot, from `ROBOT_NAME`
- `description: String` - the description of the bot, from `ROBOT_DESCRIPTION`
- `picture: String` - the picture URL for the bot, from `ROBOT_PICTURE`
- `outbox_client: nostr_sdk::Client` - used for publishing relay lists and metadata, connects to `ROBOT_OUTBOX_RELAYS`
- `indexer_client: nostr_sdk::Client` - used for publishing relay lists, connects to `ROBOT_INDEXER_RELAYS`
- `messagins_client: nostr_sdk::Client` - used for sending and receiving dms, connects to `ROBOT_MESSAGING_RELAYS`

## `pub fn new() -> Self`

- Reads environment and populates members. Relay urls should be split and normalized.
- Publishes a `kind 0` nostr profile, a `kind 10002` relay list, and `kind 10050` relay selections

## `pub async fn send_dm(&self, recipient: &str, message: &str) -> Result<()>`

- Fetches recipient's outbox relays from `indexer_relays` (cached)
- Fetches recipient's messaging relays from their outbox relays (cached)
- Sends DM to recipient via their messaging relays
- If no outbox/messaging relays are found, return an error
