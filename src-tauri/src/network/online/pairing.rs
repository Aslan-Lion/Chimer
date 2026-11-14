//! H0 — pairing (design §7): the confidential request, the short authentication
//! string, and the pending-request bookkeeping.
//!
//! This is the host half of the pairing step. It performs three things and
//! nothing else:
//!
//! 1. it mints the per-session `(sPair, pkPair)` and renders the QR fragment the
//!    operator shows (§7.1);
//! 2. it opens an HPKE base-mode request, validating the version, the origin and
//!    the payload shape (§7.2);
//! 3. it derives the short authentication string the operator compares, using
//!    HPKE's own `Export` and no hand-written KDF (§7.3).
//!
//! **No cryptography is written here.** The suites are the ones pinned in §5.2:
//! `DHKEM(X25519, HKDF-SHA256)` / `HKDF-SHA256` / `ChaCha20-Poly1305`, base mode.
//! The SAS is `HPKE_Context.Export`, and the `LP` framing below is a length
//! prefix, not a construction.
//!
//! The durable secrets are not kept here: the pairing code is owned by the
//! engine's online [`PairingRegistry`](crate::network::tokens::PairingRegistry)
//! (design §12.5), and the token minted after approval is owned by the device
//! registry. This module owns the ephemeral `pkPair` and the pending requests.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hpke::{
    Deserializable, Kem as KemTrait, OpModeR, Serializable, aead::ChaCha20Poly1305, kdf::HkdfSha256,
    kem::X25519HkdfSha256, setup_receiver,
};
use serde::Deserialize;

use super::identity::{X25519_KEY_BYTES, fingerprint};

/// The HPKE ciphersuite (§5.2). Fixed: nothing negotiates it.
type PairingKem = X25519HkdfSha256;
type PairingKdf = HkdfSha256;
type PairingAead = ChaCha20Poly1305;

/// The suite string pinned in the QR's `alg` field (§7.1). Pinning it stops a
/// future change from being silently substituted: the client compares it.
pub const ALGORITHM: &str =
    "hpke-x25519-hkdfsha256-chacha20poly1305+noise-kk-25519-chachapoly-sha256";

/// HPKE `info` for the pairing request (§7.2).
const PAIR_INFO: &[u8] = b"app.timer/e2e/v1/pair";
/// HPKE `aad` prefix; the host id is appended (§7.2).
const PAIR_AAD_PREFIX: &[u8] = b"app.timer/e2e/v1/pair|";
/// Domain-separation label for the SAS exporter context (§7.3).
const SAS_LABEL: &[u8] = b"app.timer/e2e/v1/sas";
/// The `KK` prologue prefix for the post-approval token channel (§7.6).
pub const PAIR_KK_LABEL: &[u8] = b"app.timer/e2e/v1/pair/kk";
/// The `KK` prologue prefix for the session channel (§8.1).
pub const SESSION_LABEL: &[u8] = b"app.timer/e2e/v1";

/// The version both ends pin on the pairing request and its payload.
pub const PAIRING_VERSION: u8 = 1;
/// The request's own nonce, which binds a SAS to exactly one request (§7.3).
pub const RAND_BYTES: usize = 16;
/// Bytes taken from `Export` for the SAS, before digit conversion (§7.3).
pub const SAS_BYTES: usize = 8;
/// Most pairing requests the host will list at once (§7.5).
pub const MAX_PENDING: usize = 8;

/// Why a pairing request could not be opened.
///
/// `code` is stable so the online engine can put it on the wire unchanged in a
/// `pair-err` frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingError {
    pub code: &'static str,
    pub message: String,
}

impl PairingError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PairingError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PairingError {}

/// The `{"e2e":"pair",...}` object a device sends through the relay (§7.2).
#[derive(Debug, Deserialize)]
struct PairEnvelope {
    e2e: String,
    v: u8,
    /// The encapsulated ephemeral key, b64u.
    enc: String,
    /// The HPKE ciphertext (payload ‖ tag), b64u.
    ct: String,
}

/// The plaintext inside `ct` (§7.2). `code` is present here — and only here —
/// because HPKE is what keeps the relay from reading it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairPayload {
    v: u8,
    code: String,
    device_static_pk: String,
    device_name: String,
    client_origin: String,
    rand: String,
}

/// The host's per-session pairing material and the QR it renders.
///
/// Created when the operator asks for a code and destroyed when that code
/// expires or a request is approved (§7.5). `sPair` lives only in memory and is
/// zeroised when the offer is dropped or replaced.
pub struct PairingOffer {
    host_id: String,
    host_name: String,
    host_pk: [u8; X25519_KEY_BYTES],
    code: String,
    client_origin: String,
    relay: String,
    secret: <PairingKem as KemTrait>::PrivateKey,
    public: [u8; X25519_KEY_BYTES],
    created_at: i64,
    expires_at: i64,
}

impl PairingOffer {
    /// Mints a fresh `(sPair, pkPair)` and binds it to an already-issued code.
    ///
    /// The code is passed in rather than generated here so that the engine's
    /// online registry remains the single owner of the code's lifetime.
    #[allow(clippy::too_many_arguments)]
    pub fn generate(
        host_id: &str,
        host_name: &str,
        host_pk: [u8; X25519_KEY_BYTES],
        code: &str,
        client_origin: &str,
        relay: &str,
        now: i64,
        ttl_ms: i64,
    ) -> PairingOffer {
        let (secret, public) = PairingKem::gen_keypair();
        let mut public_bytes = [0u8; X25519_KEY_BYTES];
        public_bytes.copy_from_slice(public.to_bytes().as_slice());

        PairingOffer {
            host_id: host_id.to_string(),
            host_name: host_name.to_string(),
            host_pk,
            code: code.to_string(),
            client_origin: client_origin.to_string(),
            relay: relay.to_string(),
            secret,
            public: public_bytes,
            created_at: now,
            expires_at: now + ttl_ms,
        }
    }

    /// The public half of `pkPair`, as it goes into the QR.
    pub fn public_key(&self) -> [u8; X25519_KEY_BYTES] {
        self.public
    }

    /// The code this offer belongs to.
    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }

    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }

    pub fn expired(&self, now: i64) -> bool {
        now >= self.expires_at
    }

    /// The exact origin the client must have been served from, compared verbatim
    /// against the `clientOrigin` inside an opened request (§7.3).
    pub fn client_origin(&self) -> &str {
        &self.client_origin
    }

    /// The full URL the QR encodes (§7.1): client origin, then a fragment.
    ///
    /// `name` and `relay` are percent-encoded so that an operator-chosen host
    /// name containing `&` or `#` cannot split the fragment. Every other value
    /// is drawn from a restricted charset (`host`/`code`/`pkPair`/`hostPk` from
    /// base64url and digits, `alg` from a fixed constant) and is emitted as is.
    pub fn qr_url(&self) -> String {
        let base = self.client_origin.trim_end_matches('/');
        format!(
            "{base}/#v={v}&host={host}&name={name}&code={code}&pkPair={pk}&hostPk={h}&relay={relay}&alg={alg}",
            v = PAIRING_VERSION,
            host = self.host_id,
            name = encode_component(&self.host_name),
            code = self.code,
            pk = URL_SAFE_NO_PAD.encode(self.public),
            h = URL_SAFE_NO_PAD.encode(self.host_pk),
            relay = encode_component(&self.relay),
            alg = ALGORITHM,
        )
    }

    /// Opens a pairing request and derives this session's SAS.
    ///
    /// Fails closed on anything but a well-formed, in-window, same-origin
    /// request: a bad version, a payload that will not decrypt, an unexpected
    /// origin or a malformed field are all refusals, never partial acceptance.
    pub fn open(&self, envelope_json: &str, now: i64) -> Result<OpenedRequest, PairingError> {
        if self.expired(now) {
            return Err(PairingError::new(
                "expired",
                "This pairing code has expired.",
            ));
        }

        let envelope: PairEnvelope = serde_json::from_str(envelope_json)
            .map_err(|_| PairingError::new("bad-request", "The pairing request is not valid JSON."))?;
        if envelope.e2e != "pair" {
            return Err(PairingError::new(
                "bad-request",
                "The pairing request is not a pairing request.",
            ));
        }
        if envelope.v != PAIRING_VERSION {
            return Err(PairingError::new(
                "bad-version",
                "The pairing request uses an unsupported version.",
            ));
        }

        let enc = decode_fixed::<X25519_KEY_BYTES>(&envelope.enc, "enc")?;
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ct)
            .map_err(|_| PairingError::new("bad-request", "The pairing payload is not valid base64url."))?;

        let encapped = <PairingKem as KemTrait>::EncappedKey::from_bytes(&enc)
            .map_err(|_| PairingError::new("bad-request", "The encapsulated key is malformed."))?;

        let mut context =
            setup_receiver::<PairingAead, PairingKdf, PairingKem>(&OpModeR::Base, &self.secret, &encapped, PAIR_INFO)
                .map_err(|_| PairingError::new("bad-request", "The pairing request could not be opened."))?;

        let aad = pair_aad(&self.host_id);
        let plaintext = context
            .open(&ciphertext, &aad)
            .map_err(|_| PairingError::new("bad-request", "The pairing request could not be opened."))?;

        let payload: PairPayload = serde_json::from_slice(&plaintext)
            .map_err(|_| PairingError::new("bad-request", "The pairing payload is malformed."))?;
        if payload.v != PAIRING_VERSION {
            return Err(PairingError::new(
                "bad-version",
                "The pairing payload uses an unsupported version.",
            ));
        }

        let device_static_pk = decode_fixed::<X25519_KEY_BYTES>(&payload.device_static_pk, "deviceStaticPk")?;
        let rand = decode_fixed::<RAND_BYTES>(&payload.rand, "rand")?;

        // The origin is compared exactly — scheme, host and port, no
        // normalisation — because it is also sealed inside the SAS input, so a
        // forged origin both fails this check and cannot match the SAS (§7.3).
        if payload.client_origin != self.client_origin {
            return Err(PairingError::new(
                "bad-origin",
                "The pairing request came from an unexpected client origin.",
            ));
        }

        let mut sas_bytes = [0u8; SAS_BYTES];
        context
            .export(
                &exporter_context(
                    &self.host_id,
                    &payload.client_origin,
                    &self.public,
                    &enc,
                    &self.host_pk,
                    &device_static_pk,
                    &rand,
                ),
                &mut sas_bytes,
            )
            .map_err(|_| PairingError::new("bad-request", "The authentication string could not be derived."))?;

        Ok(OpenedRequest {
            enc: envelope.enc,
            enc_bytes: enc,
            device_static_pk,
            device_name: payload.device_name,
            client_origin: payload.client_origin,
            code: payload.code,
            rand,
            sas: sas_from_export(&sas_bytes),
            opened_at: now,
        })
    }
}

/// A request that decrypted correctly and is waiting for the operator (§7.4).
///
/// Its identity is `(enc, device_static_pk)`; approval names that identity, so
/// there is no "approve the next request" operation that a race could exploit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedRequest {
    /// b64u of the encapsulated key. Also the pending map's key, so a replay is
    /// deduplicated by value rather than by arrival order (§7.5).
    pub enc: String,
    /// Raw encapsulated key, kept for the SAS and the post-approval `KK` step.
    enc_bytes: [u8; X25519_KEY_BYTES],
    /// The device's static X25519 public key `D_pub`.
    pub device_static_pk: [u8; X25519_KEY_BYTES],
    pub device_name: String,
    pub client_origin: String,
    /// The code the device sent, checked by the engine against the online
    /// registry. Never trusted for anything beyond that comparison.
    pub code: String,
    /// The request's own nonce, b64u-bound into the SAS.
    pub rand: [u8; RAND_BYTES],
    /// The eight digits the operator compares, grouped `dddd dddd`.
    pub sas: String,
    pub opened_at: i64,
}

impl OpenedRequest {
    /// The raw encapsulated key, for the `KK` channel that delivers the token.
    pub fn enc_bytes(&self) -> &[u8; X25519_KEY_BYTES] {
        &self.enc_bytes
    }

    /// A short, stable fingerprint of `D_pub` for the operator to compare.
    ///
    /// Computed with the shared §8.1 construction, so the value the operator
    /// reads here is the same one the device shows for `H_pub`. It is a display
    /// aid only; the SAS is the value the human decision rests on.
    pub fn device_fingerprint(&self) -> String {
        fingerprint(&self.device_static_pk)
    }
}

#[cfg(test)]
impl OpenedRequest {
    /// An approved request built directly, for the token-delivery tests.
    ///
    /// The token channel binds only to `device_static_pk`, the request `rand`
    /// and the ids in its prologue, so the remaining fields are inert here.
    pub(crate) fn for_test(device_static_pk: [u8; X25519_KEY_BYTES], rand: [u8; RAND_BYTES]) -> Self {
        OpenedRequest {
            enc: URL_SAFE_NO_PAD.encode([0u8; X25519_KEY_BYTES]),
            enc_bytes: [0u8; X25519_KEY_BYTES],
            device_static_pk,
            device_name: "Test device".to_string(),
            client_origin: "https://timer.example".to_string(),
            code: "123456".to_string(),
            rand,
            sas: "0123 4567".to_string(),
            opened_at: 0,
        }
    }
}

/// The host's open pairing requests (design §7.4, §7.5).
#[derive(Debug, Default)]
pub struct PendingPairings {
    entries: Vec<OpenedRequest>,
}

impl PendingPairings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a request, deduplicating by `enc` and refusing past the cap.
    ///
    /// A duplicate is silently dropped so a device that retries its request —
    /// or an attacker replaying a captured one — cannot open a second entry
    /// (§7.5). The cap bounds flooding by a rejected candidate.
    pub fn insert(&mut self, request: OpenedRequest) -> Result<(), PairingError> {
        if self.entries.iter().any(|entry| entry.enc == request.enc) {
            return Ok(());
        }
        if self.entries.len() >= MAX_PENDING {
            return Err(PairingError::new(
                "full",
                format!("There are already {MAX_PENDING} pairing requests waiting."),
            ));
        }
        self.entries.push(request);
        Ok(())
    }

    pub fn list(&self) -> &[OpenedRequest] {
        &self.entries
    }

    pub fn get(&self, enc: &str) -> Option<&OpenedRequest> {
        self.entries.iter().find(|entry| entry.enc == enc)
    }

    /// Removes and returns the request with this `enc`, if it is still open.
    pub fn take(&mut self, enc: &str) -> Option<OpenedRequest> {
        let index = self.entries.iter().position(|entry| entry.enc == enc)?;
        Some(self.entries.remove(index))
    }

    /// Drops every request but the approved one's identity, per §7.5: approving
    /// one candidate permanently discards the rest.
    pub fn retain_only(&mut self, enc: &str) {
        self.entries.retain(|entry| entry.enc == enc);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// HPKE `aad` for a pairing request (§7.2): a label and the host id.
pub fn pair_aad(host_id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(PAIR_AAD_PREFIX.len() + host_id.len());
    aad.extend_from_slice(PAIR_AAD_PREFIX);
    aad.extend_from_slice(host_id.as_bytes());
    aad
}

/// The exporter context for the SAS (§7.3).
///
/// Every variable-length field is length-prefixed exactly as the design
/// specifies, so no two distinct tuples can share an encoding. Both ends build
/// this from the same bytes and pass it to `HPKE_Context.Export`.
pub fn exporter_context(
    host_id: &str,
    client_origin: &str,
    pk_pair: &[u8],
    enc: &[u8],
    host_pk: &[u8],
    device_pk: &[u8],
    rand: &[u8],
) -> Vec<u8> {
    let mut context = Vec::new();
    context.extend_from_slice(SAS_LABEL);
    lp(&mut context, host_id.as_bytes());
    lp(&mut context, client_origin.as_bytes());
    lp(&mut context, pk_pair);
    lp(&mut context, enc);
    lp(&mut context, host_pk);
    lp(&mut context, device_pk);
    lp(&mut context, rand);
    context
}

/// Turns the eight exported bytes into the eight comparison digits (§7.3).
pub fn sas_from_export(bytes: &[u8]) -> String {
    let mut raw = [0u8; SAS_BYTES];
    for (slot, byte) in raw.iter_mut().zip(bytes.iter()) {
        *slot = *byte;
    }
    let value = u64::from_be_bytes(raw) % 100_000_000;
    let digits = format!("{value:08}");
    format!("{} {}", &digits[0..4], &digits[4..8])
}

/// `LP(x) = be32(len(x)) ‖ x` (§7.3).
fn lp(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u32).to_be_bytes());
    out.extend_from_slice(value);
}

fn decode_fixed<const N: usize>(value: &str, what: &str) -> Result<[u8; N], PairingError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| PairingError::new("bad-request", format!("{what} is not valid base64url.")))?;
    if bytes.len() != N {
        return Err(PairingError::new(
            "bad-request",
            format!("{what} must be exactly {N} bytes."),
        ));
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Percent-encodes a fragment component, leaving the RFC 3986 unreserved set.
fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpke::{OpModeS, setup_sender};

    const NOW: i64 = 1_700_000_000_000;
    const TTL: i64 = 120_000;
    const HOST_ID: &str = "h-abcdefgh";
    const ORIGIN: &str = "https://timer.example";
    const CODE: &str = "123456";

    fn host_pk() -> [u8; X25519_KEY_BYTES] {
        [7u8; X25519_KEY_BYTES]
    }

    fn offer() -> PairingOffer {
        PairingOffer::generate(
            HOST_ID,
            "Studio host",
            host_pk(),
            CODE,
            ORIGIN,
            "wss://relay.example/ws/host",
            NOW,
            TTL,
        )
    }

    /// Builds the request exactly as the browser client will: HPKE base mode to
    /// `pkPair`, the payload sealed with the design's `info` and `aad`, and the
    /// sender's own `Export` used to derive the SAS it shows.
    #[allow(clippy::too_many_arguments)]
    fn device_request(
        offer: &PairingOffer,
        device_pk: &[u8; X25519_KEY_BYTES],
        code: &str,
        name: &str,
        origin: &str,
        rand: [u8; RAND_BYTES],
    ) -> (String, String) {
        let recipient =
            <<PairingKem as KemTrait>::PublicKey as Deserializable>::from_bytes(&offer.public_key())
                .expect("the host public key is valid");

        let (encapped, mut context) =
            setup_sender::<PairingAead, PairingKdf, PairingKem>(&OpModeS::Base, &recipient, PAIR_INFO)
                .expect("encapsulation succeeds");

        let payload = serde_json::json!({
            "v": PAIRING_VERSION,
            "code": code,
            "deviceStaticPk": URL_SAFE_NO_PAD.encode(device_pk),
            "deviceName": name,
            "clientOrigin": origin,
            "rand": URL_SAFE_NO_PAD.encode(rand),
        });

        let enc = encapped.to_bytes();
        let ciphertext = context
            .seal(payload.to_string().as_bytes(), &pair_aad(HOST_ID))
            .expect("sealing succeeds");

        let envelope = serde_json::json!({
            "e2e": "pair",
            "v": PAIRING_VERSION,
            "enc": URL_SAFE_NO_PAD.encode(enc.as_slice()),
            "ct": URL_SAFE_NO_PAD.encode(&ciphertext),
        });

        let mut sas_bytes = [0u8; SAS_BYTES];
        context
            .export(
                &exporter_context(
                    HOST_ID,
                    origin,
                    &offer.public_key(),
                    enc.as_slice(),
                    &host_pk(),
                    device_pk,
                    &rand,
                ),
                &mut sas_bytes,
            )
            .expect("export succeeds");

        (envelope.to_string(), sas_from_export(&sas_bytes))
    }

    #[test]
    fn a_request_opens_and_both_sides_derive_the_same_sas() {
        let offer = offer();
        let device_pk = [9u8; X25519_KEY_BYTES];
        let rand = [3u8; RAND_BYTES];
        let (envelope, device_sas) =
            device_request(&offer, &device_pk, CODE, "Ada's iPad", ORIGIN, rand);

        let opened = offer.open(&envelope, NOW).expect("the request opens");

        assert_eq!(opened.device_name, "Ada's iPad");
        assert_eq!(opened.device_static_pk, device_pk);
        assert_eq!(opened.code, CODE);
        assert_eq!(opened.client_origin, ORIGIN);
        assert_eq!(opened.rand, rand);
        assert_eq!(
            opened.sas, device_sas,
            "the host and the device must show the same number"
        );
    }

    #[test]
    fn the_sas_is_eight_digits_in_two_groups() {
        // 12 345 678 is below the modulus, so the conversion is exact: the eight
        // exported bytes are read as a single big-endian integer (§7.3).
        let sas = sas_from_export(&12_345_678u64.to_be_bytes());

        assert_eq!(sas.len(), 9, "eight digits plus the separator");
        assert_eq!(&sas[4..5], " ");
        assert!(
            sas.chars().filter(|c| c.is_ascii_digit()).count() == 8,
            "eight digits: {sas}"
        );
        assert_eq!(sas, "1234 5678");
    }

    #[test]
    fn the_sas_reduces_all_eight_bytes_modulo_the_modulus() {
        // Every one of the eight bytes is significant, not just the low four:
        // 2^32 + 1 reduces to 294 967 297 mod 100 000 000 = 94 967 297.
        let sas = sas_from_export(&4_294_967_297u64.to_be_bytes());
        assert_eq!(sas, "9496 7297");
    }

    #[test]
    fn the_sas_is_bound_to_the_request_nonce() {
        let offer = offer();
        let device_pk = [9u8; X25519_KEY_BYTES];

        let (first, first_sas) =
            device_request(&offer, &device_pk, CODE, "iPad", ORIGIN, [1u8; RAND_BYTES]);
        let (second, second_sas) =
            device_request(&offer, &device_pk, CODE, "iPad", ORIGIN, [2u8; RAND_BYTES]);

        let first = offer.open(&first, NOW).expect("the first request opens");
        let second = offer.open(&second, NOW).expect("the second request opens");

        assert_eq!(first.sas, first_sas);
        assert_eq!(second.sas, second_sas);
        assert_ne!(
            first.sas, second.sas,
            "two requests under one code must not share a value"
        );
    }

    #[test]
    fn a_request_for_another_origin_is_refused() {
        let offer = offer();
        let (envelope, _) = device_request(
            &offer,
            &[9u8; X25519_KEY_BYTES],
            CODE,
            "iPad",
            "https://evil.example",
            [0u8; RAND_BYTES],
        );

        let error = offer.open(&envelope, NOW).expect_err("the origin is refused");
        assert_eq!(error.code, "bad-origin");
    }

    #[test]
    fn a_tampered_ciphertext_is_refused() {
        let offer = offer();
        let (envelope, _) = device_request(
            &offer,
            &[9u8; X25519_KEY_BYTES],
            CODE,
            "iPad",
            ORIGIN,
            [0u8; RAND_BYTES],
        );

        let mut value: serde_json::Value =
            serde_json::from_str(&envelope).expect("the envelope parses");
        let ct = value["ct"].as_str().expect("ct is a string").to_string();
        let mut bytes = URL_SAFE_NO_PAD.decode(&ct).expect("ct decodes");
        bytes[0] ^= 0x01;
        value["ct"] = serde_json::Value::String(URL_SAFE_NO_PAD.encode(&bytes));

        let error = offer
            .open(&value.to_string(), NOW)
            .expect_err("a modified ciphertext is refused");
        assert_eq!(error.code, "bad-request");
    }

    #[test]
    fn an_expired_offer_refuses_requests() {
        let offer = offer();
        let (envelope, _) = device_request(
            &offer,
            &[9u8; X25519_KEY_BYTES],
            CODE,
            "iPad",
            ORIGIN,
            [0u8; RAND_BYTES],
        );

        let error = offer
            .open(&envelope, NOW + TTL + 1)
            .expect_err("the offer has expired");
        assert_eq!(error.code, "expired");
    }

    #[test]
    fn a_request_of_the_wrong_version_is_refused() {
        let offer = offer();
        let mut value: serde_json::Value = serde_json::json!({
            "e2e": "pair",
            "v": 2,
            "enc": URL_SAFE_NO_PAD.encode([0u8; X25519_KEY_BYTES]),
            "ct": URL_SAFE_NO_PAD.encode([0u8; 48]),
        });

        let error = offer
            .open(&value.to_string(), NOW)
            .expect_err("the version is refused");
        assert_eq!(error.code, "bad-version");

        // An `hs` frame must never be mistaken for a pairing request.
        value["e2e"] = serde_json::Value::String("hs".to_string());
        value["v"] = serde_json::Value::from(1u8);
        let error = offer
            .open(&value.to_string(), NOW)
            .expect_err("a non-pair frame is refused");
        assert_eq!(error.code, "bad-request");
    }

    #[test]
    fn a_replayed_request_does_not_create_a_second_entry() {
        let offer = offer();
        let (envelope, _) = device_request(
            &offer,
            &[9u8; X25519_KEY_BYTES],
            CODE,
            "iPad",
            ORIGIN,
            [0u8; RAND_BYTES],
        );
        let opened = offer.open(&envelope, NOW).expect("the request opens");

        let mut pending = PendingPairings::new();
        pending.insert(opened.clone()).expect("the first insert");
        pending.insert(opened).expect("the replay is dropped");

        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn the_pending_list_is_capped() {
        let offer = offer();
        let mut pending = PendingPairings::new();

        for index in 0..MAX_PENDING {
            let mut rand = [0u8; RAND_BYTES];
            rand[0] = index as u8;
            let (envelope, _) =
                device_request(&offer, &[index as u8; X25519_KEY_BYTES], CODE, "iPad", ORIGIN, rand);
            let opened = offer.open(&envelope, NOW).expect("the request opens");
            pending.insert(opened).expect("inside the cap");
        }

        let (envelope, _) = device_request(
            &offer,
            &[99u8; X25519_KEY_BYTES],
            CODE,
            "iPad",
            ORIGIN,
            [200u8; RAND_BYTES],
        );
        let opened = offer.open(&envelope, NOW).expect("the request opens");
        let error = pending.insert(opened).expect_err("the cap is enforced");
        assert_eq!(error.code, "full");
    }

    #[test]
    fn approving_one_request_discards_the_others() {
        let offer = offer();
        let mut pending = PendingPairings::new();

        let (first, _) =
            device_request(&offer, &[1u8; X25519_KEY_BYTES], CODE, "one", ORIGIN, [1u8; RAND_BYTES]);
        let (second, _) =
            device_request(&offer, &[2u8; X25519_KEY_BYTES], CODE, "two", ORIGIN, [2u8; RAND_BYTES]);
        let first = offer.open(&first, NOW).expect("opens");
        let second = offer.open(&second, NOW).expect("opens");
        let kept = first.enc.clone();
        pending.insert(first).expect("insert");
        pending.insert(second).expect("insert");

        pending.retain_only(&kept);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.list()[0].enc, kept);
    }

    #[test]
    fn the_qr_fragment_carries_every_pinned_field() {
        let offer = offer();
        let url = offer.qr_url();

        assert!(url.starts_with("https://timer.example/#v=1"));
        assert!(url.contains("host=h-abcdefgh"));
        assert!(url.contains("code=123456"));
        assert!(url.contains(&format!("pkPair={}", URL_SAFE_NO_PAD.encode(offer.public_key()))));
        assert!(url.contains(&format!("hostPk={}", URL_SAFE_NO_PAD.encode(host_pk()))));
        assert!(url.contains(&format!("alg={ALGORITHM}")));
        // The relay URL is percent-encoded, so its separators stay inside the value.
        assert!(url.contains("relay=wss%3A%2F%2Frelay.example%2Fws%2Fhost"));
        // The code never appears in a query string, only in the fragment.
        assert!(!url.contains('?'));
    }

    #[test]
    fn an_operator_name_with_separators_cannot_split_the_fragment() {
        let offer = PairingOffer::generate(
            HOST_ID,
            "Studio & Kit #2",
            host_pk(),
            CODE,
            ORIGIN,
            "wss://relay.example/ws/host",
            NOW,
            TTL,
        );
        let url = offer.qr_url();
        assert!(url.contains("name=Studio%20%26%20Kit%20%232"));
        assert_eq!(url.matches("&alg=").count(), 1, "the fragment is not split");
    }

    #[test]
    fn the_device_fingerprint_uses_the_designs_eight_group_encoding() {
        let offer = offer();
        let (envelope, _) = device_request(
            &offer,
            &[9u8; X25519_KEY_BYTES],
            CODE,
            "iPad",
            ORIGIN,
            [0u8; RAND_BYTES],
        );
        let opened = offer.open(&envelope, NOW).expect("opens");

        // Design §8.1: 16 bytes of the labelled digest as eight groups of four.
        let fingerprint = opened.device_fingerprint();
        assert_eq!(fingerprint.len(), 39, "eight groups of four hex characters: {fingerprint}");
        assert_eq!(fingerprint.matches(' ').count(), 7);
        assert_eq!(fingerprint, opened.device_fingerprint());
        assert_eq!(fingerprint, crate::network::online::identity::fingerprint(&[9u8; X25519_KEY_BYTES]));
    }

    #[test]
    fn the_lp_prefix_is_four_big_endian_bytes() {
        let mut out = Vec::new();
        lp(&mut out, b"abc");
        assert_eq!(out, vec![0, 0, 0, 3, b'a', b'b', b'c']);
    }
}
