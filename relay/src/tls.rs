//! TLS termination for the relay's public listener.
//!
//! The relay always plays the role of a TLS **server**: it presents a
//! certificate and a private key, so both legs into it — the Windows host and
//! every remote device — can be `wss://`.
//!
//! Two things are deliberate:
//!
//! * there is no configuration path that weakens verification. This module
//!   only ever presents material; it never accepts a client-supplied
//!   certificate as trusted, and there is no "skip verification" switch
//!   anywhere in the crate;
//! * the files are checked for existence and readability *first*, so a
//!   deployment mistake produces a message naming the bad path instead of an
//!   opaque TLS error at the first handshake.

use std::path::Path;

use axum_server::tls_rustls::RustlsConfig;

use crate::config::TlsMaterial;

/// Builds the rustls configuration for the listener.
///
/// The returned [`RustlsConfig`] is what `axum_server::bind_rustls` wants. It
/// uses rustls' safe default protocol versions (TLS 1.2 and TLS 1.3) and the
/// `ring` crypto provider this crate is built with; neither is configurable,
/// on purpose.
pub async fn load(material: &TlsMaterial) -> Result<RustlsConfig, String> {
    check_readable("certificate", &material.cert)?;
    check_readable("private key", &material.key)?;

    RustlsConfig::from_pem_file(&material.cert, &material.key)
        .await
        .map_err(|error| {
            format!(
                "the TLS material could not be loaded from '{}' and '{}': {error}. The \
                 certificate must be a PEM chain (leaf first) and the key an unencrypted PEM \
                 private key that matches it.",
                material.cert.display(),
                material.key.display()
            )
        })
}

/// Reports a missing or unreadable file before rustls gets a chance to.
fn check_readable(label: &str, path: &Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(format!(
            "the TLS {label} path '{}' exists but is not a file",
            path.display()
        )),
        Err(error) => Err(format!(
            "cannot read the TLS {label} at '{}': {error}",
            path.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn material(cert: &str, key: &str) -> TlsMaterial {
        TlsMaterial {
            cert: PathBuf::from(cert),
            key: PathBuf::from(key),
        }
    }

    #[tokio::test]
    async fn a_missing_certificate_is_reported_by_path() {
        let error = load(&material("does-not-exist/cert.pem", "does-not-exist/key.pem"))
            .await
            .unwrap_err();
        assert!(error.contains("certificate"), "{error}");
        assert!(error.contains("cert.pem"), "{error}");
    }

    #[tokio::test]
    async fn a_missing_key_is_reported_after_the_certificate_passes() {
        // The certificate path is a real file this time, so the failure has to
        // come from the key check rather than from a blanket "nothing exists".
        let directory = std::env::temp_dir();
        let cert = directory.join(format!("app-timer-relay-tls-{}.pem", std::process::id()));
        std::fs::write(&cert, b"not a certificate").unwrap();

        let error = load(&material(
            cert.to_str().unwrap(),
            "does-not-exist/key.pem",
        ))
        .await
        .unwrap_err();
        assert!(error.contains("private key"), "{error}");

        let _ = std::fs::remove_file(&cert);
    }

    #[tokio::test]
    async fn a_directory_is_not_accepted_as_tls_material() {
        let directory = std::env::temp_dir();
        let error = load(&material(
            directory.to_str().unwrap(),
            directory.to_str().unwrap(),
        ))
        .await
        .unwrap_err();
        assert!(error.contains("not a file"), "{error}");
    }
}
