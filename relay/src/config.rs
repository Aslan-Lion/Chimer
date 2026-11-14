//! Relay configuration, read from the environment only.
//!
//! Two rules shape this module:
//!
//! * **nothing is hardcoded.** No domain, certificate path, credential or host
//!   identifier has a working default. Every example value in the codebase is a
//!   placeholder the operator must replace.
//! * **nothing dangerous is defaulted on.** The service refuses to start without
//!   TLS material unless it is explicitly asked to run insecurely *and* is bound
//!   to a loopback address. A misconfigured deployment fails loudly at start-up
//!   instead of quietly serving plaintext on a public interface.

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;

use crate::logging::Level;

/// Bind address when TLS is configured.
pub const DEFAULT_BIND: &str = "0.0.0.0:8443";

/// Bind address used when `RELAY_ALLOW_INSECURE=1` and no explicit bind was
/// given. Loopback by construction, because the insecure mode only exists for
/// development on one machine.
pub const DEFAULT_LOOPBACK_BIND: &str = "127.0.0.1:8788";

/// Directory the built remote application is served from.
pub const DEFAULT_WEB_ROOT: &str = "dist";

pub const DEFAULT_MAX_HOSTS: usize = 64;
pub const DEFAULT_MAX_DEVICES_PER_HOST: usize = 32;
pub const DEFAULT_MAX_CONNECTIONS: usize = 512;
/// Largest single WebSocket frame accepted from either leg.
pub const DEFAULT_MAX_FRAME_BYTES: usize = 16 * 1024;
/// Largest forwarded application payload. Matches the host's own
/// `MAX_MESSAGE_BYTES`, so a device cannot use the relay to push something the
/// host would have refused on the LAN.
pub const DEFAULT_MAX_PAYLOAD_BYTES: usize = 8 * 1024;
pub const DEFAULT_DATA_FRAMES_PER_SEC: usize = 40;
pub const DEFAULT_CONNECTIONS_PER_MINUTE: usize = 30;

/// Hard ceiling for `RELAY_MAX_FRAME_BYTES`, so a typo cannot turn the relay
/// into an unbounded buffer.
pub const MAX_FRAME_CEILING: usize = 64 * 1024;

/// Paths to the TLS certificate chain and its private key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlsMaterial {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// Everything the relay needs to run.
#[derive(Clone)]
pub struct Config {
    pub bind: SocketAddr,
    /// `None` means "plaintext, loopback only". See [`Config::parse`].
    pub tls: Option<TlsMaterial>,
    /// Host credentials, `host id -> shared secret`.
    pub hosts: BTreeMap<String, String>,
    pub web_root: PathBuf,
    /// The origin the remote application is served from, used by the
    /// same-origin check on the WebSocket upgrade. `None` means "derive it from
    /// the request".
    pub public_origin: Option<String>,
    /// Extra origins allowed to open a device socket, on top of same-origin.
    pub allowed_origins: Vec<String>,
    pub max_hosts: usize,
    pub max_devices_per_host: usize,
    pub max_connections: usize,
    pub max_frame_bytes: usize,
    pub max_payload_bytes: usize,
    pub data_frames_per_sec: usize,
    pub connections_per_minute: usize,
    pub log_level: Level,
}

/// Never prints a credential. Credentials are compared, never shown.
impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("bind", &self.bind)
            .field("tls", &self.tls.is_some())
            .field("hosts", &self.hosts.keys().collect::<Vec<_>>())
            .field("host_credentials", &format!("<{} redacted>", self.hosts.len()))
            .field("web_root", &self.web_root)
            .field("public_origin", &self.public_origin)
            .field("allowed_origins", &self.allowed_origins)
            .field("max_hosts", &self.max_hosts)
            .field("max_devices_per_host", &self.max_devices_per_host)
            .field("max_connections", &self.max_connections)
            .field("max_frame_bytes", &self.max_frame_bytes)
            .field("max_payload_bytes", &self.max_payload_bytes)
            .field("data_frames_per_sec", &self.data_frames_per_sec)
            .field("connections_per_minute", &self.connections_per_minute)
            .field("log_level", &self.log_level)
            .finish()
    }
}

impl Config {
    /// Reads the process environment.
    pub fn from_env() -> Result<Config, String> {
        Config::parse(std::env::vars().collect::<BTreeMap<_, _>>())
    }

    /// Parses a set of `KEY=value` pairs.
    ///
    /// Split out from [`Config::from_env`] so the whole validation surface is
    /// testable without mutating the process environment.
    pub fn parse(vars: BTreeMap<String, String>) -> Result<Config, String> {
        let insecure = flag(&vars, "RELAY_ALLOW_INSECURE");

        let bind_raw = text(&vars, "RELAY_BIND").unwrap_or_else(|| {
            if insecure {
                DEFAULT_LOOPBACK_BIND.to_string()
            } else {
                DEFAULT_BIND.to_string()
            }
        });
        let bind = SocketAddr::from_str(&bind_raw)
            .map_err(|error| format!("RELAY_BIND '{bind_raw}' is not an address:port pair ({error})"))?;

        let tls = match (
            text(&vars, "RELAY_TLS_CERT"),
            text(&vars, "RELAY_TLS_KEY"),
        ) {
            (Some(cert), Some(key)) => Some(TlsMaterial {
                cert: PathBuf::from(cert),
                key: PathBuf::from(key),
            }),
            (None, None) => None,
            _ => {
                return Err(
                    "RELAY_TLS_CERT and RELAY_TLS_KEY must be set together, or both left unset"
                        .to_string(),
                );
            }
        };

        if tls.is_none() && !insecure {
            return Err(
                "no TLS material configured. Set RELAY_TLS_CERT and RELAY_TLS_KEY, or set \
                 RELAY_ALLOW_INSECURE=1 to run without TLS on a loopback address for development."
                    .to_string(),
            );
        }
        if tls.is_none() && !bind.ip().is_loopback() {
            return Err(format!(
                "RELAY_ALLOW_INSECURE=1 may only be used with a loopback RELAY_BIND; '{bind}' is \
                 reachable from the network. Configure a certificate instead."
            ));
        }

        let hosts = parse_hosts(&vars)?;

        let max_frame_bytes = number(&vars, "RELAY_MAX_FRAME_BYTES", DEFAULT_MAX_FRAME_BYTES)?;
        if max_frame_bytes == 0 || max_frame_bytes > MAX_FRAME_CEILING {
            return Err(format!(
                "RELAY_MAX_FRAME_BYTES must be between 1 and {MAX_FRAME_CEILING}"
            ));
        }

        let max_payload_bytes = number(&vars, "RELAY_MAX_PAYLOAD_BYTES", DEFAULT_MAX_PAYLOAD_BYTES)?;
        if max_payload_bytes == 0 || max_payload_bytes > max_frame_bytes {
            return Err(format!(
                "RELAY_MAX_PAYLOAD_BYTES must be between 1 and RELAY_MAX_FRAME_BYTES \
                 ({max_frame_bytes})"
            ));
        }

        Ok(Config {
            bind,
            tls,
            hosts,
            web_root: PathBuf::from(
                text(&vars, "RELAY_WEB_ROOT").unwrap_or_else(|| DEFAULT_WEB_ROOT.to_string()),
            ),
            public_origin: text(&vars, "RELAY_PUBLIC_ORIGIN"),
            allowed_origins: list(&vars, "RELAY_ALLOWED_ORIGINS"),
            max_hosts: number(&vars, "RELAY_MAX_HOSTS", DEFAULT_MAX_HOSTS)?,
            max_devices_per_host: number(
                &vars,
                "RELAY_MAX_DEVICES_PER_HOST",
                DEFAULT_MAX_DEVICES_PER_HOST,
            )?,
            max_connections: number(&vars, "RELAY_MAX_CONNECTIONS", DEFAULT_MAX_CONNECTIONS)?,
            max_frame_bytes,
            max_payload_bytes,
            data_frames_per_sec: number(
                &vars,
                "RELAY_DATA_FRAMES_PER_SEC",
                DEFAULT_DATA_FRAMES_PER_SEC,
            )?,
            connections_per_minute: number(
                &vars,
                "RELAY_CONN_PER_MIN",
                DEFAULT_CONNECTIONS_PER_MINUTE,
            )?,
            log_level: Level::parse(text(&vars, "RELAY_LOG").as_deref().unwrap_or("info")),
        })
    }

    /// True when the listener must terminate TLS.
    pub fn tls_enabled(&self) -> bool {
        self.tls.is_some()
    }

    /// The credential configured for a host, if any.
    pub fn host_credential(&self, host_id: &str) -> Option<&str> {
        self.hosts.get(host_id).map(String::as_str)
    }

    pub fn knows_host(&self, host_id: &str) -> bool {
        self.hosts.contains_key(host_id)
    }
}

/// Reads `KEY`, treating an empty or whitespace-only value as absent.
fn text(vars: &BTreeMap<String, String>, key: &str) -> Option<String> {
    vars.get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Reads a boolean-ish flag. Only an explicit affirmative turns it on.
fn flag(vars: &BTreeMap<String, String>, key: &str) -> bool {
    match text(vars, key) {
        Some(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        None => false,
    }
}

/// Reads a comma-separated list, ignoring empty entries.
fn list(vars: &BTreeMap<String, String>, key: &str) -> Vec<String> {
    text(vars, key)
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn number(vars: &BTreeMap<String, String>, key: &str, fallback: usize) -> Result<usize, String> {
    match text(vars, key) {
        None => Ok(fallback),
        Some(value) => value
            .parse::<usize>()
            .map_err(|error| format!("{key} '{value}' is not a number ({error})")),
    }
}

/// Parses `RELAY_HOSTS`, a comma-separated list of `host-id:credential` pairs.
///
/// The credential is the shared secret the Windows host presents when it
/// registers. It lives in the environment so the relay keeps nothing on disk.
fn parse_hosts(vars: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    let Some(raw) = text(vars, "RELAY_HOSTS") else {
        return Ok(BTreeMap::new());
    };

    let mut hosts = BTreeMap::new();
    for entry in raw.split(',').map(str::trim).filter(|e| !e.is_empty()) {
        // Split on the *first* colon: a credential may legitimately contain one.
        let Some((host_id, credential)) = entry.split_once(':') else {
            return Err(format!(
                "RELAY_HOSTS entry '{entry}' is not in the form <host-id>:<credential>"
            ));
        };
        let host_id = host_id.trim();
        let credential = credential.trim();

        if !crate::wire::valid_host_id(host_id) {
            return Err(format!(
                "RELAY_HOSTS entry '{entry}' has an invalid host id. Expected the same shape the \
                 desktop application generates: '{}', 8-64 characters of A-Z a-z 0-9 - _.",
                crate::wire::HOST_ID_PREFIX
            ));
        }
        if credential.len() < 16 {
            return Err(format!(
                "the credential for host '{host_id}' is shorter than 16 characters, which is too \
                 weak for a shared secret. Generate one with a password manager or \
                 `openssl rand -base64 32`."
            ));
        }
        if hosts.insert(host_id.to_string(), credential.to_string()).is_some() {
            return Err(format!("host id '{host_id}' appears more than once in RELAY_HOSTS"));
        }
    }
    Ok(hosts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn a_missing_certificate_is_refused_rather_than_defaulted() {
        let error = Config::parse(vars(&[])).unwrap_err();
        assert!(error.contains("no TLS material configured"), "{error}");
    }

    #[test]
    fn only_one_of_the_tls_pair_is_refused() {
        let error = Config::parse(vars(&[("RELAY_TLS_CERT", "/tmp/cert.pem")])).unwrap_err();
        assert!(error.contains("must be set together"), "{error}");
    }

    #[test]
    fn insecure_mode_refuses_a_network_bind() {
        let error = Config::parse(vars(&[
            ("RELAY_ALLOW_INSECURE", "1"),
            ("RELAY_BIND", "0.0.0.0:8443"),
        ]))
        .unwrap_err();
        assert!(error.contains("loopback"), "{error}");
    }

    #[test]
    fn insecure_mode_defaults_to_loopback() {
        let config = Config::parse(vars(&[("RELAY_ALLOW_INSECURE", "1")])).unwrap();
        assert!(config.bind.ip().is_loopback());
        assert!(!config.tls_enabled());
    }

    #[test]
    fn a_certificate_configuration_binds_the_public_default() {
        let config = Config::parse(vars(&[
            ("RELAY_TLS_CERT", "/etc/relay/tls/fullchain.pem"),
            ("RELAY_TLS_KEY", "/etc/relay/tls/privkey.pem"),
        ]))
        .unwrap();
        assert_eq!(config.bind.to_string(), DEFAULT_BIND);
        assert!(config.tls_enabled());
    }

    #[test]
    fn host_credentials_are_parsed_and_validated() {
        let config = Config::parse(vars(&[
            ("RELAY_TLS_CERT", "/c.pem"),
            ("RELAY_TLS_KEY", "/k.pem"),
            (
                "RELAY_HOSTS",
                "h-classroom1:correct-horse-battery-staple, h-office2:another-long-secret-value",
            ),
        ]))
        .unwrap();
        assert_eq!(
            config.host_credential("h-classroom1"),
            Some("correct-horse-battery-staple")
        );
        assert!(config.host_credential("h-unknown").is_none());
        assert!(config.knows_host("h-office2"));
    }

    #[test]
    fn a_credential_containing_a_colon_survives_parsing() {
        let config = Config::parse(vars(&[
            ("RELAY_TLS_CERT", "/c.pem"),
            ("RELAY_TLS_KEY", "/k.pem"),
            ("RELAY_HOSTS", "h-classroom1:part-one:part-two"),
        ]))
        .unwrap();
        assert_eq!(config.host_credential("h-classroom1"), Some("part-one:part-two"));
    }

    #[test]
    fn a_malformed_host_entry_is_refused() {
        for entry in ["no-colon-at-all", "h-ok:short", "x:pretty-long-secret-value-here"] {
            let error = Config::parse(vars(&[
                ("RELAY_TLS_CERT", "/c.pem"),
                ("RELAY_TLS_KEY", "/k.pem"),
                ("RELAY_HOSTS", entry),
            ]))
            .unwrap_err();
            assert!(!error.is_empty(), "{entry} must be refused");
        }
    }

    #[test]
    fn a_duplicated_host_id_is_refused() {
        let error = Config::parse(vars(&[
            ("RELAY_TLS_CERT", "/c.pem"),
            ("RELAY_TLS_KEY", "/k.pem"),
            (
                "RELAY_HOSTS",
                "h-classroom1:correct-horse-battery-staple,h-classroom1:another-long-secret",
            ),
        ]))
        .unwrap_err();
        assert!(error.contains("more than once"), "{error}");
    }

    #[test]
    fn an_absurd_frame_limit_is_refused() {
        let error = Config::parse(vars(&[
            ("RELAY_TLS_CERT", "/c.pem"),
            ("RELAY_TLS_KEY", "/k.pem"),
            ("RELAY_MAX_FRAME_BYTES", "99999999"),
        ]))
        .unwrap_err();
        assert!(error.contains("RELAY_MAX_FRAME_BYTES"), "{error}");
    }

    #[test]
    fn a_payload_limit_above_the_frame_limit_is_refused() {
        let error = Config::parse(vars(&[
            ("RELAY_TLS_CERT", "/c.pem"),
            ("RELAY_TLS_KEY", "/k.pem"),
            ("RELAY_MAX_FRAME_BYTES", "1024"),
            ("RELAY_MAX_PAYLOAD_BYTES", "2048"),
        ]))
        .unwrap_err();
        assert!(error.contains("RELAY_MAX_PAYLOAD_BYTES"), "{error}");
    }

    #[test]
    fn the_default_limits_match_the_host_constraints() {
        let config = Config::parse(vars(&[
            ("RELAY_TLS_CERT", "/c.pem"),
            ("RELAY_TLS_KEY", "/k.pem"),
        ]))
        .unwrap();
        assert_eq!(config.max_payload_bytes, 8 * 1024);
        assert_eq!(config.max_devices_per_host, 32);
    }

    #[test]
    fn debugging_a_config_never_prints_a_credential() {
        let config = Config::parse(vars(&[
            ("RELAY_TLS_CERT", "/c.pem"),
            ("RELAY_TLS_KEY", "/k.pem"),
            ("RELAY_HOSTS", "h-classroom1:correct-horse-battery-staple"),
        ]))
        .unwrap();
        let rendered = format!("{config:?}");
        assert!(
            !rendered.contains("correct-horse-battery-staple"),
            "a credential leaked into Debug: {rendered}"
        );
        assert!(rendered.contains("h-classroom1"), "the host id is not a secret");
    }

    #[test]
    fn the_log_level_is_configurable_and_defaults_to_info() {
        let base = vars(&[("RELAY_TLS_CERT", "/c.pem"), ("RELAY_TLS_KEY", "/k.pem")]);

        let mut quiet = base.clone();
        quiet.insert("RELAY_LOG".to_string(), "warn".to_string());
        assert_eq!(Config::parse(quiet).unwrap().log_level, Level::Warn);

        assert_eq!(Config::parse(base).unwrap().log_level, Level::Info);
    }

    #[test]
    fn allowed_origins_ignore_empty_entries() {
        let mut input = vars(&[("RELAY_TLS_CERT", "/c.pem"), ("RELAY_TLS_KEY", "/k.pem")]);
        input.insert(
            "RELAY_ALLOWED_ORIGINS".to_string(),
            "https://a.example, ,https://b.example".to_string(),
        );
        let config = Config::parse(input).unwrap();
        assert_eq!(
            config.allowed_origins,
            vec!["https://a.example", "https://b.example"]
        );
    }
}
