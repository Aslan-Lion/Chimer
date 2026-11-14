//! §7.6 — authenticated token delivery over the pairing `KK` channel.
//!
//! Approval alone gives the device no proof of *who* approved it, and base-mode
//! HPKE cannot supply that. So, on approval, the two sides run a **Noise `KK`
//! handshake** — the same machinery as the session ([`super::session`]) — and the
//! host delivers the freshly minted device token inside it:
//!
//! ```text
//! device (initiator)  -> e, es, ss   // step 1, `deviceId` in the frame
//! host   (responder)  <- e, ee, se   // step 2
//! host   (responder)  -> { "e2e":"m", "ct":… }   // the sealed grant
//! ```
//!
//! * The **device** is the initiator with `rs = H_pub`, pinned from the QR before
//!   any pairing traffic (§7.8).
//! * The **host** is the responder with `is = D_pub`, taken from the *approved*
//!   request — so the host can only ever deliver to the device that proved
//!   possession of the key the operator approved (§7.7).
//! * Prologue = `ASCII("app.timer/e2e/v1/pair/kk") ‖ hostId ‖ 0x00 ‖ deviceId`.
//!
//! **No cryptography is written here.** Both drivers are thin state machines over
//! [`Handshake`] and [`Session`]; the suite, the nonces and the direction ciphers
//! are the library's (§8.4, §8.5). Nothing is hand-composed.
//!
//! # What this module guarantees
//!
//! * the token travels only as Noise transport ciphertext — never in a relay
//!   frame, a log, a diagnostic or an error message:
//!   [`TokenGrant`]'s `Debug` redacts it and every failure is the uniform §8.6
//!   code, which never carries a payload;
//! * the grant is bound to **one exact approved request** (`requestNonce`, the
//!   request's own `rand`) and to **one device identity** (`deviceId` in the
//!   prologue and echoed in the grant);
//! * the host mints nothing until the handshake has completed, and can produce a
//!   grant **once** — a second attempt is refused;
//! * a reconnect is always a **fresh** `KK` run with fresh ephemerals (§10.1):
//!   each `begin` builds new handshake state, and a frame sealed on one run
//!   cannot be opened on another.
//!
//! The device is handed the `deviceId` it must use for the prologue by its caller
//! before this step; the host mints it at approval. Keeping the two ends agreeing
//! on that id, and refusing an unknown or revoked device *before* any responder
//! state is built (§8.3), is the engine seam's job, not this module's.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::pairing::{OpenedRequest, RAND_BYTES};
use super::session::{Handshake, Session, SessionError, SessionFrame, pairing_prologue};
use crate::network::model::{DevicePermission, DeviceRole};

/// The version of the sealed grant payload (§7.6).
pub const GRANT_VERSION: u8 = 1;

/// The §7.6 payload the host seals inside the `KK` channel and the device stores.
///
/// `Debug` is written by hand so the token is never printed: it is the one value
/// on this channel that must not reach a log, a diagnostic or an error (§7.6,
/// §14.4).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenGrant {
    /// Payload version, pinned at [`GRANT_VERSION`].
    pub v: u8,
    /// The identifier the host assigned this device.
    pub device_id: String,
    /// The device token, base64url. **Secret.**
    pub device_token: String,
    /// The role the operator approved.
    pub role: DeviceRole,
    /// The permissions the operator granted.
    pub permissions: Vec<DevicePermission>,
    /// The approved request's `rand`, binding this grant to exactly that request
    /// (§7.3, §7.6).
    pub request_nonce: String,
}

impl TokenGrant {
    /// Builds the grant for one approved request.
    ///
    /// The nonce is taken from the request itself, so a grant can only ever name
    /// the request it was minted for; there is no constructor that lets a caller
    /// bind it to a different one.
    pub fn new(
        device_id: impl Into<String>,
        device_token: impl Into<String>,
        role: DeviceRole,
        permissions: Vec<DevicePermission>,
        request: &OpenedRequest,
    ) -> TokenGrant {
        TokenGrant {
            v: GRANT_VERSION,
            device_id: device_id.into(),
            device_token: device_token.into(),
            role,
            permissions,
            request_nonce: URL_SAFE_NO_PAD.encode(request.rand),
        }
    }
}

impl std::fmt::Debug for TokenGrant {
    /// Prints everything except the token (§7.6).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TokenGrant")
            .field("v", &self.v)
            .field("device_id", &self.device_id)
            .field("device_token", &"[redacted]")
            .field("role", &self.role)
            .field("permissions", &self.permissions)
            .field("request_nonce", &self.request_nonce)
            .finish()
    }
}

/// The **host** half of §7.6.
///
/// Built from the approved request, so the responder is bound to that request's
/// `D_pub` before step 1 arrives (§8.3). Held only for the duration of one
/// approval.
pub struct HostDelivery {
    handshake: Option<Handshake>,
    session: Option<Session>,
    /// The id the prologue and the grant are both bound to.
    device_id: String,
    grant: TokenGrant,
    delivered: bool,
}

impl HostDelivery {
    /// Builds the responder for one approved request.
    ///
    /// Fails closed if the grant does not describe exactly this request: a wrong
    /// version, an empty identifier or token, or a nonce that is not the
    /// request's own `rand` are all refused before any handshake state exists.
    pub fn begin(
        host_private: &[u8],
        host_id: &str,
        request: &OpenedRequest,
        grant: TokenGrant,
    ) -> Result<HostDelivery, SessionError> {
        if grant.v != GRANT_VERSION
            || grant.device_id.is_empty()
            || grant.device_token.is_empty()
            || grant.request_nonce != URL_SAFE_NO_PAD.encode(request.rand)
        {
            return Err(SessionError::Internal);
        }

        let prologue = pairing_prologue(host_id, &grant.device_id);
        let handshake = Handshake::responder(host_private, &request.device_static_pk, &prologue)?;

        Ok(HostDelivery {
            handshake: Some(handshake),
            session: None,
            device_id: grant.device_id.clone(),
            grant,
            delivered: false,
        })
    }

    /// Reads step 1 and answers with step 2.
    ///
    /// A frame that is not step 1, that names another device, or whose `ss` token
    /// does not verify against the approved `D_pub` is refused under the uniform
    /// §8.6 code — so a prober cannot tell "wrong device" from "wrong key".
    pub fn respond(&mut self, frame: &SessionFrame) -> Result<SessionFrame, SessionError> {
        let SessionFrame::Hs { step, device_id, .. } = frame else {
            return Err(SessionError::AuthenticationFailed);
        };
        if *step != 1 || device_id.as_deref() != Some(self.device_id.as_str()) {
            return Err(SessionError::AuthenticationFailed);
        }
        let message = frame.handshake_message()?;

        let step_two = {
            let handshake = self.handshake.as_mut().ok_or(SessionError::Internal)?;
            handshake.read(&message)?;
            // §8.2/§8.3: confirm the peer proved possession of the approved
            // `D_pub`. This is an assertion in `KK`, not a discovery.
            handshake.confirm_remote_static()?;
            handshake.write(&[])?
        };

        let session = self
            .handshake
            .take()
            .ok_or(SessionError::Internal)?
            .into_transport()?;
        self.session = Some(session);

        Ok(SessionFrame::handshake(2, None, &step_two))
    }

    /// Seals the grant onto the completed session and returns the `{"e2e":"m"}`
    /// frame. Can be produced **once**; a second call is refused (§7.6).
    pub fn grant_frame(&mut self) -> Result<SessionFrame, SessionError> {
        if self.delivered {
            return Err(SessionError::Internal);
        }
        let mut session = self.session.take().ok_or(SessionError::Internal)?;

        let mut payload = serde_json::to_vec(&self.grant).map_err(|_| SessionError::Internal)?;
        let sealed = session.seal(&payload);
        // The plaintext held the token; do not leave it on the heap.
        payload.zeroize();
        self.delivered = true;

        Ok(SessionFrame::message(sealed?))
    }

    /// The device id this delivery is bound to.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// The grant describing role, permissions and credentials.
    pub fn grant(&self) -> &TokenGrant {
        &self.grant
    }
}

/// The **device** half of §7.6.
///
/// Built with the pin taken from the QR, so the initiator is bound to `H_pub`
/// before step 1 is written (§8.3).
pub struct DeviceDelivery {
    handshake: Option<Handshake>,
    session: Option<Session>,
    device_id: String,
    request_nonce: String,
    started: bool,
}

impl DeviceDelivery {
    /// Builds the initiator for one delivery attempt.
    ///
    /// `pinned_host_public` is `H_pub` from the QR; `device_id` is the id the
    /// caller gave for the prologue; `request_nonce` is the `rand` this device
    /// sent in its own pairing request, so the grant it accepts must be for that
    /// request and no other.
    pub fn begin(
        device_private: &[u8],
        pinned_host_public: &[u8],
        host_id: &str,
        device_id: &str,
        request_nonce: &[u8; RAND_BYTES],
    ) -> Result<DeviceDelivery, SessionError> {
        let prologue = pairing_prologue(host_id, device_id);
        let handshake = Handshake::initiator(device_private, pinned_host_public, &prologue)?;

        Ok(DeviceDelivery {
            handshake: Some(handshake),
            session: None,
            device_id: device_id.to_string(),
            request_nonce: URL_SAFE_NO_PAD.encode(request_nonce),
            started: false,
        })
    }

    /// Writes step 1, carrying the `deviceId` in the frame (§8.1).
    pub fn step_one(&mut self) -> Result<SessionFrame, SessionError> {
        if self.started {
            return Err(SessionError::Internal);
        }
        let message = {
            let handshake = self.handshake.as_mut().ok_or(SessionError::Internal)?;
            handshake.write(&[])?
        };
        self.started = true;
        Ok(SessionFrame::handshake(1, Some(self.device_id.clone()), &message))
    }

    /// Reads step 2 and confirms the host proved possession of the pinned key.
    pub fn finish(&mut self, frame: &SessionFrame) -> Result<(), SessionError> {
        let SessionFrame::Hs { step, .. } = frame else {
            return Err(SessionError::AuthenticationFailed);
        };
        if *step != 2 {
            return Err(SessionError::AuthenticationFailed);
        }
        let message = frame.handshake_message()?;

        let session = {
            let handshake = self.handshake.as_mut().ok_or(SessionError::Internal)?;
            handshake.read(&message)?;
            // §8.3: the device reports the distinct `host-key-mismatch` if the
            // host did not prove the pinned `H_pub`.
            handshake.confirm_remote_static()?;
            self.handshake
                .take()
                .ok_or(SessionError::Internal)?
                .into_transport()?
        };
        self.session = Some(session);

        Ok(())
    }

    /// Opens the sealed grant and verifies it is for this device and this request.
    ///
    /// Consumes the session, so a replayed frame cannot be opened twice. A
    /// tampered ciphertext, a malformed payload, a different `deviceId` or a
    /// different `requestNonce` are all refused under the uniform §8.6 code.
    pub fn accept_grant(&mut self, frame: &SessionFrame) -> Result<TokenGrant, SessionError> {
        let SessionFrame::M { ct, .. } = frame else {
            return Err(SessionError::AuthenticationFailed);
        };

        let mut session = self.session.take().ok_or(SessionError::Internal)?;
        let mut plaintext = session.open(ct)?;
        let parsed = serde_json::from_slice::<TokenGrant>(&plaintext);
        // The plaintext held the token; drop it deliberately.
        plaintext.zeroize();

        let grant = parsed.map_err(|_| SessionError::AuthenticationFailed)?;
        if grant.v != GRANT_VERSION
            || grant.device_id != self.device_id
            || grant.request_nonce != self.request_nonce
            || grant.device_token.is_empty()
        {
            return Err(SessionError::AuthenticationFailed);
        }

        Ok(grant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snow::params::NoiseParams;

    use super::super::session::SUITE;

    const HOST_ID: &str = "h-abcdefgh";
    const DEVICE_ID: &str = "device-12345678";
    const TOKEN: &str = "3q2-7wAAAAAA-device-token";

    /// A freshly generated X25519 key pair.
    struct Party {
        private: Vec<u8>,
        public: Vec<u8>,
    }

    fn keypair() -> Party {
        let params: NoiseParams = SUITE.parse().expect("the pinned suite parses");
        let pair = snow::Builder::new(params).generate_keypair().expect("a key pair");
        Party {
            private: pair.private,
            public: pair.public,
        }
    }

    fn device_public(party: &Party) -> [u8; 32] {
        let mut out = [0u8; 32];
        out.copy_from_slice(&party.public);
        out
    }

    /// An approved request naming this device key and this nonce.
    fn approved_request(device: &Party, rand: [u8; RAND_BYTES]) -> OpenedRequest {
        OpenedRequest::for_test(device_public(device), rand)
    }

    fn controller_grant(request: &OpenedRequest) -> TokenGrant {
        TokenGrant::new(
            DEVICE_ID,
            TOKEN,
            DeviceRole::Controller,
            DeviceRole::Controller.default_permissions(),
            request,
        )
    }

    /// Runs the whole §7.6 exchange and returns what the device accepted.
    fn deliver(
        host: &Party,
        device: &Party,
        request: &OpenedRequest,
        device_id: &str,
        expected_nonce: [u8; RAND_BYTES],
    ) -> Result<TokenGrant, SessionError> {
        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, request, controller_grant(request))?;
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, device_id, &expected_nonce)?;

        let step_one = device_side.step_one()?;
        let step_two = host_side.respond(&step_one)?;
        device_side.finish(&step_two)?;

        device_side.accept_grant(&host_side.grant_frame()?)
    }

    #[test]
    fn a_device_receives_the_grant_over_an_authenticated_channel() {
        let host = keypair();
        let device = keypair();
        let rand = [3u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let grant = deliver(&host, &device, &request, DEVICE_ID, rand).expect("the grant arrives");

        assert_eq!(grant.v, GRANT_VERSION);
        assert_eq!(grant.device_id, DEVICE_ID);
        assert_eq!(grant.device_token, TOKEN);
        assert_eq!(grant.role, DeviceRole::Controller);
        assert_eq!(grant.permissions, DeviceRole::Controller.default_permissions());
        assert_eq!(grant.request_nonce, URL_SAFE_NO_PAD.encode(rand));
    }

    #[test]
    fn an_attacker_key_cannot_obtain_a_grant() {
        let host = keypair();
        let approved = keypair();
        let attacker = keypair();
        let rand = [4u8; RAND_BYTES];
        let request = approved_request(&approved, rand);

        // The host binds the responder to the *approved* D_pub. The attacker
        // holds a different static key, so its step-1 `ss` cannot verify.
        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut attacker_side =
            DeviceDelivery::begin(&attacker.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the attacker builds");

        let step_one = attacker_side.step_one().expect("the attacker writes step 1");
        assert!(
            host_side.respond(&step_one).is_err(),
            "a peer without the approved key is refused"
        );
        assert!(
            host_side.grant_frame().is_err(),
            "no credential is ever produced for the attacker"
        );
    }

    #[test]
    fn a_host_that_cannot_prove_the_pinned_key_yields_no_grant() {
        let host = keypair();
        let other = keypair();
        let device = keypair();
        let rand = [5u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        // The device pinned a key the real host does not hold, so step 1's `es`
        // and `ss` were computed against the wrong key and the host refuses it.
        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &other.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds against the wrong pin");

        let step_one = device_side.step_one().expect("step 1 is written");
        assert!(
            host_side.respond(&step_one).is_err(),
            "the pinned key cannot be proven"
        );
    }

    #[test]
    fn a_frame_naming_another_device_is_refused() {
        let host = keypair();
        let device = keypair();
        let rand = [6u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let real = device_side.step_one().expect("step 1 is written");
        let message = real.handshake_message().expect("the message decodes");
        let forged = SessionFrame::handshake(1, Some("device-someone".to_string()), &message);

        assert_eq!(
            host_side.respond(&forged).unwrap_err(),
            SessionError::AuthenticationFailed,
            "a step 1 claiming another device id is refused before any crypto runs"
        );
    }

    #[test]
    fn a_tampered_step_one_is_refused() {
        let host = keypair();
        let device = keypair();
        let rand = [7u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1 is written");
        let mut message = step_one.handshake_message().expect("the message decodes");
        message[0] ^= 0x01;
        let tampered = SessionFrame::handshake(1, Some(DEVICE_ID.to_string()), &message);

        assert_eq!(
            host_side.respond(&tampered).unwrap_err(),
            SessionError::AuthenticationFailed
        );
    }

    #[test]
    fn a_tampered_grant_is_refused() {
        let host = keypair();
        let device = keypair();
        let rand = [8u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1");
        let step_two = host_side.respond(&step_one).expect("step 2");
        device_side.finish(&step_two).expect("the device finishes");

        let sealed = host_side.grant_frame().expect("the grant seals");
        let SessionFrame::M { ct, .. } = sealed else {
            panic!("the grant is an application frame");
        };
        let mut bytes = URL_SAFE_NO_PAD.decode(&ct).expect("ct decodes");
        bytes[0] ^= 0x01;
        let tampered = SessionFrame::message(URL_SAFE_NO_PAD.encode(&bytes));

        assert_eq!(
            device_side.accept_grant(&tampered).unwrap_err(),
            SessionError::AuthenticationFailed,
            "a modified ciphertext yields no plaintext"
        );
    }

    #[test]
    fn a_replayed_grant_frame_is_refused() {
        let host = keypair();
        let device = keypair();
        let rand = [9u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1");
        let step_two = host_side.respond(&step_one).expect("step 2");
        device_side.finish(&step_two).expect("the device finishes");
        let sealed = host_side.grant_frame().expect("the grant seals");

        assert!(
            device_side.accept_grant(&sealed).is_ok(),
            "the first delivery is accepted"
        );
        assert!(
            device_side.accept_grant(&sealed).is_err(),
            "the replay is refused"
        );
    }

    #[test]
    fn the_host_can_deliver_only_once() {
        let host = keypair();
        let device = keypair();
        let rand = [10u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1");
        let step_two = host_side.respond(&step_one).expect("step 2");
        device_side.finish(&step_two).expect("the device finishes");

        assert!(host_side.grant_frame().is_ok(), "the first grant is minted");
        assert!(
            host_side.grant_frame().is_err(),
            "the host never delivers a second credential"
        );
    }

    #[test]
    fn nothing_is_minted_before_the_handshake_completes() {
        let host = keypair();
        let device = keypair();
        let rand = [11u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");

        assert_eq!(
            host_side.grant_frame().unwrap_err(),
            SessionError::Internal,
            "there is no session to seal onto yet"
        );
    }

    #[test]
    fn a_grant_bound_to_another_request_is_refused() {
        let host = keypair();
        let device = keypair();
        // The host mints the grant for request A …
        let request_a = approved_request(&device, [1u8; RAND_BYTES]);
        // … but the device is expecting the grant for request B.
        let nonce_b = [2u8; RAND_BYTES];

        let error = deliver(&host, &device, &request_a, DEVICE_ID, nonce_b)
            .expect_err("a grant for another request is refused");
        assert_eq!(error, SessionError::AuthenticationFailed);
    }

    #[test]
    fn a_grant_for_an_empty_token_is_refused() {
        let host = keypair();
        let device = keypair();
        let rand = [12u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let empty = TokenGrant::new(
            DEVICE_ID,
            "",
            DeviceRole::Display,
            Vec::new(),
            &request,
        );
        assert_eq!(
            HostDelivery::begin(&host.private, HOST_ID, &request, empty).err(),
            Some(SessionError::Internal),
            "the host refuses to mint a grant with no token"
        );
    }

    #[test]
    fn a_grant_for_another_nonce_is_refused_by_the_host() {
        let host = keypair();
        let device = keypair();
        let request = approved_request(&device, [13u8; RAND_BYTES]);

        let mut mismatched = controller_grant(&request);
        mismatched.request_nonce = URL_SAFE_NO_PAD.encode([99u8; RAND_BYTES]);

        assert_eq!(
            HostDelivery::begin(&host.private, HOST_ID, &request, mismatched).err(),
            Some(SessionError::Internal)
        );
    }

    /// Runs one complete §7.6 exchange and returns the sealed grant frame.
    fn sealed_grant(host: &Party, device: &Party, request: &OpenedRequest) -> SessionFrame {
        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, request, controller_grant(request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &request.rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1");
        let step_two = host_side.respond(&step_one).expect("step 2");
        device_side.finish(&step_two).expect("the device finishes");

        host_side.grant_frame().expect("the grant seals")
    }

    #[test]
    fn a_reconnect_establishes_a_fresh_session() {
        let host = keypair();
        let device = keypair();
        let rand = [14u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        // Run 1: a complete delivery over one `KK` session.
        let first_sealed = sealed_grant(&host, &device, &request);

        // Run 2: an independent `KK` session, with its own fresh ephemerals.
        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");
        let step_one = device_side.step_one().expect("step 1");
        let step_two = host_side.respond(&step_one).expect("step 2");
        device_side.finish(&step_two).expect("the device finishes");

        // A frame sealed on run 1 cannot be opened on run 2: the two runs share
        // no key material.
        assert!(
            device_side.accept_grant(&first_sealed).is_err(),
            "a frame from another session cannot be opened"
        );

        // A later run still completes normally, so the reconnect left nothing
        // stale behind.
        let grant = deliver(&host, &device, &request, DEVICE_ID, rand).expect("a later run succeeds");
        assert_eq!(grant.device_token, TOKEN);
    }

    #[test]
    fn an_interrupted_handshake_abandons_cleanly_and_can_restart() {
        let host = keypair();
        let device = keypair();
        let rand = [15u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        // First attempt: the device's step 1 never reaches a valid responder.
        let mut abandoned =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        assert!(abandoned.respond(&SessionFrame::message("junk".to_string())).is_err());
        assert!(
            abandoned.grant_frame().is_err(),
            "an interrupted handshake produces no credential"
        );
        drop(abandoned);

        // A fresh attempt succeeds, so the abandonment left nothing stale behind.
        let grant = deliver(&host, &device, &request, DEVICE_ID, rand).expect("the retry succeeds");
        assert_eq!(grant.device_token, TOKEN);
    }

    #[test]
    fn a_non_application_frame_cannot_carry_a_grant() {
        let host = keypair();
        let device = keypair();
        let rand = [16u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1");
        let step_two = host_side.respond(&step_one).expect("step 2");
        device_side.finish(&step_two).expect("the device finishes");

        assert_eq!(
            device_side.accept_grant(&step_two).unwrap_err(),
            SessionError::AuthenticationFailed,
            "only a sealed application frame may carry the grant"
        );
    }

    #[test]
    fn the_sealed_frame_never_contains_the_token() {
        let host = keypair();
        let device = keypair();
        let rand = [17u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1");
        let step_two = host_side.respond(&step_one).expect("step 2");
        device_side.finish(&step_two).expect("the device finishes");
        let sealed = host_side.grant_frame().expect("the grant seals");

        let text = sealed.encode();
        assert!(text.starts_with('{'), "the relay requires a JSON object: {text}");
        assert!(!text.contains(TOKEN), "the token must not appear in any frame: {text}");
    }

    #[test]
    fn the_grant_debug_output_redacts_the_token() {
        let device = keypair();
        let request = approved_request(&device, [18u8; RAND_BYTES]);
        let grant = controller_grant(&request);

        let debug = format!("{grant:?}");
        assert!(!debug.contains(TOKEN), "the token must not reach a log: {debug}");
        assert!(debug.contains("[redacted]"));
        assert!(debug.contains(DEVICE_ID));
    }

    #[test]
    fn the_encoded_grant_uses_the_designs_field_names() {
        let device = keypair();
        let request = approved_request(&device, [19u8; RAND_BYTES]);
        let json = serde_json::to_string(&controller_grant(&request)).expect("the grant serialises");

        assert!(json.contains(r#""v":1"#), "{json}");
        assert!(json.contains(r#""deviceId":"device-12345678""#), "{json}");
        assert!(json.contains(r#""deviceToken":"#), "{json}");
        assert!(json.contains(r#""role":"controller""#), "{json}");
        assert!(json.contains(r#""permissions":["timerStart""#), "{json}");
        assert!(json.contains(r#""requestNonce":"#), "{json}");

        let parsed: TokenGrant = serde_json::from_str(&json).expect("the grant parses");
        assert_eq!(parsed, controller_grant(&request));
    }

    #[test]
    fn a_display_role_carries_no_permissions() {
        let device = keypair();
        let request = approved_request(&device, [20u8; RAND_BYTES]);
        let grant = TokenGrant::new(
            DEVICE_ID,
            TOKEN,
            DeviceRole::Display,
            DeviceRole::Display.default_permissions(),
            &request,
        );

        let json = serde_json::to_string(&grant).expect("the grant serialises");
        assert!(json.contains(r#""role":"display""#), "{json}");
        assert!(json.contains(r#""permissions":[]"#), "{json}");
    }

    #[test]
    fn a_tampered_step_two_is_refused() {
        let host = keypair();
        let device = keypair();
        let rand = [21u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1");
        let step_two = host_side.respond(&step_one).expect("step 2");

        let mut message = step_two.handshake_message().expect("step 2 decodes");
        message[0] ^= 0x01;
        let tampered = SessionFrame::handshake(2, None, &message);

        assert_eq!(
            device_side.finish(&tampered).unwrap_err(),
            SessionError::AuthenticationFailed,
            "a tampered step 2 is refused"
        );
        assert!(
            device_side.accept_grant(&host_side.grant_frame().unwrap()).is_err(),
            "no grant can be accepted without a finished handshake"
        );
    }

    #[test]
    fn an_out_of_order_step_is_refused() {
        let host = keypair();
        let device = keypair();
        let rand = [22u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut host_side =
            HostDelivery::begin(&host.private, HOST_ID, &request, controller_grant(&request))
                .expect("the host builds");
        let mut device_side =
            DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand)
                .expect("the device builds");

        let step_one = device_side.step_one().expect("step 1");
        // Device cannot accept step 1 as its step 2
        assert_eq!(
            device_side.finish(&step_one).unwrap_err(),
            SessionError::AuthenticationFailed
        );
        // Host cannot accept step 2 as its step 1
        let dummy_step_two = SessionFrame::handshake(2, None, &[1, 2, 3]);
        assert_eq!(
            host_side.respond(&dummy_step_two).unwrap_err(),
            SessionError::AuthenticationFailed
        );
    }

    #[test]
    fn a_grant_with_wrong_version_is_rejected_by_device() {
        let host = keypair();
        let device = keypair();
        let rand = [23u8; RAND_BYTES];
        let request = approved_request(&device, rand);

        let mut invalid_grant = controller_grant(&request);
        invalid_grant.v = GRANT_VERSION + 1;

        let prologue = pairing_prologue(HOST_ID, DEVICE_ID);
        let mut host_hs = Handshake::responder(&host.private, &device.public, &prologue).unwrap();
        let mut device_side = DeviceDelivery::begin(&device.private, &host.public, HOST_ID, DEVICE_ID, &rand).unwrap();

        let s1 = device_side.step_one().unwrap();
        let s1_bytes = s1.handshake_message().unwrap();
        host_hs.read(&s1_bytes).unwrap();
        host_hs.confirm_remote_static().unwrap();

        let s2 = host_hs.write(&[]).unwrap();
        let s2_frame = SessionFrame::handshake(2, None, &s2);
        device_side.finish(&s2_frame).unwrap();

        let mut host_transport = host_hs.into_transport().unwrap();
        let payload = serde_json::to_vec(&invalid_grant).unwrap();
        let ct = host_transport.seal(&payload).unwrap();
        let frame = SessionFrame::message(ct);

        assert_eq!(
            device_side.accept_grant(&frame).unwrap_err(),
            SessionError::AuthenticationFailed,
            "a grant with unsupported version is rejected"
        );
    }
}
