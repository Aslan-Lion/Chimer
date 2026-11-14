//! Sliding-window rate limiting.
//!
//! The same shape Milestone 6 uses on the host: a bucket of timestamps per key,
//! trimmed to a window. Two properties are deliberate and tested:
//!
//! * a **refused** hit is not recorded, so an attacker cannot extend their own
//!   lock-out indefinitely by hammering;
//! * the bucket is small and self-pruning, so a hostile client cannot grow the
//!   relay's memory by inventing keys.

use std::collections::{HashMap, VecDeque};

/// A fixed-limit sliding window keyed by an arbitrary string.
pub struct RateLimiter {
    limit: usize,
    window_ms: i64,
    buckets: HashMap<String, VecDeque<i64>>,
}

impl RateLimiter {
    /// `limit` hits per `window_ms` milliseconds, per key.
    pub fn new(limit: usize, window_ms: i64) -> Self {
        Self {
            limit,
            window_ms,
            buckets: HashMap::new(),
        }
    }

    /// Records a hit for `key` and reports whether it is allowed.
    pub fn check(&mut self, key: &str, now_ms: i64) -> bool {
        let window = self.window_ms;
        let bucket = self.buckets.entry(key.to_string()).or_default();
        while bucket.front().is_some_and(|at| now_ms - at >= window) {
            bucket.pop_front();
        }

        if bucket.len() >= self.limit {
            return false;
        }

        bucket.push_back(now_ms);
        true
    }

    /// Drops the history for one key.
    pub fn forget(&mut self, key: &str) {
        self.buckets.remove(key);
    }

    /// Drops every entry that has aged out.
    ///
    /// Called periodically so the map does not keep one empty bucket for every
    /// address that ever connected.
    pub fn prune(&mut self, now_ms: i64) {
        let window = self.window_ms;
        self.buckets.retain(|_, bucket| {
            while bucket.front().is_some_and(|at| now_ms - at >= window) {
                bucket.pop_front();
            }
            !bucket.is_empty()
        });
    }

    /// How many keys currently hold history.
    ///
    /// Only the pruning test needs to observe the map itself; everything else
    /// is asserted through [`RateLimiter::check`].
    #[cfg(test)]
    pub fn tracked(&self) -> usize {
        self.buckets.len()
    }
}

use crate::logging::now_ms;

/// One limiter per protected surface, built from the configuration.
pub struct Limits {
    /// WebSocket handshakes per source address.
    pub connections: RateLimiter,
    /// Failed registration attempts per source address.
    pub registrations: RateLimiter,
    /// Forwarded application frames per route.
    pub frames: RateLimiter,
}

impl Limits {
    pub fn new(
        connections_per_minute: usize,
        frames_per_second: usize,
    ) -> Limits {
        Limits {
            // A handshake window is a minute; the frame window is a second.
            connections: RateLimiter::new(connections_per_minute, 60_000),
            registrations: RateLimiter::new(connections_per_minute, 60_000),
            frames: RateLimiter::new(frames_per_second, 1_000),
        }
    }

    /// Ages out every bucket. Called from the maintenance task.
    pub fn prune(&mut self) {
        let now = now_ms();
        self.connections.prune(now);
        self.registrations.prune(now);
        self.frames.prune(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_allows_exactly_its_limit() {
        let mut limiter = RateLimiter::new(3, 1_000);
        assert!(limiter.check("a", 0));
        assert!(limiter.check("a", 10));
        assert!(limiter.check("a", 20));
        assert!(!limiter.check("a", 30), "the fourth hit is refused");
    }

    #[test]
    fn a_refused_hit_does_not_extend_the_window() {
        let mut limiter = RateLimiter::new(1, 1_000);
        assert!(limiter.check("a", 0));
        assert!(!limiter.check("a", 500));
        assert!(!limiter.check("a", 900));
        // The single recorded hit has aged out; the bucket is empty again.
        assert!(limiter.check("a", 1_000));
    }

    #[test]
    fn the_window_slides_rather_than_resetting() {
        let mut limiter = RateLimiter::new(2, 1_000);
        assert!(limiter.check("a", 0));
        assert!(limiter.check("a", 600));
        assert!(!limiter.check("a", 900));
        // The first hit expires at 1000, so there is room again.
        assert!(limiter.check("a", 1_000));
        // But only one, because the second hit is still live until 1600.
        assert!(!limiter.check("a", 1_100));
        assert!(limiter.check("a", 1_600));
    }

    #[test]
    fn keys_are_independent() {
        let mut limiter = RateLimiter::new(1, 1_000);
        assert!(limiter.check("a", 0));
        assert!(!limiter.check("a", 0));
        assert!(limiter.check("b", 0), "another address still has its budget");
    }

    #[test]
    fn forgetting_a_key_restores_its_budget() {
        let mut limiter = RateLimiter::new(1, 1_000);
        assert!(limiter.check("a", 0));
        assert!(!limiter.check("a", 0));
        limiter.forget("a");
        assert!(limiter.check("a", 0));
    }

    #[test]
    fn pruning_drops_aged_out_keys_entirely() {
        let mut limiter = RateLimiter::new(5, 1_000);
        limiter.check("a", 0);
        limiter.check("b", 5_000);
        assert_eq!(limiter.tracked(), 2);

        limiter.prune(5_000);
        assert_eq!(limiter.tracked(), 1, "only the live key survives");
        // The surviving key kept its one recorded hit, so exactly four remain.
        assert!(limiter.check("b", 5_000));
        assert!(limiter.check("b", 5_000));
        assert!(limiter.check("b", 5_000));
        assert!(limiter.check("b", 5_000));
        assert!(!limiter.check("b", 5_000), "the fifth recorded hit exhausts it");
    }

    #[test]
    fn the_configured_limits_are_applied_per_surface() {
        let mut limits = Limits::new(2, 1);
        assert!(limits.connections.check("1.2.3.4", 0));
        assert!(limits.connections.check("1.2.3.4", 1));
        assert!(!limits.connections.check("1.2.3.4", 2));

        assert!(limits.frames.check("route-1", 0));
        assert!(!limits.frames.check("route-1", 1));
        assert!(limits.frames.check("route-1", 1_000));
    }
}
