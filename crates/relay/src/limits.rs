//! Per-address rate limits: a token bucket for each client IP.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Instant;

/// How many addresses a limiter remembers before forgetting the refilled ones.
const MAX_TRACKED: usize = 10_000;

/// So many actions a minute per address, allowing a minute's worth at once. Zero
/// is no limit.
#[derive(Debug)]
pub struct Limiter {
    per_minute: u32,
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
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
            buckets: Mutex::new(HashMap::new()),
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

        if buckets.len() >= MAX_TRACKED {
            buckets.retain(|_, bucket| {
                bucket.tokens + now.duration_since(bucket.at).as_secs_f64() * per_second < capacity
            });
        }

        let bucket = buckets.entry(ip).or_insert(Bucket {
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
}
