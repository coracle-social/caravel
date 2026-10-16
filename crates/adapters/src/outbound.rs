//! HTTP requests to urls other people chose, such as push callbacks and media to
//! mirror, which must never reach the platform's own network.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect;

/// A client that refuses loopback, private, link-local, unspecified and multicast
/// addresses. It resolves names itself and connects only to the addresses that
/// pass, so a name resolving somewhere else at request time (DNS rebinding) is
/// still refused. `allow_private` is for tests, which call servers on loopback.
fn client(timeout: Duration, allow_private: bool) -> Result<reqwest::Client> {
    let redirects = redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        match check_url(attempt.url(), allow_private) {
            Ok(()) => attempt.follow(),
            Err(error) => attempt.error(error.to_string()),
        }
    });

    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .no_proxy()
        .redirect(redirects)
        .dns_resolver(Arc::new(PublicResolver { allow_private }))
        .build()?)
}

/// Whether a url may be fetched: http or https, and not a refused address when
/// it names one literally, which resolution never sees.
fn check_url(url: &url::Url, allow_private: bool) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("only http and https urls may be fetched");
    }

    let literal = match url.host() {
        Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        Some(url::Host::Domain(_)) => None,
        None => bail!("the url has no host"),
    };

    if let Some(ip) = literal
        && !allow_private
        && is_refused(ip)
    {
        bail!("refusing to reach a local or internal address");
    }

    Ok(())
}

/// Loopback, private, link-local, unspecified and multicast addresses.
fn is_refused(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                // 100.64.0.0/10, carrier-grade NAT
                || (ip.octets()[0] == 100 && (ip.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || (ip.segments()[0] & 0xfe00) == 0xfc00
                || (ip.segments()[0] & 0xffc0) == 0xfe80
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|ip| is_refused(IpAddr::V4(ip)))
        }
    }
}

struct PublicResolver {
    allow_private: bool,
}

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let allow_private = self.allow_private;
        Box::pin(async move {
            let host = format!("{}:0", name.as_str());
            let resolved: Vec<SocketAddr> = tokio::net::lookup_host(host)
                .await?
                .filter(|addr| allow_private || !is_refused(addr.ip()))
                .collect();

            if resolved.is_empty() {
                return Err(anyhow!("{} has no public address", name.as_str()).into());
            }

            Ok(Box::new(resolved.into_iter()) as Addrs)
        })
    }
}

/// Requests to urls other people chose, through a guarded [`client`].
#[derive(Clone)]
pub struct Outbound {
    client: reqwest::Client,
    allow_private: bool,
}

/// What a fetch returned: its content type and bytes.
pub struct Fetched {
    pub content_type: Option<String>,
    pub bytes: bytes::Bytes,
}

impl Outbound {
    pub fn new(timeout: Duration) -> Result<Self> {
        Self::build(timeout, false)
    }

    /// A client that also reaches local addresses, for tests' servers on loopback.
    pub fn allowing_private(timeout: Duration) -> Result<Self> {
        Self::build(timeout, true)
    }

    fn build(timeout: Duration, allow_private: bool) -> Result<Self> {
        Ok(Self {
            client: client(timeout, allow_private)?,
            allow_private,
        })
    }

    /// Whether a url may be reached: [`check_url`] under this client's rules.
    pub fn check(&self, url: &str) -> Result<()> {
        check_url(&url.parse()?, self.allow_private)
    }

    /// POST a JSON body, returning the response's status.
    pub async fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<u16> {
        let url: url::Url = url.parse()?;
        check_url(&url, self.allow_private)?;
        Ok(self
            .client
            .post(url)
            .json(body)
            .send()
            .await?
            .status()
            .as_u16())
    }

    /// GET a url's body, refusing one larger than `max_size` bytes.
    pub async fn fetch(&self, url: &str, max_size: u64) -> Result<Fetched> {
        use futures_util::StreamExt;

        let url: url::Url = url.parse()?;
        check_url(&url, self.allow_private)?;
        let response = self.client.get(url).send().await?.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|size| size > max_size)
        {
            bail!("the file is larger than {max_size} bytes");
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            body.extend_from_slice(&chunk?);
            if body.len() as u64 > max_size {
                bail!("the file is larger than {max_size} bytes");
            }
        }

        Ok(Fetched {
            content_type,
            bytes: body.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_addresses_are_refused() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "169.254.1.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "100.64.0.1",
        ] {
            assert!(is_refused(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "2606:4700::1111"] {
            assert!(!is_refused(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn literal_internal_urls_are_refused_before_any_request() {
        assert!(check_url(&"http://127.0.0.1/x".parse().unwrap(), false).is_err());
        assert!(check_url(&"http://[::1]/x".parse().unwrap(), false).is_err());
        assert!(check_url(&"ftp://example.com/x".parse().unwrap(), false).is_err());
        assert!(check_url(&"https://example.com/x".parse().unwrap(), false).is_ok());
        assert!(check_url(&"http://127.0.0.1/x".parse().unwrap(), true).is_ok());
    }

    #[tokio::test]
    async fn a_name_resolving_to_loopback_is_never_dialed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let outbound = Outbound::new(Duration::from_secs(2)).unwrap();

        let refused = outbound
            .post_json(&format!("http://localhost:{port}/"), &serde_json::json!({}))
            .await;
        assert!(refused.is_err());
    }
}
