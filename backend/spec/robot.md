# `pub struct Robot`

Robot is a nostr identity which acts on behalf of the application.

Members:

- `secret: String` - a nostr secret key, from `ROBOT_SECRET`
- `name: String` - the name of the bot, from `ROBOT_NAME`
- `description: String` - the description of the bot, from `ROBOT_DESCRIPTION`
- `picture: String` - the picture URL for the bot, from `ROBOT_PICTURE`
- `outbox_relays: Vec<String>` - outbox relay URLs, from `ROBOT_OUTBOX_RELAYS`
- `indexer_relays: Vec<String>` - indexer relay URLs, from `ROBOT_INDEXER_RELAYS`
- `messaging_relays: Vec<String>` - messaging relay URLs, from `ROBOT_MESSAGING_RELAYS`
- `client: nostr_sdk::Client`

## `pub fn new() -> Self`

- Reads environment and populates members
- Publishes a `kind 0` nostr profile, a `kind 10002` relay list, and `kind 10050` relay selections using `client`

## `pub async fn send_dm(&self, recipient: &str, message: &str) -> Result<()>`

- Fetches recipient's outbox relays from `indexer_relays` (cached)
- Fetches recipient's messaging relays from their outbox relays (cached)
- Sends DM to recipient via their messaging relays
- If no outbox/messaging relays are found, return an error
