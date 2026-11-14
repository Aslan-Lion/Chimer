//! Secrets used by the pairing workflow, and the registry that tracks the
//! short-lived pairing codes.
//!
//! Every value here comes from the operating system's cryptographic random
//! generator. Nothing is derived from the clock, a counter or a seed, and
//! nothing is ever written to disk or to a log: a token only exists in the
//! hand that was given it, plus — on the host — as a SHA-256 digest.

use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use super::model::{PAIRING_CODE_DIGITS, PAIRING_TTL_MS};

/// Fills a buffer from the OS entropy pool.
///
/// Windows implements this with `BCryptGenRandom`, which cannot fail in
/// practice. It is still reported rather than swallowed: silently issuing a
/// predictable credential would be worse than refusing to pair.
fn random_bytes(length: usize) -> Result<Vec<u8>, String> {
    let mut buffer = vec![0u8; length];
    getrandom::fill(&mut buffer)
        .map_err(|error| format!("The system random number generator failed: {error}"))?;
    Ok(buffer)
}

/// An opaque, URL-safe token with `bytes` bytes of entropy.
pub fn random_token(bytes: usize) -> Result<String, String> {
    Ok(URL_SAFE_NO_PAD.encode(random_bytes(bytes)?))
}

/// A short opaque identifier, safe to display and to store.
pub fn new_id(prefix: &str) -> Result<String, String> {
    Ok(format!("{prefix}-{}", URL_SAFE_NO_PAD.encode(random_bytes(9)?)))
}

/// A decimal pairing code the operator reads off the host and types, or scans.
///
/// Uniformly distributed over its digits: rejection sampling discards the
/// values that would bias a plain `% 10^digits` reduction.
pub fn pairing_code() -> Result<String, String> {
    let limit = 10u32.pow(PAIRING_CODE_DIGITS as u32);
    // Largest multiple of `limit` that fits in a u32; anything above is
    // discarded so every code is equally likely.
    let ceiling = (u32::MAX / limit) * limit;

    loop {
        let bytes = random_bytes(4)?;
        let value = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if value < ceiling {
            return Ok(format!("{:0width$}", value % limit, width = PAIRING_CODE_DIGITS));
        }
    }
}

/// SHA-256 of a token, hex encoded. This is the only form a token is stored in.
pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Comparison that does not stop at the first differing byte.
///
/// The strings compared here are secrets the host issued, so a timing signal
/// that reveals how long a shared prefix is must not be introduced.
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Whether a pairing code belongs to a device that is still waiting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingCodeState {
    /// Issued and shown to the operator; no device has used it yet.
    Open,
    /// A device submitted it and is waiting for the operator's decision. Held
    /// so one code cannot gather several pending requests.
    Claimed(String),
    /// A pairing was completed with it.
    Used,
}

/// One short-lived pairing code.
#[derive(Clone)]
pub struct PairingCode {
    /// Plain code. Held in memory only — never persisted, and deliberately kept
    /// out of the `Debug` output below.
    pub code: String,
    pub expires_at: i64,
    pub state: PairingCodeState,
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PairingCode")
            .field("code", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("state", &self.state)
            .finish()
    }
}

impl PairingCode {
    pub fn expired(&self, now: i64) -> bool {
        now >= self.expires_at
    }
}

/// The host's set of live pairing codes.
///
/// A code is single-use: it is claimed while a request is pending and consumed
/// when the pairing succeeds. If the operator rejects the request the claim is
/// released, so the same still-valid code can be retried — the code itself was
/// never spent on a rejected attempt.
#[derive(Debug, Default)]
pub struct PairingRegistry {
    codes: Vec<PairingCode>,
}

impl PairingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drops every code that has expired.
    pub fn prune(&mut self, now: i64) {
        self.codes.retain(|code| !code.expired(now));
    }

    /// Issues a new code, replacing any previous one.
    ///
    /// Only one code is ever live: showing two at once would let the operator
    /// read out a code the device is not looking at.
    pub fn issue(&mut self, code: String, now: i64) -> String {
        self.codes.clear();
        self.codes.push(PairingCode {
            code: code.clone(),
            expires_at: now + PAIRING_TTL_MS,
            state: PairingCodeState::Open,
        });
        code
    }

    /// The code currently offered to the operator, if one is still open.
    pub fn active(&self, now: i64) -> Option<&PairingCode> {
        self.codes
            .iter()
            .find(|code| !code.expired(now) && code.state == PairingCodeState::Open)
    }

    /// True when a usable code exists (open or claimed by a pending request).
    pub fn live(&self, now: i64) -> bool {
        self.codes
            .iter()
            .any(|code| !code.expired(now) && code.state != PairingCodeState::Used)
    }

    /// Marks the code as being used by a pending request.
    ///
    /// Returns `false` when the code does not match, has expired, is already
    /// spent, or already has a request attached to it.
    pub fn claim(&mut self, candidate: &str, request_id: &str, now: i64) -> bool {
        self.prune(now);
        for code in self.codes.iter_mut() {
            if code.state == PairingCodeState::Open && constant_time_eq(&code.code, candidate) {
                code.state = PairingCodeState::Claimed(request_id.to_string());
                return true;
            }
        }
        false
    }

    /// Releases the claim held by a rejected request.
    pub fn release(&mut self, request_id: &str) {
        for code in self.codes.iter_mut() {
            if code.state == PairingCodeState::Claimed(request_id.to_string()) {
                code.state = PairingCodeState::Open;
            }
        }
    }

    /// Permanently spends the code held by an approved request.
    pub fn consume(&mut self, request_id: &str) {
        for code in self.codes.iter_mut() {
            if code.state == PairingCodeState::Claimed(request_id.to_string()) {
                code.state = PairingCodeState::Used;
            }
        }
    }

    /// Forgets every code. Used when networking is switched off, so a code
    /// cannot outlive the server that issued it.
    pub fn clear(&mut self) {
        self.codes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    #[test]
    fn tokens_are_unique_and_url_safe() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..64 {
            let token = random_token(24).expect("the OS entropy pool is available");
            assert!(token.len() >= 30, "24 bytes encode to at least 32 characters");
            assert!(
                token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
                "a token must survive a URL: {token}"
            );
            assert!(seen.insert(token), "two tokens must never collide");
        }
    }

    #[test]
    fn a_pairing_code_has_the_configured_number_of_digits() {
        for _ in 0..200 {
            let code = pairing_code().expect("the OS entropy pool is available");
            assert_eq!(code.len(), PAIRING_CODE_DIGITS);
            assert!(code.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn hashing_is_stable_and_never_returns_the_token() {
        let digest = hash_token("secret");
        assert_eq!(digest, hash_token("secret"));
        assert_ne!(digest, hash_token("secrez"));
        assert_eq!(digest.len(), 64);
        assert!(!digest.contains("secret"));
    }

    #[test]
    fn constant_time_comparison_matches_equality() {
        assert!(constant_time_eq("123456", "123456"));
        assert!(!constant_time_eq("123456", "123457"));
        assert!(!constant_time_eq("123456", "12345"));
        assert!(!constant_time_eq("", "0"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn a_code_is_single_use() {
        let mut registry = PairingRegistry::new();
        let code = registry.issue("123456".to_string(), NOW);
        assert_eq!(code, "123456");

        assert!(registry.claim("123456", "request-1", NOW));
        // A second device cannot attach to the same code while one is pending.
        assert!(!registry.claim("123456", "request-2", NOW));

        registry.consume("request-1");
        assert!(!registry.claim("123456", "request-3", NOW));
        assert!(registry.active(NOW).is_none());
        assert!(!registry.live(NOW));
    }

    #[test]
    fn a_rejected_request_releases_its_code() {
        let mut registry = PairingRegistry::new();
        registry.issue("654321".to_string(), NOW);
        assert!(registry.claim("654321", "request-1", NOW));

        registry.release("request-1");

        // The operator rejected, so the operator may let the device try again
        // with the same still-valid code.
        assert!(registry.claim("654321", "request-2", NOW));
    }

    #[test]
    fn a_code_expires() {
        let mut registry = PairingRegistry::new();
        registry.issue("111111".to_string(), NOW);
        assert!(registry.active(NOW).is_some());

        let later = NOW + PAIRING_TTL_MS + 1;
        assert!(registry.active(later).is_none());
        assert!(!registry.live(later));
        assert!(!registry.claim("111111", "request-1", later));
    }

    #[test]
    fn issuing_replaces_the_previous_code() {
        let mut registry = PairingRegistry::new();
        registry.issue("111111".to_string(), NOW);
        registry.issue("222222".to_string(), NOW);

        assert!(!registry.claim("111111", "request-1", NOW));
        assert!(registry.claim("222222", "request-2", NOW));
    }

    #[test]
    fn a_wrong_code_is_never_claimed() {
        let mut registry = PairingRegistry::new();
        registry.issue("123456".to_string(), NOW);
        assert!(!registry.claim("999999", "request-1", NOW));
        assert!(registry.active(NOW).is_some(), "the real code is still open");
    }
}
