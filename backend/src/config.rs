use std::env;

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
}

impl Config {
    pub fn from_env() -> Self {
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/hosting.db".to_string());
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);
        let admin_pubkeys = env::var("HOSTING_ADMIN_PUBKEYS")
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
        }
    }
}
