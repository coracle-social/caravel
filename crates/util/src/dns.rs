//! DNS lookups over HTTPS, used to check that a custom domain points at a relay.

use std::time::Duration;

use anyhow::Result;

const DOH_URL: &str = "https://cloudflare-dns.com/dns-query";

/// Whether `domain` has a CNAME to `expected` or resolves to one of its IPv4
/// addresses. The address fallback accepts Cloudflare's CNAME flattening and
/// ALIAS/ANAME records, which resolvers report as A records.
pub async fn domain_points_to(domain: &str, expected: &str) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let expected_name = expected.trim_end_matches('.');
    let cnames = query(&client, domain, "CNAME").await?;
    if cnames.iter().any(|record| {
        record.record_type == 5
            && record
                .data
                .trim_end_matches('.')
                .eq_ignore_ascii_case(expected_name)
    }) {
        return Ok(true);
    }

    let addresses = |records: Vec<Record>| -> Vec<String> {
        records
            .into_iter()
            .filter(|record| record.record_type == 1)
            .map(|record| record.data)
            .collect()
    };

    let domain_ips = addresses(query(&client, domain, "A").await?);
    let expected_ips = addresses(query(&client, expected, "A").await?);

    Ok(domain_ips.iter().any(|ip| expected_ips.contains(ip)))
}

#[derive(serde::Deserialize)]
struct Response {
    #[serde(rename = "Answer")]
    answer: Option<Vec<Record>>,
}

#[derive(serde::Deserialize)]
struct Record {
    #[serde(rename = "type")]
    record_type: u16,
    data: String,
}

async fn query(client: &reqwest::Client, name: &str, record_type: &str) -> Result<Vec<Record>> {
    let response: Response = client
        .get(DOH_URL)
        .query(&[("name", name), ("type", record_type)])
        .header("Accept", "application/dns-json")
        .send()
        .await?
        .json()
        .await?;

    Ok(response.answer.unwrap_or_default())
}
