# `pub struct Env`

Env is the application's configuration, loaded once at startup and cloned into every service that needs it (`Api`, `Query`, `Command`, `Billing`, `Infra`, `Robot`, `Stripe`). It is the single source of truth for environment-derived settings, and it also owns the robot nostr key used for signing, NIP-44 encryption, and NIP-98 auth.

Members (all populated from environment variables):

- `server_host: String` - from `SERVER_HOST`; also used for the NIP-98 `u` host check
- `server_port: u16` - from `SERVER_PORT`
- `server_admin_pubkeys: Vec<String>` - admin pubkeys from `SERVER_ADMIN_PUBKEYS`
- `server_allow_origins: Vec<String>` - CORS origins from `SERVER_ALLOW_ORIGINS`
- `database_url: String` - from `DATABASE_URL`
- `robot_name: String` - from `ROBOT_NAME`
- `robot_wallet: String` - the system NWC URL from `ROBOT_WALLET`, used to issue/look up bolt11 invoices
- `robot_picture: String` - from `ROBOT_PICTURE`
- `robot_description: String` - from `ROBOT_DESCRIPTION`
- `robot_outbox_relays: Vec<String>` - from `ROBOT_OUTBOX_RELAYS`
- `robot_indexer_relays: Vec<String>` - from `ROBOT_INDEXER_RELAYS`
- `robot_messaging_relays: Vec<String>` - from `ROBOT_MESSAGING_RELAYS`
- `blossom_s3_region` / `blossom_s3_bucket` / `blossom_s3_endpoint` / `blossom_s3_access_key` / `blossom_s3_secret_key: String` - from the matching `BLOSSOM_S3_*` vars
- `zooid_api_url: String` - from `ZOOID_API_URL`
- `relay_domain: String` - from `RELAY_DOMAIN`
- `livekit_url` / `livekit_api_key` / `livekit_api_secret: String` - from the matching `LIVEKIT_*` vars
- `stripe_secret_key: String` - from `STRIPE_SECRET_KEY`
- `stripe_webhook_secret: String` - from `STRIPE_WEBHOOK_SECRET`
- `stripe_price_basic: String` - Stripe price id for the Basic plan, from `STRIPE_PRICE_BASIC`
- `stripe_price_growth: String` - Stripe price id for the Growth plan, from `STRIPE_PRICE_GROWTH`
- `keys: Keys` - parsed from `ROBOT_SECRET`; used for nostr signing, NIP-44 encryption, and NIP-98 auth

## `pub fn load() -> Self`

- Reads every variable above and panics if any is missing or malformed.
- String vars must be present and non-blank (trimmed).
- The port must parse as a `u16`.
- CSV vars are split on commas, trimmed, and empties dropped; each must contain at least one entry.
- `keys` is parsed from `ROBOT_SECRET` and panics if it is not a valid nostr secret key.

## `pub fn encrypt(&self, plaintext: &str) -> Result<String>`

- NIP-44 (v2) encrypts `plaintext` to the robot's own key. Used to encrypt a tenant's `nwc_url` at rest.

## `pub fn decrypt(&self, ciphertext: &str) -> Result<String>`

- NIP-44 decrypts a value previously produced by `encrypt`.

## `pub async fn make_auth(&self, url: &str, method: HttpMethod) -> Result<String>`

- Builds a NIP-98 `Authorization` header value for an outgoing request to `url` with `method`, signed with `keys`. Used by `Infra` to authenticate requests to zooid.
