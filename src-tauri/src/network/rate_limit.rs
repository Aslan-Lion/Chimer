//! Sliding-window rate limiting.
//!
//! Two things are limited: pairing attempts from an address, so a six-digit
//! code cannot be guessed by brute force inside its two-minute life, and the
//! commands one connection may issue, so a broken or hostile client cannot
//! flood the engine.
//!
//! The window slides rather than resetting on a fixed boundary, so a client
//! cannot double its allowance by straddling a tick.

use std::collections::{HashMap, VecDeque};

/// Keeps the timestamps of the hits each key has made inside the window.
#[derive(Debug, Default)]
pub struct RateLimiter {
    buckets: HashMap<String, VecDeque<i64>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a hit for `key` and reports whether it is inside the allowance.
    ///
    /// A rejected hit is *not* recorded: otherwise a client that keeps retrying
    /// would push its own window forward for ever and never be allowed again.
    pub fn check(&mut self, key: &str, limit: usize, window_ms: i64, now: i64) -> bool {
        let hits = self.buckets.entry(key.to_string()).or_default();
        Self::trim(hits, window_ms, now);

        if hits.len() >= limit {
            return false;
        }

        hits.push_back(now);
        true
    }

    /// Hits still available to `key`, without recording anything.
    pub fn remaining(&self, key: &str, limit: usize, window_ms: i64, now: i64) -> usize {
        let mut hits = self.buckets.get(key).cloned().unwrap_or_default();
        Self::trim(&mut hits, window_ms, now);
        limit.saturating_sub(hits.len())
    }

    /// Drops the history of one key, so a successful pairing clears the
    /// failures that preceded it.
    pub fn forget(&mut self, key: &str) {
        self.buckets.remove(key);
    }

    /// Drops every key that has gone quiet, so the map cannot grow without
    /// bound as devices come and go.
    pub fn prune(&mut self, window_ms: i64, now: i64) {
        self.buckets.retain(|_, hits| {
            Self::trim(hits, window_ms, now);
            !hits.is_empty()
        });
    }

    pub fn clear(&mut self) {
        self.buckets.clear();
    }

    fn trim(hits: &mut VecDeque<i64>, window_ms: i64, now: i64) {
        let earliest = now - window_ms;
        while hits.front().is_some_and(|stamp| *stamp <= earliest) {
            hits.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 5;
    const WINDOW: i64 = 60_000;
    const NOW: i64 = 1_700_000_000_000;

    #[test]
    fn the_allowance_is_spent_then_refused() {
        let mut limiter = RateLimiter::new();
        for attempt in 0..LIMIT {
            assert!(
                limiter.check("device", LIMIT, WINDOW, NOW),
                "attempt {attempt} is inside the allowance"
            );
        }
        assert!(
            !limiter.check("device", LIMIT, WINDOW, NOW),
            "the sixth attempt in the same instant is refused"
        );
    }

    #[test]
    fn a_refused_attempt_does_not_extend_the_window() {
        let mut limiter = RateLimiter::new();
        for _ in 0..LIMIT {
            assert!(limiter.check("device", LIMIT, WINDOW, NOW));
        }

        // Hammering while blocked must not push the window forward.
        for _ in 0..100 {
            assert!(!limiter.check("device", LIMIT, WINDOW, NOW + 1_000));
        }

        assert!(
            limiter.check("device", LIMIT, WINDOW, NOW + WINDOW + 1),
            "the original window still expires on its own schedule"
        );
    }

    #[test]
    fn the_window_slides() {
        let window = 5_000;
        let mut limiter = RateLimiter::new();
        for step in 0..LIMIT {
            assert!(limiter.check("device", LIMIT, window, NOW + step as i64 * 1_000));
        }
        // Every hit is still inside the window, so the next one is refused.
        assert!(!limiter.check("device", LIMIT, window, NOW + 4_500));
        // By NOW + 6_000 the earliest hits have fallen out, so room has been made.
        assert!(limiter.check("device", LIMIT, window, NOW + 6_000));
    }

    #[test]
    fn keys_are_independent() {
        let mut limiter = RateLimiter::new();
        for _ in 0..LIMIT {
            assert!(limiter.check("a", LIMIT, WINDOW, NOW));
        }
        assert!(!limiter.check("a", LIMIT, WINDOW, NOW));
        assert!(limiter.check("b", LIMIT, WINDOW, NOW));
    }

    #[test]
    fn forgetting_a_key_restores_the_allowance() {
        let mut limiter = RateLimiter::new();
        for _ in 0..LIMIT {
            limiter.check("device", LIMIT, WINDOW, NOW);
        }
        assert!(!limiter.check("device", LIMIT, WINDOW, NOW));
        limiter.forget("device");
        assert!(limiter.check("device", LIMIT, WINDOW, NOW));
    }

    #[test]
    fn pruning_drops_quiet_keys() {
        let mut limiter = RateLimiter::new();
        limiter.check("old", LIMIT, WINDOW, NOW);
        limiter.check("recent", LIMIT, WINDOW, NOW + WINDOW * 2);
        limiter.prune(WINDOW, NOW + WINDOW * 2);
        assert_eq!(limiter.remaining("old", LIMIT, WINDOW, NOW + WINDOW * 2), LIMIT);
        assert_eq!(limiter.remaining("recent", LIMIT, WINDOW, NOW + WINDOW * 2), LIMIT - 1);
    }
}
