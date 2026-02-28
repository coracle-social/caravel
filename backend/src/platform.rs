use anyhow::{Result, anyhow};
use nostr_sdk::prelude::*;

pub async fn publish_platform_identity(
    platform_secret: &str,
    indexer_relays: &[String],
    name: &str,
    description: &str,
    picture: &str,
    messaging_relays: &[String],
) -> Result<()> {
    if indexer_relays.is_empty() {
        return Ok(());
    }

    if platform_secret.trim().is_empty() {
        return Err(anyhow!("PLATFORM_SECRET is required for platform identity"));
    }

    let keys = Keys::parse(platform_secret)?;
    let client = Client::new(keys);

    for relay in indexer_relays {
        client.add_relay(relay).await?;
    }

    client.connect().await;

    let mut metadata = Metadata::new();
    if !name.is_empty() {
        metadata = metadata.name(name);
    }
    if !description.is_empty() {
        metadata = metadata.about(description);
    }
    if !picture.is_empty() {
        metadata = metadata.picture(Url::parse(picture)?);
    }

    let metadata_builder = EventBuilder::metadata(&metadata);
    client.send_event_builder(metadata_builder).await?;

    if messaging_relays.is_empty() {
        return Ok(());
    }

    let mut tags = Vec::new();
    for relay in messaging_relays {
        let tag = Tag::parse(["relay", relay.as_str()])?;
        tags.push(tag);
    }

    let relay_builder = EventBuilder::new(Kind::Custom(10050), "").tags(tags);
    client.send_event_builder(relay_builder).await?;

    Ok(())
}
