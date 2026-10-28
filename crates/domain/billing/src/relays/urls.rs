//! The pages a membership is bought, renewed and accepted on.

use super::Relay;
use caravel_util::config::Config;
use caravel_util::error::{Error, Result};

/// `page`, carrying the `return_url` a client asked to be sent back to. The portal
/// only offers it as a link, so it needs no allow-list.
pub fn with_return(page: &str, return_url: Option<&str>) -> Result<String> {
    let mut url = url::Url::parse(page).map_err(Error::internal)?;
    if let Some(back) = return_url.map(str::trim).filter(|url| !url.is_empty()) {
        url.query_pairs_mut().append_pair("return_url", back);
    }
    Ok(url.to_string())
}

/// Where Stripe sends a payer back to: `page`, with the checkout session for it
/// to settle. Stripe fills in the placeholder, so it is appended unencoded.
pub fn checkout_success_url(page: &str) -> String {
    let separator = if page.contains('?') { '&' } else { '?' };
    format!("{page}{separator}checkout={{CHECKOUT_SESSION_ID}}")
}

impl Relay {
    /// A page of the relay's member portal, on its own host.
    pub fn portal_url(&self, path: &str) -> String {
        format!("https://{}{path}", self.host())
    }

    /// Where an invite is accepted: Flotilla's join page for the relay and code.
    pub fn invite_url(&self, config: &Config, code: &str) -> Result<String> {
        let mut url =
            url::Url::parse(&format!("{}/join", config.flotilla_url)).map_err(Error::internal)?;
        url.query_pairs_mut()
            .append_pair("r", &self.host())
            .append_pair("c", code);
        Ok(url.to_string())
    }
}
