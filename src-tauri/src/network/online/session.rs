//! H1 — the Noise `KK` session (design §8 and §9).
//!
//! After pairing, every online connection is carried inside a fresh
//! `Noise_KK_25519_ChaChaPoly_SHA256` session. This module is the host's and the
//! device's shared implementation of that handshake and of the transport that
//! follows it. It is a thin, faithful wrapper around [`snow`]; **no
//! cryptography is written here**. The suite is pinned (§5.2) and never
//! negotiated, and every buffer, nonce and direction is owned by the library
//! (§8.4, §8.5).
//!
//! # The two roles
//!
//! `KK` is a **two-message, pre-message** pattern: each side knows the other's
//! static public key *before* the first message is sent, so the pattern
//! performs **confirmation, not discovery** (§8.2).
//!
//! ```text
//! device (initiator)  -> e, es, ss      // step 1, carries the device id
//! host   (responder)  <- e, ee, se      // step 2
//! ```
//!
//! * The **device** sets `local_private_key = sDev` and
//!   `remote_public_key = H_pub`, pinned out of band from the pairing QR (§7.1).
//! * The **host** sets `local_private_key = sHost` and
//!   `remote_public_key = D_pub`, looked up from the approved device registry for
//!   the `deviceId` in step 1. If the device is unknown, revoked or has no stored
//!   `D_pub`, the host refuses **before** any handshake state is built (§8.1).
//!
//! # Failure behaviour
//!
//! Every failure is reported under a **stable, non-oracular** code (§8.3, §8.6)
//! so a prober cannot tell "unknown device" from "wrong key". A failed read
//! never yields plaintext and the caller tears the connection down.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use snow::params::NoiseParams;

use super::identity::X25519_KEY_BYTES;
use super::pairing::{PAIR_KK_LABEL, SESSION_LABEL};

/// The pinned Noise suite (§5.2). Parsed by the library into its parameter set.
pub const SUITE: &str = "Noise_KK_25519_ChaChaPoly_SHA256";

/// The E2E frame version, shared with the pairing frames (§7.2, §9).
pub const FRAME_VERSION: u8 = 1;

/// Overhead `ChaCha20-Poly1305` adds to a Noise transport message (§8.4).
const AEAD_TAG_BYTES: usize = 16;

/// Room reserved for a handshake message: the `KK` messages are 48 bytes for
/// this suite (§17.3) and the payloads here are empty. This is a fixed,
/// bounded buffer, well under the Noise spec's 64 KiB ceiling.
const HANDSHAKE_HEADROOM: usize = 256;

/// Why a handshake or a transport read failed.
///
/// `code` is stable so the online engine can put it on the wire unchanged in an
/// `hs-err` frame (§8.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// The device's remote-static confirmation failed: the host did not prove
    /// possession of the pinned `H_pub` (§8.3).
    HostKeyMismatch,
    /// Any host-side or generic handshake/tag failure, reported under one
    /// uniform code so the outcome is not oracle-like (§8.6).
    AuthenticationFailed,
    /// A local fault — the suite could not be parsed or a buffer was too small.
    /// Never caused by the peer.
    Internal,
}

impl SessionError {
    /// The stable wire code for this failure (§8.3, §8.6).
    pub fn code(self) -> &'static str {
        match self {
            SessionError::HostKeyMismatch => "host-key-mismatch",
            SessionError::AuthenticationFailed => "authentication-failed",
            SessionError::Internal => "internal",
        }
    }
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            SessionError::HostKeyMismatch => "the host key did not match the pinned key",
            SessionError::AuthenticationFailed => "the session could not be authenticated",
            SessionError::Internal => "the session could not be constructed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SessionError {}

/// The `KK` prologue (§8.1): the label, the host id, a `0x00` separator and the
/// device id.
///
/// The separator stops `("ab", "c")` and `("a", "bc")` colliding, so a
/// transcript from one pair can never be replayed against another.
fn prologue(label: &[u8], host_id: &str, device_id: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(label.len() + host_id.len() + 1 + device_id.len());
    out.extend_from_slice(label);
    out.extend_from_slice(host_id.as_bytes());
    out.push(0x00);
    out.extend_from_slice(device_id.as_bytes());
    out
}

/// The prologue for a **session** connection (§8.1).
pub fn session_prologue(host_id: &str, device_id: &str) -> Vec<u8> {
    prologue(SESSION_LABEL, host_id, device_id)
}

/// The prologue for the **token-delivery** `KK` run right after pairing (§7.6).
pub fn pairing_prologue(host_id: &str, device_id: &str) -> Vec<u8> {
    prologue(PAIR_KK_LABEL, host_id, device_id)
}

/// One side of a `KK` handshake.
///
/// The peer's static key is supplied at construction and never learned from the
/// handshake; [`Handshake::confirm_remote_static`] merely checks that the key
/// actually used equals the one supplied (§8.2).
pub struct Handshake {
    state: snow::HandshakeState,
    remote_public: [u8; X25519_KEY_BYTES],
    initiator: bool,
}

impl Handshake {
    /// The **device** side: initiator, with `sDev` local and the pinned `H_pub`
    /// remote.
    pub fn initiator(
        local_private: &[u8],
        pinned_remote: &[u8],
        prologue: &[u8],
    ) -> Result<Self, SessionError> {
        Self::build(true, local_private, pinned_remote, prologue)
    }

    /// The **host** side: responder, with `sHost` local and the approved `D_pub`
    /// remote.
    pub fn responder(
        local_private: &[u8],
        device_public: &[u8],
        prologue: &[u8],
    ) -> Result<Self, SessionError> {
        Self::build(false, local_private, device_public, prologue)
    }

    fn build(
        initiator: bool,
        local_private: &[u8],
        remote_public: &[u8],
        prologue: &[u8],
    ) -> Result<Self, SessionError> {
        let params: NoiseParams = SUITE.parse().map_err(|_| SessionError::Internal)?;
        let builder = snow::Builder::new(params)
            .local_private_key(local_private)
            .map_err(|_| SessionError::Internal)?
            .remote_public_key(remote_public)
            .map_err(|_| SessionError::Internal)?
            .prologue(prologue)
            .map_err(|_| SessionError::Internal)?;

        let state = if initiator {
            builder.build_initiator()
        } else {
            builder.build_responder()
        }
        .map_err(|_| SessionError::Internal)?;

        Ok(Self {
            state,
            remote_public: copy_32(remote_public),
            initiator,
        })
    }

    /// Whether this side is the initiator (the device).
    pub fn is_initiator(&self) -> bool {
        self.initiator
    }

    /// Whether the handshake is complete and [`Handshake::into_transport`] may
    /// be called.
    pub fn is_finished(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Writes the next handshake message (payload empty in this design).
    pub fn write(&mut self, payload: &[u8]) -> Result<Vec<u8>, SessionError> {
        let mut buffer = vec![0u8; payload.len() + HANDSHAKE_HEADROOM];
        let written = self
            .state
            .write_message(payload, &mut buffer)
            .map_err(|_| SessionError::Internal)?;
        buffer.truncate(written);
        Ok(buffer)
    }

    /// Reads an incoming handshake message. A malformed or tampered message is
    /// reported under the uniform failure code and yields no plaintext (§8.6).
    pub fn read(&mut self, message: &[u8]) -> Result<Vec<u8>, SessionError> {
        let mut buffer = vec![0u8; HANDSHAKE_HEADROOM];
        let read = self
            .state
            .read_message(message, &mut buffer)
            .map_err(|_| SessionError::AuthenticationFailed)?;
        buffer.truncate(read);
        Ok(buffer)
    }

    /// Confirms the peer proved possession of the static key supplied at
    /// construction (§8.2, §8.3).
    ///
    /// In `KK` the key is already known, so this is an assertion, not a
    /// discovery. The device reports `host-key-mismatch`; the host reports the
    /// uniform code.
    pub fn confirm_remote_static(&self) -> Result<(), SessionError> {
        match self.state.get_remote_static() {
            Some(actual) if actual == self.remote_public.as_slice() => Ok(()),
            _ => Err(if self.initiator {
                SessionError::HostKeyMismatch
            } else {
                SessionError::AuthenticationFailed
            }),
        }
    }

    /// The handshake hash, shared by both sides once the handshake completes.
    pub fn handshake_hash(&self) -> Vec<u8> {
        self.state.get_handshake_hash().to_vec()
    }

    /// Splits into an established session (§8.4). Each direction gets an
    /// independent cipher state and nonce counter, owned by the library.
    pub fn into_transport(self) -> Result<Session, SessionError> {
        let initiator = self.initiator;
        let state = self
            .state
            .into_transport_mode()
            .map_err(|_| SessionError::Internal)?;
        Ok(Session { state, initiator })
    }
}

/// An established `KK` session.
///
/// The two directions use independent keys **and** independent counters (§8.4,
/// §8.5), so a frame cannot be reflected into the opposite direction and a
/// replayed or reordered frame fails authentication.
pub struct Session {
    state: snow::TransportState,
    initiator: bool,
}

impl Session {
    /// Whether this side is the initiator (the device).
    pub fn is_initiator(&self) -> bool {
        self.initiator
    }

    /// Seals a plaintext application frame and returns the base64url `ct` for a
    /// `{"e2e":"m",…}` frame (§9).
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<String, SessionError> {
        let mut buffer = vec![0u8; plaintext.len() + AEAD_TAG_BYTES];
        let written = self
            .state
            .write_message(plaintext, &mut buffer)
            .map_err(|_| SessionError::Internal)?;
        buffer.truncate(written);
        Ok(URL_SAFE_NO_PAD.encode(&buffer))
    }

    /// Opens a base64url `ct` from a `{"e2e":"m",…}` frame. Any authentication
    /// failure is reported under the uniform code and yields no plaintext
    /// (§8.6).
    pub fn open(&mut self, ciphertext_b64: &str) -> Result<Vec<u8>, SessionError> {
        let ciphertext = URL_SAFE_NO_PAD
            .decode(ciphertext_b64)
            .map_err(|_| SessionError::AuthenticationFailed)?;
        let mut buffer = vec![0u8; ciphertext.len()];
        let read = self
            .state
            .read_message(&ciphertext, &mut buffer)
            .map_err(|_| SessionError::AuthenticationFailed)?;
        buffer.truncate(read);
        Ok(buffer)
    }
}

/// The three frames a `KK` connection exchanges (§8.1, §9).
///
/// All three are JSON objects, which is the only shape the relay will carry, so
/// the relay needs no change to forward them (§9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "e2e", rename_all = "snake_case")]
pub enum SessionFrame {
    /// A handshake message: step 1 carries the `deviceId`; step 2 does not.
    Hs {
        v: u8,
        step: u8,
        #[serde(rename = "deviceId", skip_serializing_if = "Option::is_none")]
        device_id: Option<String>,
        msg: String,
    },
    /// A terminal, non-oracular handshake failure (§8.6).
    #[serde(rename = "hs-err")]
    HsErr { v: u8, code: String },
    /// An application frame whose `ct` is a Noise transport message (§9).
    M { v: u8, ct: String },
}

impl SessionFrame {
    /// A handshake frame carrying a raw Noise message.
    pub fn handshake(step: u8, device_id: Option<String>, message: &[u8]) -> Self {
        SessionFrame::Hs {
            v: FRAME_VERSION,
            step,
            device_id,
            msg: URL_SAFE_NO_PAD.encode(message),
        }
    }

    /// A terminal failure frame.
    pub fn error(code: &str) -> Self {
        SessionFrame::HsErr {
            v: FRAME_VERSION,
            code: code.to_string(),
        }
    }

    /// An application frame already sealed into `ct`.
    pub fn message(ct: String) -> Self {
        SessionFrame::M {
            v: FRAME_VERSION,
            ct,
        }
    }

    /// The frame's protocol version.
    pub fn version(&self) -> u8 {
        match self {
            SessionFrame::Hs { v, .. } | SessionFrame::HsErr { v, .. } | SessionFrame::M { v, .. } => {
                *v
            }
        }
    }

    /// The raw bytes of a handshake frame's `msg`.
    pub fn handshake_message(&self) -> Result<Vec<u8>, SessionError> {
        match self {
            SessionFrame::Hs { msg, .. } => URL_SAFE_NO_PAD
                .decode(msg)
                .map_err(|_| SessionError::AuthenticationFailed),
            _ => Err(SessionError::AuthenticationFailed),
        }
    }

    /// The raw bytes of an application frame's `ct`.
    pub fn application_message(&self) -> Result<Vec<u8>, SessionError> {
        match self {
            SessionFrame::M { ct, .. } => URL_SAFE_NO_PAD
                .decode(ct)
                .map_err(|_| SessionError::AuthenticationFailed),
            _ => Err(SessionError::AuthenticationFailed),
        }
    }

    /// Serialises the frame for the relay's `data` payload.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parses a frame. A malformed frame is reported under the uniform failure
    /// code rather than as a distinct error (§8.6).
    pub fn parse(text: &str) -> Result<Self, SessionError> {
        serde_json::from_str(text).map_err(|_| SessionError::AuthenticationFailed)
    }
}

/// Copies a 32-byte key without an intermediate allocation.
fn copy_32(bytes: &[u8]) -> [u8; X25519_KEY_BYTES] {
    let mut out = [0u8; X25519_KEY_BYTES];
    out.copy_from_slice(bytes);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use snow::Builder;

    const HOST_ID: &str = "h-abcdefgh";
    const DEVICE_ID: &str = "d-12345678";

    /// A freshly generated X25519 key pair.
    struct KeyPair {
        private: Vec<u8>,
        public: Vec<u8>,
    }

    fn keypair() -> KeyPair {
        let params: NoiseParams = SUITE.parse().expect("the pinned suite parses");
        let pair = Builder::new(params).generate_keypair().expect("a key pair");
        KeyPair {
            private: pair.private,
            public: pair.public,
        }
    }

    fn device_prologue() -> Vec<u8> {
        session_prologue(HOST_ID, DEVICE_ID)
    }

    /// Runs a full handshake and returns both transport sessions.
    fn establish() -> (Session, Session, Vec<u8>, Vec<u8>) {
        let host = keypair();
        let device = keypair();
        let prologue = device_prologue();

        let mut initiator = Handshake::initiator(&device.private, &host.public, &prologue)
            .expect("the device builds an initiator");
        let mut responder = Handshake::responder(&host.private, &device.public, &prologue)
            .expect("the host builds a responder");

        let first = initiator.write(&[]).expect("step 1 is written");
        responder.read(&first).expect("step 1 is accepted");
        responder
            .confirm_remote_static()
            .expect("the device proved its static key");

        let second = responder.write(&[]).expect("step 2 is written");
        initiator.read(&second).expect("step 2 is accepted");
        initiator
            .confirm_remote_static()
            .expect("the host proved the pinned static key");

        assert!(initiator.is_finished(), "the device handshake is complete");
        assert!(responder.is_finished(), "the host handshake is complete");

        let device_hash = initiator.handshake_hash();
        let host_hash = responder.handshake_hash();
        assert_eq!(device_hash, host_hash, "both ends derive the same hash");

        (
            initiator.into_transport().expect("device transport"),
            responder.into_transport().expect("host transport"),
            device_hash,
            host_hash,
        )
    }

    #[test]
    fn the_pinned_suite_is_the_approved_one() {
        assert_eq!(SUITE, "Noise_KK_25519_ChaChaPoly_SHA256");
    }

    #[test]
    fn a_handshake_produces_identical_transcripts_and_bidirectional_messages() {
        let (mut device, mut host, device_hash, host_hash) = establish();
        assert_eq!(device_hash, host_hash);
        assert_eq!(device_hash.len(), 32, "SHA-256 handshake hash");
        assert!(device.is_initiator());
        assert!(!host.is_initiator());

        let sealed = device.seal(br#"{"t":"snapshot"}"#).expect("the device seals");
        let opened = host.open(&sealed).expect("the host opens");
        assert_eq!(opened, br#"{"t":"snapshot"}"#);

        let sealed = host.seal(br#"{"t":"state"}"#).expect("the host seals");
        let opened = device.open(&sealed).expect("the device opens");
        assert_eq!(opened, br#"{"t":"state"}"#);
    }

    #[test]
    fn a_wrong_pinned_host_key_fails_closed() {
        let host = keypair();
        let other = keypair();
        let device = keypair();
        let prologue = device_prologue();

        // The device pinned the wrong host key, so step 1's `es`/`ss` tokens
        // were computed against a key the real host does not hold; the host
        // rejects step 1.
        let mut initiator = Handshake::initiator(&device.private, &other.public, &prologue)
            .expect("the device builds an initiator against the wrong pin");
        let mut responder = Handshake::responder(&host.private, &device.public, &prologue)
            .expect("the host builds a responder");

        let first = initiator.write(&[]).expect("step 1 is written");
        assert!(
            responder.read(&first).is_err(),
            "step 1 bound to a different host key is refused"
        );
    }

    #[test]
    fn a_tampered_handshake_message_is_rejected() {
        let host = keypair();
        let device = keypair();
        let prologue = device_prologue();

        let mut initiator =
            Handshake::initiator(&device.private, &host.public, &prologue).expect("the device");
        let mut responder =
            Handshake::responder(&host.private, &device.public, &prologue).expect("the host");

        let mut first = initiator.write(&[]).expect("step 1 is written");
        first[0] ^= 0x01;
        assert!(responder.read(&first).is_err(), "a flipped bit is rejected");
    }

    #[test]
    fn a_prologue_mismatch_is_rejected() {
        let host = keypair();
        let device = keypair();

        let mut initiator =
            Handshake::initiator(&device.private, &host.public, &session_prologue(HOST_ID, DEVICE_ID))
                .expect("the device");
        let mut responder = Handshake::responder(
            &host.private,
            &device.public,
            &session_prologue("h-other", DEVICE_ID),
        )
        .expect("the host");

        let first = initiator.write(&[]).expect("step 1 is written");
        assert!(
            responder.read(&first).is_err(),
            "a prologue bound to a different host id cannot complete"
        );
    }

    #[test]
    fn a_responder_with_the_wrong_device_key_rejects_step_one() {
        let host = keypair();
        let device = keypair();
        let prologue = device_prologue();

        let mut initiator =
            Handshake::initiator(&device.private, &host.public, &prologue).expect("the device");
        // The host built a responder against a key that is not the device's, so
        // the `ss` token cannot verify and step 1 is refused.
        let mut responder =
            Handshake::responder(&host.private, &keypair().public, &prologue).expect("the host");

        let first = initiator.write(&[]).expect("step 1 is written");
        assert!(
            responder.read(&first).is_err(),
            "the wrong device static key fails closed"
        );
    }

    #[test]
    fn a_replayed_transport_frame_is_rejected() {
        let (mut device, mut host, _, _) = establish();
        let sealed = device.seal(b"once").expect("the device seals");
        assert!(host.open(&sealed).is_ok(), "the first delivery is accepted");
        assert!(host.open(&sealed).is_err(), "the replay is rejected");
    }

    #[test]
    fn a_frame_cannot_be_reflected_into_the_opposite_direction() {
        let (mut device, mut host, _, _) = establish();
        let sealed = device.seal(b"outbound").expect("the device seals");
        assert!(device.open(&sealed).is_err(), "the device is not the receiver");
        assert!(host.open(&sealed).is_ok(), "the host is the receiver");
    }

    #[test]
    fn the_handshake_frames_round_trip_as_json_objects() {
        let frame = SessionFrame::handshake(1, Some(DEVICE_ID.to_string()), &[1, 2, 3]);
        let text = frame.encode();
        assert!(text.starts_with('{'), "the relay requires a JSON object: {text}");
        assert!(text.contains(r#""e2e":"hs""#));
        assert!(text.contains(r#""step":1"#));
        assert!(text.contains(r#""deviceId":"d-12345678""#));
        let parsed = SessionFrame::parse(&text).expect("the frame parses");
        assert_eq!(parsed, frame);
        assert_eq!(parsed.version(), FRAME_VERSION);
        assert_eq!(parsed.handshake_message().expect("bytes"), vec![1, 2, 3]);
    }

    #[test]
    fn the_second_step_omits_the_device_id() {
        let frame = SessionFrame::handshake(2, None, &[4, 5, 6]);
        let text = frame.encode();
        assert!(!text.contains("deviceId"), "step 2 carries no device id: {text}");
    }

    #[test]
    fn the_error_and_message_frames_use_the_designs_tags() {
        let error = SessionFrame::error("authentication-failed").encode();
        assert!(error.contains(r#""e2e":"hs-err""#), "{error}");
        assert!(error.contains(r#""code":"authentication-failed""#), "{error}");

        let message = SessionFrame::message(URL_SAFE_NO_PAD.encode(b"ct")).encode();
        assert!(message.contains(r#""e2e":"m""#), "{message}");
        assert!(message.contains(r#""ct":"#), "{message}");
    }

    #[test]
    fn a_malformed_frame_is_a_uniform_failure() {
        assert_eq!(
            SessionFrame::parse("not json").unwrap_err(),
            SessionError::AuthenticationFailed
        );
        assert_eq!(
            SessionFrame::error("authentication-failed").application_message().unwrap_err(),
            SessionError::AuthenticationFailed
        );
    }

    #[test]
    fn the_prologues_bind_the_host_and_device_ids_and_a_separator() {
        let session = session_prologue("h-ab", "d-c");
        let mut expected = SESSION_LABEL.to_vec();
        expected.extend_from_slice(b"h-ab");
        expected.push(0x00);
        expected.extend_from_slice(b"d-c");
        assert_eq!(session, expected);

        let pairing = pairing_prologue("h-ab", "d-c");
        let mut expected = PAIR_KK_LABEL.to_vec();
        expected.extend_from_slice(b"h-ab");
        expected.push(0x00);
        expected.extend_from_slice(b"d-c");
        assert_eq!(pairing, expected);
        assert_ne!(session, pairing, "the two channels are domain-separated");
    }

    #[test]
    fn the_error_codes_are_the_stable_non_oracular_ones() {
        assert_eq!(SessionError::HostKeyMismatch.code(), "host-key-mismatch");
        assert_eq!(SessionError::AuthenticationFailed.code(), "authentication-failed");
        assert_eq!(SessionError::Internal.code(), "internal");
    }
}
