use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Result;
use regex::Regex;

pub static CUSTOM_DOMAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z0-9][a-z0-9\-]*\.)+[a-z]{2,}$").unwrap());

/// Whether `domain` is one of the platform's zooid domains or sits under one —
/// the hosts we serve ourselves, as opposed to a tenant's custom domain.
pub fn is_zooid_domain(domain: &str) -> bool {
    crate::env::get()
        .zooid_domains
        .iter()
        .any(|zooid_domain| domain == zooid_domain || domain.ends_with(&format!(".{zooid_domain}")))
}

/// Returns true if `domain` either has a CNAME pointing to `expected` or
/// resolves to the same IPv4 addresses. The A-record fallback accepts
/// Cloudflare CNAME flattening and ALIAS/ANAME records, which external
/// resolvers see as A records rather than CNAMEs.
pub async fn domain_points_to(domain: &str, expected: &str) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let cnames = doh_query(&client, domain, "CNAME").await?;
    let expected_norm = expected.trim_end_matches('.');
    for r in &cnames {
        if r.record_type == 5
            && r.data
                .trim_end_matches('.')
                .eq_ignore_ascii_case(expected_norm)
        {
            return Ok(true);
        }
    }

    let domain_ips = doh_query(&client, domain, "A").await?;
    let expected_ips = doh_query(&client, expected, "A").await?;

    let domain_set: Vec<&str> = domain_ips
        .iter()
        .filter(|r| r.record_type == 1)
        .map(|r| r.data.as_str())
        .collect();
    let expected_set: Vec<&str> = expected_ips
        .iter()
        .filter(|r| r.record_type == 1)
        .map(|r| r.data.as_str())
        .collect();

    if domain_set.is_empty() || expected_set.is_empty() {
        return Ok(false);
    }

    Ok(domain_set.iter().any(|ip| expected_set.contains(ip)))
}

#[derive(serde::Deserialize)]
struct DohResponse {
    #[serde(rename = "Answer")]
    answer: Option<Vec<DohRecord>>,
}

#[derive(serde::Deserialize)]
struct DohRecord {
    #[serde(rename = "type")]
    record_type: u16,
    data: String,
}

const DOH_URL: &str = "https://cloudflare-dns.com/dns-query";

async fn doh_query(client: &reqwest::Client, name: &str, qtype: &str) -> Result<Vec<DohRecord>> {
    let resp: DohResponse = client
        .get(DOH_URL)
        .query(&[("name", name), ("type", qtype)])
        .header("Accept", "application/dns-json")
        .send()
        .await?
        .json()
        .await?;
    Ok(resp.answer.unwrap_or_default())
}
