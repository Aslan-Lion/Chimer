//! Local network host (Milestone 6).
//!
//! The desktop application is the only authority: it owns the timer and the
//! session, and it owns the small WebSocket server remote devices talk to.
//! A remote browser never runs a timer of its own — it renders the snapshots
//! this module broadcasts and asks this module to run commands.
//!
//! * [`model`] — the vocabulary (roles, permissions, status, addresses)
//! * [`tokens`] — cryptographic secrets, the pairing-code registry
//! * [`rate_limit`] — sliding-window limiting for pairing and commands
//! * [`devices`] — the paired-device register and its permission rules
//! * [`protocol`] — the wire protocol in both directions
//! * [`persistence`] — the paired-device document
//! * [`qr`] — pairing QR rendering
//! * [`transport`] — the HTTP head reader and the RFC 6455 frame codec
//! * [`engine`] — the listener, the connections and the command pipeline
//! * [`commands`] — the IPC surface the Settings page calls
//! * [`events`] — the `network://` broadcast helpers
//! * [`online`] — the Milestone 7 online transport: an outbound tunnel to a
//!   relay, wrapped end to end (HPKE pairing, Noise `KK` session)
//!
//! # Two transports, one engine
//!
//! A connection reaches [`engine`] over one of exactly two paths, decided by
//! which code path registered it and never by anything the peer claims:
//!
//! * **Local** — the M6 listener, plain `ws://` on the LAN, unchanged.
//! * **Online** — a virtual connection carried over the relay tunnel and
//!   sealed end to end. The seal is unconditional; there is no negotiation
//!   field, no "encryption optional" switch and no downgrade path.
//!
//! Above the transport the two paths are the same code: the command pipeline,
//! permission checks, idempotency and revision bookkeeping all receive the same
//! decrypted bytes a LAN socket produces (`m7-e2e-security-design.md` §12).
//!
//! # Transport security
//!
//! The local server speaks plain `ws://` and `http://` only. This is a deliberate
//! trade-off for Milestone 6: ordinary mobile browsers can load the controller
//! over the LAN without a certificate, but it means **device tokens, pairing
//! codes and timer commands travel unencrypted**. Anyone able to observe the
//! local network — a hostile device on the same Wi-Fi, or a compromised router
//! — can read those frames and could replay a captured device token.
//!
//! What *is* protected: credentials are single-use, pairing codes expire in two
//! minutes, tokens are stored only as hashes on the host, and every command is
//! authenticated, permission-checked and rate-limited per device. What is *not*
//! protected: confidentiality and integrity on the wire. Treat the local
//! network as trusted, or move to TLS (a `wss://` listener with a self-signed
//! certificate the device installs) before treating the LAN as hostile.

pub mod commands;
pub mod devices;
pub mod engine;
pub mod events;
pub mod model;
pub mod online;
pub mod persistence;
pub mod protocol;
pub mod qr;
pub mod rate_limit;
pub mod tokens;
pub mod transport;

pub use engine::NetworkEngine;
