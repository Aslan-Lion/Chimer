//! Online transport (Milestone 7, Stage 2).
//!
//! Where the M6 engine owns a listener inside the local network, this module
//! owns an *outbound* connection to a relay the operator runs, and the
//! end-to-end encryption that wraps everything carried over it. The relay
//! forwards opaque bytes; it is never given application plaintext, a device
//! token or a pairing code.
//!
//! The design this follows is `docs/m7-e2e-security-design.md` (Revision 4) and
//! `docs/m7-online-networking.md`. Two cryptographic constructions, both used
//! verbatim from published specifications — nothing here is hand-composed:
//!
//! * **H0, pairing** — HPKE (RFC 9180), `DHKEM(X25519, HKDF-SHA256)` /
//!   `HKDF-SHA256` / `ChaCha20-Poly1305`, base mode, with the human-compared
//!   short authentication string derived from HPKE's own `Export`.
//! * **H1, session** — Noise `KK`, suite
//!   `Noise_KK_25519_ChaChaPoly_SHA256`.
//!
//! # Layout
//!
//! * [`identity`] — the host's long-term X25519 identity and its at-rest
//!   protection
//! * [`persistence`] — the `online-settings.json` document
//! * [`envelope`] — the host's mirror of the relay's `relay/1` envelope
//! * [`client`] — the blocking TLS WebSocket client to the relay
//! * [`tunnel`] — the supervisor, the route table and the reconnect loop
//! * [`pairing`] — H0: the HPKE pairing request, the SAS and the pending list
//! * [`session`] — H1: the Noise `KK` handshake, the transport and the frames
//! * [`delivery`] — §7.6: the token handed to the device over a pairing `KK` run
//!
//! The `KK` session sits at the tunnel's frame boundary: everything the online
//! engine hands the relay is sealed first, and the relay only ever sees opaque
//! `{"e2e":…}` objects.

pub mod client;
pub mod delivery;
pub mod envelope;
pub mod identity;
pub mod pairing;
pub mod persistence;
pub mod session;
pub mod tunnel;

pub use identity::HostIdentity;
