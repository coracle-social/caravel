use anyhow::{anyhow, Result};
use std::str::FromStr;

use nostr_sdk::nostr::key::PublicKey;
use nostr_sdk::nostr::nips::nip98::{self, HttpMethod};
use nostr_sdk::nostr::types::time::Timestamp;
use nostr_sdk::nostr::types::url::Url;

pub fn verify_nip98(auth_header: &str, url: &str, method: &str) -> Result<PublicKey> {
    let url = Url::parse(url).map_err(|e| anyhow!(e))?;
    let method = HttpMethod::from_str(&method.to_uppercase()).map_err(|e| anyhow!(e))?;
    let now = Timestamp::now();

    nip98::verify_auth_header(auth_header, &url, method, now, None).map_err(|e| anyhow!(e))
}
