//! Per-address rate limits: a token bucket for each client IP.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;
use std::time::Instant;

/// How many addresses a limiter remembers before forgetting the refilled ones.
const MAX_TRACKED: usize = 10_000;

/// So many actions a minute per address, allowing a minute's worth at once. Zero
/// is no limit.
#[derive(Debug)]
pub struct Limiter {
    per_minute: u32,
    buckets: Mutex<Buckets>,
}

#[derive(Debug)]
struct Buckets {
    by_address: HashMap<IpAddr, Bucket>,
    /// How many buckets may be held before the refilled ones are next forgotten:
    /// twice what the last sweep kept, so sweeps stay rare while many are busy.
    sweep_at: usize,
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    at: Instant,
}

impl Limiter {
    pub fn new(per_minute: u32) -> Self {
        Self {
            per_minute,
            buckets: Mutex::new(Buckets {
                by_address: HashMap::new(),
                sweep_at: MAX_TRACKED,
            }),
        }
    }

    /// Spend one action for `ip`, if it has one to spend.
    pub fn allow(&self, ip: IpAddr) -> bool {
        self.allow_at(ip, Instant::now())
    }

    fn allow_at(&self, ip: IpAddr, now: Instant) -> bool {
        if self.per_minute == 0 {
            return true;
        }

        let capacity = f64::from(self.per_minute);
        let per_second = capacity / 60.0;
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        if buckets.by_address.len() >= buckets.sweep_at {
            buckets.by_address.retain(|_, bucket| {
                bucket.tokens + now.duration_since(bucket.at).as_secs_f64() * per_second < capacity
            });
            buckets.sweep_at = MAX_TRACKED.max(buckets.by_address.len() * 2);
        }

        let bucket = buckets.by_address.entry(client(ip)).or_insert(Bucket {
            tokens: capacity,
            at: now,
        });
        bucket.tokens = (bucket.tokens + now.duration_since(bucket.at).as_secs_f64() * per_second)
            .min(capacity);
        bucket.at = now;

        if bucket.tokens < 1.0 {
            return false;
        }

        bucket.tokens -= 1.0;
        true
    }
}

/// The address a client is limited by: an IPv6 address's /64, which one subscriber
/// usually holds whole, and an IPv4-mapped address as IPv4.
fn client(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(ip) => IpAddr::V6(Ipv6Addr::from_bits(ip.to_bits() & !u128::from(u64::MAX))),
        ip => ip,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_burst_spends_a_minute_s_worth_and_then_refills() {
        let limiter = Limiter::new(2);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        let start = Instant::now();

        assert!(limiter.allow_at(ip, start));
        assert!(limiter.allow_at(ip, start));
        assert!(!limiter.allow_at(ip, start));
        assert!(limiter.allow_at(ip, start + Duration::from_secs(30)));
    }

    #[test]
    fn zero_is_no_limit() {
        let limiter = Limiter::new(0);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!((0..1000).all(|_| limiter.allow(ip)));
    }

    #[test]
    fn an_ipv6_client_is_limited_by_its_64() {
        let limiter = Limiter::new(1);
        let start = Instant::now();
        let allow = |ip: &str| limiter.allow_at(ip.parse().unwrap(), start);

        assert!(allow("2001:db8:1:2::1"));
        assert!(!allow("2001:db8:1:2:ffff::9"));
        assert!(allow("2001:db8:1:3::1"));
        assert!(allow("::ffff:192.0.2.1"));
        assert!(!allow("192.0.2.1"));
    }

    #[test]
    fn a_sweep_that_frees_nothing_waits_for_the_map_to_double() {
        let limiter = Limiter::new(1);
        let start = Instant::now();
        let busy = MAX_TRACKED as u32 + 1;
        for n in 0..busy {
            limiter.allow_at(IpAddr::from(n.to_be_bytes()), start);
        }
        let swept_at = limiter.buckets.lock().unwrap().sweep_at;
        assert_eq!(swept_at, 2 * MAX_TRACKED);

        // Past a minute, those buckets have refilled and the next sweep forgets them.
        let later = start + Duration::from_secs(61);
        for n in busy..2 * MAX_TRACKED as u32 + 1 {
            limiter.allow_at(IpAddr::from(n.to_be_bytes()), later);
        }
        assert_eq!(
            limiter.buckets.lock().unwrap().by_address.len(),
            MAX_TRACKED
        );
    }
}
