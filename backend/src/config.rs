use std::env;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub host: String,
    pub port: u16,
    pub admin_pubkeys: Vec<String>,
    pub zooid_api_url: String,
    pub platform_secret: String,
    pub relay_domain: String,
    pub platform_nwc_url: String,
    pub indexer_relays: Vec<String>,
    pub platform_name: String,
    pub platform_description: String,
    pub platform_picture: String,
    pub platform_messaging_relays: Vec<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/caravel.db".to_string());
        let database_url = resolve_database_url(database_url);
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);
        let admin_pubkeys = env::var("PLATFORM_ADMIN_PUBKEYS")
            .unwrap_or_default()
            .split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        let zooid_api_url =
            env::var("ZOOID_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8032".to_string());
        let platform_secret = env::var("PLATFORM_SECRET").unwrap_or_default();
        let relay_domain =
            env::var("RELAY_DOMAIN").unwrap_or_else(|_| "spaces.coracle.social".to_string());
        let platform_nwc_url = env::var("NWC_URL").unwrap_or_default();
        let indexer_relays = env::var("NOSTR_INDEXER_RELAYS")
            .unwrap_or_default()
            .split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        let platform_name = env::var("PLATFORM_NAME").unwrap_or_default();
        let platform_description = env::var("PLATFORM_DESCRIPTION").unwrap_or_default();
        let platform_picture = env::var("PLATFORM_PICTURE").unwrap_or_default();
        let platform_messaging_relays = env::var("PLATFORM_MESSAGING_RELAYS")
            .unwrap_or_default()
            .split(',')
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();

        Self {
            database_url,
            host,
            port,
            admin_pubkeys,
            zooid_api_url,
            platform_secret,
            relay_domain,
            platform_nwc_url,
            indexer_relays,
            platform_name,
            platform_description,
            platform_picture,
            platform_messaging_relays,
        }
    }
}

fn resolve_database_url(database_url: String) -> String {
    const PREFIX: &str = "sqlite://";
    if !database_url.starts_with(PREFIX) {
        return database_url;
    }

    let path = &database_url[PREFIX.len()..];
    if path.is_empty() || path.starts_with('/') || path == ":memory:" {
        return database_url;
    }

    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let absolute = base.join(path);
    let normalized = absolute.to_string_lossy();
    format!("sqlite:///{}", normalized.trim_start_matches('/'))
}
