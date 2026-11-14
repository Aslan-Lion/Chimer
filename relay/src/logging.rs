//! Structured operational logging that cannot leak a secret.
//!
//! Every line is one JSON object on stdout, which is what container and
//! systemd log collectors want. Two defensive measures are built in rather than
//! left to discipline:
//!
//! * fields named after a secret are **dropped**, not printed, even if a caller
//!   passes one by mistake;
//! * the only way to log is through a key/value list, so there is no
//!   `format!("{config:?}")` habit to fall into.
//!
//! What the relay logs is deliberately coarse: connection lifecycle, host ids
//! (which are public — they travel in the pairing QR code), opaque route
//! prefixes, peer addresses, counts and stable error codes. It never logs a
//! pairing code, a device token, a host credential, or the content of any
//! forwarded message.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

/// Log levels, ordered from quietest to loudest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl Level {
    pub fn parse(raw: &str) -> Level {
        match raw.trim().to_ascii_lowercase().as_str() {
            "error" => Level::Error,
            "warn" | "warning" => Level::Warn,
            "debug" | "trace" => Level::Debug,
            _ => Level::Info,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }
}

/// Field names that must never reach the log, whatever a caller passes.
const SECRET_KEY_FRAGMENTS: &[&str] = &[
    "token",
    "code",
    "credential",
    "secret",
    "password",
    "passwd",
    "authorization",
    "payload",
];

static THRESHOLD: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// Applies the configured level. Called once at start-up.
pub fn init(level: Level) {
    THRESHOLD.store(level as u8, Ordering::Relaxed);
}

/// True when a line at this level would be emitted.
pub fn enabled(level: Level) -> bool {
    (level as u8) <= THRESHOLD.load(Ordering::Relaxed)
}

/// Returns `None` for a key that looks like it carries a secret.
fn acceptable(key: &str) -> Option<&str> {
    let lowered = key.to_ascii_lowercase();
    if SECRET_KEY_FRAGMENTS
        .iter()
        .any(|fragment| lowered.contains(fragment))
    {
        return None;
    }
    Some(key)
}

/// Emits one structured line.
pub fn event(level: Level, name: &str, fields: &[(&str, String)]) {
    if !enabled(level) {
        return;
    }

    let mut object = Map::new();
    object.insert("ts".to_string(), Value::String(timestamp(now_ms())));
    object.insert("level".to_string(), Value::String(level.label().to_string()));
    object.insert("event".to_string(), Value::String(name.to_string()));
    for (key, value) in fields {
        if let Some(key) = acceptable(key) {
            object.insert(key.to_string(), Value::String(value.clone()));
        }
    }

    println!("{}", Value::Object(object));
}

/// Convenience wrappers so call sites read as a sentence.
pub fn info(name: &str, fields: &[(&str, String)]) {
    event(Level::Info, name, fields);
}

pub fn warn(name: &str, fields: &[(&str, String)]) {
    event(Level::Warn, name, fields);
}

pub fn error(name: &str, fields: &[(&str, String)]) {
    event(Level::Error, name, fields);
}

pub fn debug(name: &str, fields: &[(&str, String)]) {
    event(Level::Debug, name, fields);
}

/// Current wall-clock milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

/// The first six characters of a route id.
///
/// Enough to correlate two log lines by eye, far too little to address a route.
pub fn route_hint(route: &str) -> String {
    route.chars().take(6).collect()
}

/// Formats epoch milliseconds as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
///
/// Written out rather than pulled from a date crate: the relay needs one
/// timestamp format and nothing else from a calendar library.
fn timestamp(millis: i64) -> String {
    let seconds = millis.div_euclid(1_000);
    let millis_part = millis.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis_part:03}Z"
    )
}

/// Howard Hinnant's `civil_from_days`, valid for the whole proleptic Gregorian
/// range the relay could ever see.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialises the tests that toggle the global threshold.
    static GATE: Mutex<()> = Mutex::new(());

    #[test]
    fn a_secret_shaped_field_name_is_never_written() {
        assert!(acceptable("hostId").is_some());
        assert!(acceptable("route").is_some());
        assert!(acceptable("peer").is_some());

        for rejected in ["hostToken", "pairingCode", "deviceToken", "credential", "payload"] {
            assert!(
                acceptable(rejected).is_none(),
                "{rejected} must never be logged"
            );
        }
    }

    #[test]
    fn the_level_filter_is_ordered() {
        let _gate = GATE.lock().unwrap();
        init(Level::Warn);
        assert!(enabled(Level::Error));
        assert!(enabled(Level::Warn));
        assert!(!enabled(Level::Info));
        assert!(!enabled(Level::Debug));
        init(Level::Info);
        assert!(enabled(Level::Info));
        assert!(!enabled(Level::Debug));
    }

    #[test]
    fn the_level_parser_is_forgiving_and_safe_by_default() {
        assert_eq!(Level::parse("DEBUG"), Level::Debug);
        assert_eq!(Level::parse(" warn "), Level::Warn);
        assert_eq!(Level::parse("error"), Level::Error);
        assert_eq!(Level::parse("nonsense"), Level::Info);
    }

    #[test]
    fn a_route_hint_is_short_enough_to_be_useless_as_a_key() {
        let hint = route_hint("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopq");
        assert_eq!(hint, "ABCDEF");
        assert_eq!(route_hint("ab").len(), 2);
    }

    #[test]
    fn timestamps_render_as_utc() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(timestamp(1_000), "1970-01-01T00:00:01.000Z");
        // 2026-11-14T00:00:00Z
        assert_eq!(timestamp(1_794_614_400_000), "2026-11-14T00:00:00.000Z");
        // A leap day, to prove the calendar maths rather than the easy path.
        assert_eq!(timestamp(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
    }
}
