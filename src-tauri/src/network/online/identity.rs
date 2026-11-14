//! The host's long-term identity key pair (X25519).
//!
//! The host identity is the trust anchor a paired device pins **out of band**,
//! from the pairing QR, before any pairing traffic reaches it
//! (`m7-e2e-security-design.md` §7.1, §7.8). Its private half never leaves this
//! module's protection: it is sealed with the operating system's own
//! data-protection service — Windows DPAPI, **user** scope — before it touches
//! the disk, and it is zeroised the moment the in-memory copy is dropped.
//!
//! Two things this module deliberately does **not** do:
//!
//! * it invents no key-derivation and no wrapping scheme. The key pair is
//!   produced by the X25519 KEM the pairing suite already uses, and the at-rest
//!   wrapping is the platform's `CryptProtectData`.
//! * it never writes the private scalar into `online-settings.json`. That
//!   document is read by the Settings page; only `H_pub` belongs there (§6).
//!
//! # Failure behaviour
//!
//! Loading fails **closed**. If an identity file exists but cannot be read or
//! unsealed, [`load_or_create`] returns an error rather than minting a
//! replacement: silently generating a new `sHost` would replace the anchor every
//! paired device has pinned, and would hide a storage fault behind a key change.
//! A replacement is only ever created when no identity file exists at all.

use std::fs;
use std::path::{Path, PathBuf};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hpke::kem::{Kem as KemTrait, X25519HkdfSha256};
use hpke::{Deserializable as _, Serializable as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// Length of an X25519 private scalar and of an X25519 public key, in bytes.
pub const X25519_KEY_BYTES: usize = 32;

/// Domain-separation label for the display fingerprint (design §8.1).
const FINGERPRINT_LABEL: &[u8] = b"app.timer/e2e/v1/fp";

/// File the DPAPI-sealed private key is written to, beside `online-settings.json`.
pub const HOST_IDENTITY_FILE: &str = "online-host-identity.key";

/// Version tag of the on-disk identity document.
const IDENTITY_VERSION: u32 = 1;

/// The host's long-term identity key pair.
///
/// The public key is derived on demand rather than cached, so there is exactly
/// one copy of the secret in memory and it is owned by this value.
pub struct HostIdentity {
    private: [u8; X25519_KEY_BYTES],
}

impl HostIdentity {
    /// Generates a fresh identity from the operating-system CSPRNG.
    pub fn generate() -> Self {
        let (private, _public) = X25519HkdfSha256::gen_keypair();
        Self {
            private: copy_32(&private.to_bytes()),
        }
    }

    /// Rebuilds an identity from a stored private scalar.
    pub fn from_private(private: [u8; X25519_KEY_BYTES]) -> Self {
        Self { private }
    }

    /// The raw private scalar.
    ///
    /// Used as the Noise `KK` static private key (`local_private_key`) and as
    /// the HPKE recipient key. X25519 clamping is idempotent, so the stored
    /// (already-clamped) bytes are valid for both consumers.
    pub fn private_key(&self) -> &[u8] {
        &self.private
    }

    /// The public key `H_pub`, derived with the same X25519 KEM the suites use.
    pub fn public_key(&self) -> [u8; X25519_KEY_BYTES] {
        let private = <X25519HkdfSha256 as KemTrait>::PrivateKey::from_bytes(&self.private)
            .expect("a host identity is always a 32-byte X25519 scalar");
        let public = X25519HkdfSha256::sk_to_pk(&private);
        copy_32(&public.to_bytes())
    }

    /// The public key in the transport encoding used in the QR and in
    /// `online-settings.json`: base64url without padding.
    pub fn public_key_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.public_key())
    }
}

impl Drop for HostIdentity {
    fn drop(&mut self) {
        self.private.zeroize();
    }
}

/// The human-comparable fingerprint of an X25519 public key (design §8.1).
///
/// `SHA-256(ASCII("app.timer/e2e/v1/fp") ‖ public_key)`, first 16 bytes,
/// rendered as eight groups of four upper-case hex characters. The
/// domain-separation label keeps this value distinct from any other hash of the
/// same key. It is a display aid the operator compares when verifying a pin —
/// never a secret — and both ends compute it identically.
pub fn fingerprint(public_key: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_LABEL);
    hasher.update(public_key);
    let digest = hasher.finalize();

    // 16 bytes render as eight groups of four hex characters, e.g.
    // "1F31 8970 28B0 FF6E 5519 1EA4 49DB 2644".
    let mut out = String::with_capacity(39);
    for (index, byte) in digest[..16].iter().enumerate() {
        if index > 0 && index % 2 == 0 {
            out.push(' ');
        }
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

// ---------------------------------------------------------------------------
// At-rest storage
// ---------------------------------------------------------------------------

/// On-disk shape of the identity document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityDocument {
    version: u32,
    /// base64url (no padding) of the DPAPI-sealed private scalar.
    protected: String,
}

/// Absolute path of the identity file inside an application-data directory.
pub fn resolve_path(directory: &Path) -> PathBuf {
    directory.join(HOST_IDENTITY_FILE)
}

/// Loads the stored identity, or creates and stores a fresh one.
///
/// Returns an error — never a replacement key — when a file exists but cannot
/// be read or unsealed.
pub fn load_or_create(directory: &Path) -> Result<HostIdentity, String> {
    let path = resolve_path(directory);
    if !path.exists() {
        let identity = HostIdentity::generate();
        save(&path, &identity)?;
        return Ok(identity);
    }
    load(&path)
}

/// Reads and unseals the identity file.
pub fn load(path: &Path) -> Result<HostIdentity, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("The host identity file could not be read: {error}"))?;
    let document: IdentityDocument = serde_json::from_str(&raw)
        .map_err(|_| "The host identity file is not a valid document.".to_string())?;
    if document.version != IDENTITY_VERSION {
        return Err("The host identity file was written by a different version.".to_string());
    }
    let sealed = URL_SAFE_NO_PAD
        .decode(document.protected.as_bytes())
        .map_err(|_| "The host identity file is not valid base64url.".to_string())?;

    let mut unsealed = protect::unseal(&sealed)?;
    if unsealed.len() != X25519_KEY_BYTES {
        unsealed.zeroize();
        return Err("The host identity file did not contain an X25519 key.".to_string());
    }
    let mut private = [0u8; X25519_KEY_BYTES];
    private.copy_from_slice(&unsealed);
    unsealed.zeroize();
    Ok(HostIdentity::from_private(private))
}

/// Seals and writes the identity file, using the same temp-file-plus-rename
/// discipline as the rest of the persistence layer.
pub fn save(path: &Path, identity: &HostIdentity) -> Result<(), String> {
    let sealed = protect::seal(identity.private_key())?;
    let document = IdentityDocument {
        version: IDENTITY_VERSION,
        protected: URL_SAFE_NO_PAD.encode(&sealed),
    };
    let payload = serde_json::to_string_pretty(&document)
        .map_err(|error| format!("The host identity could not be encoded: {error}"))?;
    let temporary = path.with_extension("key.tmp");
    fs::write(&temporary, payload)
        .map_err(|error| format!("The host identity file could not be written: {error}"))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("The host identity file could not be replaced: {error}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Platform sealing
// ---------------------------------------------------------------------------

/// Copies a serialised fixed-size array without an intermediate allocation.
fn copy_32(bytes: &impl AsRef<[u8]>) -> [u8; X25519_KEY_BYTES] {
    let mut out = [0u8; X25519_KEY_BYTES];
    out.copy_from_slice(bytes.as_ref());
    out
}

#[cfg(windows)]
mod protect {
    //! Windows DPAPI, user scope.
    //!
    //! `CryptProtectData` with no `CRYPTPROTECT_LOCAL_MACHINE` flag ties the
    //! sealed blob to the *current user*: another account on the same machine
    //! cannot unseal it. The UI prompt is forbidden so a headless host never
    //! blocks, and the description string is a non-secret label.

    use std::ffi::c_void;
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData,
    };

    /// Non-secret label stored alongside the blob.
    const DESCRIPTION: &str = "app.timer host identity";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Seals `plaintext` with DPAPI at user scope.
    pub fn seal(plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };
        let description = wide(DESCRIPTION);

        let ok = unsafe {
            CryptProtectData(
                &input,
                description.as_ptr(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("Windows could not protect the host identity key.".to_string());
        }
        let sealed = unsafe {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        unsafe { LocalFree(output.pbData as *mut c_void) };
        Ok(sealed)
    }

    /// Unseals a blob produced by [`seal`] for the current user.
    pub fn unseal(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: ciphertext.len() as u32,
            pbData: ciphertext.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: ptr::null_mut(),
        };

        let ok = unsafe {
            CryptUnprotectData(
                &mut input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(
                "Windows could not unseal the host identity key for this user.".to_string(),
            );
        }
        let unsealed = unsafe {
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec()
        };
        unsafe { LocalFree(output.pbData as *mut c_void) };
        Ok(unsealed)
    }
}

#[cfg(not(windows))]
mod protect {
    //! No supported non-Windows build exists (§ target platform: Windows 10/11
    //! 64-bit only). Rather than weaken the guarantee with a plaintext store,
    //! this build refuses to seal or unseal at all.

    pub fn seal(_plaintext: &[u8]) -> Result<Vec<u8>, String> {
        Err("Host identity protection is only available on Windows.".to_string())
    }

    pub fn unseal(_ciphertext: &[u8]) -> Result<Vec<u8>, String> {
        Err("Host identity protection is only available on Windows.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "app-timer-online-identity-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch directory");
        path
    }

    #[test]
    fn a_generated_identity_has_a_non_zero_public_key() {
        let identity = HostIdentity::generate();
        assert_eq!(identity.public_key().len(), X25519_KEY_BYTES);
        assert_ne!(identity.public_key(), [0u8; X25519_KEY_BYTES]);
        assert_eq!(identity.private_key().len(), X25519_KEY_BYTES);
    }

    #[test]
    fn a_generated_identity_derives_the_same_public_key_twice() {
        let identity = HostIdentity::generate();
        assert_eq!(identity.public_key(), identity.public_key());
    }

    #[test]
    fn two_generated_identities_differ() {
        let first = HostIdentity::generate();
        let second = HostIdentity::generate();
        assert_ne!(first.public_key(), second.public_key());
        assert_ne!(first.private_key(), second.private_key());
    }

    #[test]
    fn a_fingerprint_is_eight_groups_of_four_hex_characters() {
        let value = fingerprint(&[7u8; X25519_KEY_BYTES]);
        assert_eq!(value.len(), 39, "eight groups of four hex digits plus seven spaces: {value}");
        assert_eq!(value.matches(' ').count(), 7);
        for group in value.split(' ') {
            assert_eq!(group.len(), 4, "each group is four characters: {value}");
            assert!(
                group.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()),
                "each group is upper-case hex: {value}"
            );
        }
    }

    #[test]
    fn a_fingerprint_is_deterministic_and_key_bound() {
        assert_eq!(fingerprint(&[1u8; 32]), fingerprint(&[1u8; 32]));
        assert_ne!(fingerprint(&[1u8; 32]), fingerprint(&[2u8; 32]));
    }

    #[cfg(windows)]
    #[test]
    fn the_sealed_blob_is_not_the_plaintext() {
        let identity = HostIdentity::generate();
        let sealed = protect::seal(identity.private_key()).expect("seal");
        assert_ne!(sealed.as_slice(), identity.private_key());
        // DPAPI output is larger than the input (it adds an integrity header).
        assert!(sealed.len() > X25519_KEY_BYTES);
    }

    #[cfg(windows)]
    #[test]
    fn sealing_round_trips_for_the_current_user() {
        let identity = HostIdentity::generate();
        let sealed = protect::seal(identity.private_key()).expect("seal");
        let unsealed = protect::unseal(&sealed).expect("unseal");
        assert_eq!(unsealed.as_slice(), identity.private_key());
    }

    #[cfg(windows)]
    #[test]
    fn the_stored_document_never_contains_the_raw_private_key() {
        let directory = scratch("stored-document");
        let identity = HostIdentity::generate();
        save(&resolve_path(&directory), &identity).expect("save");

        let raw = fs::read_to_string(resolve_path(&directory)).expect("read");
        let plaintext = URL_SAFE_NO_PAD.encode(identity.private_key());
        assert!(
            !raw.contains(&plaintext),
            "the private scalar appeared in the document in the clear"
        );
        assert!(!raw.contains("privateKey"));
    }

    #[cfg(windows)]
    #[test]
    fn an_identity_survives_a_save_and_load_round_trip() {
        let directory = scratch("round-trip");
        let identity = HostIdentity::generate();
        save(&resolve_path(&directory), &identity).expect("save");

        let loaded = load(&resolve_path(&directory)).expect("load");
        assert_eq!(loaded.private_key(), identity.private_key());
        assert_eq!(loaded.public_key(), identity.public_key());
    }

    #[cfg(windows)]
    #[test]
    fn load_or_create_is_stable_across_calls() {
        let directory = scratch("stable");
        let first = load_or_create(&directory).expect("first");
        let second = load_or_create(&directory).expect("second");
        assert_eq!(first.public_key(), second.public_key());
    }

    #[cfg(windows)]
    #[test]
    fn a_corrupt_identity_file_is_reported_rather_than_replaced() {
        let directory = scratch("corrupt");
        let path = resolve_path(&directory);
        fs::write(&path, "not json at all").expect("write");

        assert!(load_or_create(&directory).is_err());
        // The unusable file is left in place; it was not silently overwritten.
        assert_eq!(fs::read_to_string(&path).expect("read"), "not json at all");
    }

    #[cfg(windows)]
    #[test]
    fn an_unsealed_blob_of_the_wrong_length_is_rejected() {
        let directory = scratch("short");
        let path = resolve_path(&directory);
        // A valid DPAPI blob, but over 16 bytes rather than 32.
        let sealed = protect::seal(&[0u8; 16]).expect("seal");
        let document = serde_json::json!({
            "version": IDENTITY_VERSION,
            "protected": URL_SAFE_NO_PAD.encode(&sealed),
        });
        save_raw(&path, &document.to_string());

        assert!(load(&path).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn an_unknown_document_version_is_rejected() {
        let directory = scratch("version");
        let path = resolve_path(&directory);
        let document = serde_json::json!({
            "version": IDENTITY_VERSION + 1,
            "protected": URL_SAFE_NO_PAD.encode([0u8; 8]),
        });
        save_raw(&path, &document.to_string());

        assert!(load(&path).is_err());
    }

    #[cfg(windows)]
    fn save_raw(path: &Path, contents: &str) {
        fs::write(path, contents).expect("write raw");
    }
}
