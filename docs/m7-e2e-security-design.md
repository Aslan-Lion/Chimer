# app.timer — M7 end-to-end security design (host ⇄ device)

**Document status: design proposal, revision 4, for review before Stage 2.
Nothing here is implemented.** No host tunnel, no host-side key handling and no
frontend integration will be written until this design is approved or amended.

Revision 3 answers the second review. It (1) separates trusted client delivery
from the relay, (2) repairs the pairing protocol with a human-comparison step
and an authenticated host response, (3) replaces the ad-hoc session DH with a
real Noise `KK` handshake, (4) removes the hand-written nonce and rekey design,
(5) states exact, verified dependency choices and their browser-support limits,
(6) extends the threat model to the attacks this revision is about, and (7)
expands the negative-test matrix.

**Revision 4 applies the four "final corrections" to revision 3 and adds the
results of the standalone Noise/WASM spike** (§0.1, §17):

- `KK` identity verification is stated correctly: both sides supply the expected
  remote static key *before* the handshake state is built, `get_remote_static()`
  confirms rather than discovers, and the host refuses an unknown or revoked
  device before constructing the responder (§8.1, §8.2, §8.3).
- Offline host-key rotation **fails closed**; only a device with an already
  authenticated live session can accept an in-session host-key change (§10.3).
- The SAS construction is specified byte-for-byte, including an explicit
  eight-digit vs six-digit vs word-list evaluation (§7.3).
- The `hpke` pin is reviewed and fixed at **0.14.1**, chosen over `0.14.0` (§5.4.1).
- The spike (§17) proves `snow` 0.10.0 builds for `wasm32-unknown-unknown`,
  interoperates with native Rust on the exact `Noise_KK_25519_ChaChaPoly_SHA256`
  suite, and fails closed on wrong pins and tampered messages.

---

## 0. What changed in revision 3, and why

| # | Review finding | Change in revision 3 |
| --- | --- | --- |
| 1 | **Client delivery is relay-controlled** | Confirmed against the Dockerfile (§1.2). The relay is removed from client delivery; three mechanisms are evaluated with their real trust assumptions; a recommendation and an explicit statement of why "another URL" is not a fix (§4). |
| 2 | **Photographed live QR is treated as harmless** | Corrected. The QR carries the code *and* the pairing public key, so anyone who photographs a live QR can submit a request. A SAS derived from the actual exchange, shown on both screens, and compared by a human, is now mandatory (§7). |
| 3 | **HPKE base mode does not authenticate the sender** | Stated explicitly (§1.4); the host response is no longer base-mode HPKE but a Noise `KK` channel that proves possession of the pinned host identity key (§7.6). |
| 4 | **Custom DH labelled "Noise XX"** | Removed. The session is `Noise_KK_25519_ChaChaPoly_SHA256` (or the BLAKE2b variant, §5.6), driven by a real implementation, with no hand-composed DH and no hand-written key schedule (§5, §8). |
| 5 | **13-byte nonce for a 96-bit AEAD** | Removed entirely. Nonces and counters are the Noise implementation's own (§8.5); application frames no longer carry `epoch`/`seq`/`dir` (§9). |
| 6 | **Custom epoch/rekey** | Deferred. v1 uses a fresh authenticated handshake on every reconnection (§10.1). If in-session rekeying is ever needed it uses the library's documented `REKEY()` — never a bespoke one (§10.2). |
| 7 | **Vague dependency references** | Exact crate and package names and observed versions, purposes, platforms, maintenance notes, browser equivalents and support baselines (§5). |
| 8 | **Browser support assumed** | Removed. Verified baselines are stated, downgrade paths are defined, and the one library that cannot express the required suite is disqualified rather than assumed (§5.3, §5.5, §5.6). |
| 9 | **Threat model incomplete** | Added: relay JS substitution, QR photographer, key-submitting attacker, HPKE-response replay, host-key rollback, storage theft, XSS/same-origin JS (§3, §13). |
| 10 | **Tests underspecified** | Negative-test matrix expanded to 23 cases plus cross-language vectors and a relay-cannot-decrypt test (§14). |

**Errors in revision 2 that are corrected here.**

- Revision 2 §7.6 asserted that the pairing code "never transited the relay" and
  implied a photographed QR was harmless. It is not: the QR carries the code and
  the host's pairing public key, and a photograph is enough to submit a
  well-formed request (§1.3). The fix is a human-compared SAS, not a claim.
- Revision 2 §9 specified a **13-byte** nonce, which cannot be correct for
  ChaCha20-Poly1305 (96-bit nonce). The construction is deleted; the Noise
  implementation owns the counter (§8.5).
- Revision 2 §5.3 named `@chainsafe/libp2p-noise` as the browser Noise
  implementation. That package implements the `XX` pattern inside libp2p framing
  and cannot express `KK`; it is disqualified (§5.5).
- Revision 2 §8 used `XX`. Both peers pin each other's static keys after pairing,
  so `KK` is the correct, shorter, mutually-authenticating pattern (§5.4).
- Revision 2's session key schedule was described as "Noise XX-like" but was a
  hand-composed HKDF over four DH terms. That is a custom protocol. It is
  removed.

### 0.1 What changed in revision 4 (revision 3 final corrections)

| # | Correction required | Where |
| --- | --- | --- |
| 1 | `KK` uses **pre-known** static public keys. Both sides supply the independently authenticated expected remote key when *constructing* the handshake state; `get_remote_static()` confirms. The host loads `D_pub` from the approved registry and refuses unknown/revoked devices before the handshake. | §8.1, §8.2, §8.3 |
| 2 | Offline host-key rotation cannot be delivered; a stale pin **fails closed** and requires a fresh QR scan. In-session rotation applies only to a live authenticated session. Signed key-transition records are deferred. | §10.3 |
| 3 | The SAS is specified canonically: length-prefixed input, domain-separation label, exporter context and length, digit conversion, binding to one request, client-origin validation and replay handling, with an eight-digit vs word evaluation. | §7.3, §7.5 |
| 4 | `hpke` 0.14.1 reviewed against 0.14.0 and chosen; **no Cargo change is made in this task**. | §5.4, §5.4.1 |
| 5 | Standalone Noise/WASM feasibility spike executed; commands, versions, vectors and results. | §17 |

---

## 1. The problems

### 1.1 Relay-visible bearer tokens

The M6 device credential is a **bearer token**: 24 CSPRNG bytes, sent by the
device in `ClientMessage::Authenticate`, checked by the host against a stored
SHA-256, and sufficient on its own to act as that device. The relay terminates
TLS on both legs, so it reads that token in flight. With the Stage 1 design a
relay operator — or anyone who compromises the relay — can capture a live device
token and replay it to the host to impersonate that device.

### 1.2 Relay-delivered client code — confirmed

**Confirmed against the artefacts.** The relay serves the remote-control client:

- [relay/src/http.rs](../../relay/src/http.rs) mounts `ServeDir` as the router's
  `fallback_service`, so any path that is not `/healthz` or `/ws/*` is served
  from disk.
- [relay/src/config.rs](../../relay/src/config.rs) defaults `web_root` to `dist`
  (`DEFAULT_WEB_ROOT`) and reads `RELAY_WEB_ROOT` from the environment.
- [relay/Dockerfile](../../relay/Dockerfile) builds the Vite project in a `web`
  stage, copies `/app/dist` into the runtime image at `/srv/dist`, and sets
  `RELAY_WEB_ROOT=/srv/dist`.

Therefore **the relay operator controls the JavaScript that executes on the
device**. That JavaScript can read `location.hash` — which is how the pairing
code and the host's pairing public key reach the client — and it can read every
decrypted token, key and command. End-to-end encryption protects the channel
between two honest endpoints; it says nothing about the honesty of the endpoint
the relay itself supplied.

### 1.3 A photographed live pairing QR is a real attack vector

The QR fragment contains the pairing **code** and the host's pairing **public
key** `pkPair`. Anyone who photographs a live QR can, from any device, HPKE-seal
a well-formed pairing request to `pkPair`, submit it through the relay, and have
the host open it. Whether the attacker can then have it *approved* is a separate
question — and that question is exactly what revision 3 answers with a
human-compared short authentication string (§7).

A QR is a **broadcast** of two values to anyone with line of sight. It cannot
authenticate the requester. It can only establish a confidential channel to one
specific host.

### 1.4 HPKE base mode does not authenticate the sender

`mode_base` (RFC 9180 `0x00`) provides confidentiality to the recipient and no
sender authentication. Anyone holding `pkPair` can produce a request the host
will open. Any design that derives trust from "only the host could open it"
is deriving trust from confidentiality, which is the wrong direction. Sender
authentication during pairing comes from the human comparison (§7), and host
authentication to the device comes from a Noise `KK` channel that proves
possession of the pinned host identity key (§7.6).

---

## 2. Goals and non-goals

**Goals**

1. Application-level **authenticated encryption** between the Windows host and
   each remote device, independent of the relay.
2. The relay forwards **opaque ciphertext only** — never an application message,
   a device token or a pairing code.
3. Pairing credentials and the initial device token are **encrypted for the
   intended device**, and **approved by a human who has compared a value that
   only the two honest endpoints can produce**.
4. **Mutual authentication**: each side is cryptographically sure which party it
   is talking to, using pinned static keys.
5. Replay, impersonation, reordering and substitution resistance against an
   **active** relay.
6. **Client code that the relay operator cannot silently replace.**
7. Reconnection is always a fresh, authenticated, forward-secret handshake.
8. The host remains the only authority: permission enforcement, revocation and
   command idempotency unchanged and host-side.
9. The existing M6 application protocol carried **unchanged** inside the seal.

**Non-goals (stated so they are not mistaken for guarantees)**

- Hiding **metadata** from the relay: host ids, connection times, source
  addresses, sizes and timing remain visible. Padding is not proposed.
- Protecting against a **compromised host**.
- Protecting a device against **malicious JavaScript running in the client's own
  origin/context** — including XSS. See §11.3 and §13.
- Any change to the M6 **LAN** transport (`ws://` on port 8722).
- Defending against a **malicious host the user chooses to pair with**; a human
  still approves pairings.
- Protecting the pairing step against an attacker who can **observe both
  screens** (e.g. a camera in the room). The comparison defends against remote
  attackers, not against a local observer of the approved device's display.

---

## 3. Threat model

| Actor | Capability assumed |
| --- | --- |
| **Passive relay / network** | Records every byte on both legs. Reads `hostId`, routes, sizes, timing. |
| **Active relay (MITM)** | Modifies, drops, reorders, duplicates, injects and replays any frame; opens its own host or device sockets. |
| **Malicious relay operator** | Everything above, plus full control of the HTTP response body, and therefore of **any client code the relay serves**. |
| **QR photographer** | Has seen a live pairing QR (code + `pkPair`). Can submit an HPKE request with its own device keys from anywhere. |
| **Key-substituting attacker** | Attempts to present its own device public key during pairing, or to substitute/roll back the host identity key. |
| **Replay attacker** | Re-submits a previously captured pairing request or response, or a previously captured transport frame. |
| **Storage thief** | Copies the browser profile / IndexedDB / a backup of either side. |
| **Same-origin script** | Malicious JavaScript executing **in the client's own origin and application context** (XSS, a hostile dependency, or a relay-substituted bundle). |
| **Off-path attacker** | No channel access; may know host ids and the QR URL shape. |

**Trusted:** the Windows host process and its storage; the **client origin named
by the host** (§4); the device browser and its storage, *provided the correct
client code is running*; the OS trust stores used for TLS; the human performing
the SAS comparison.

**Untrusted:** the relay process, its operator, its host, the network path, and
**any client code the relay itself delivers**.

Design consequence: the relay is an **active adversary**. Any client code it can
serve is adversary-controlled code. Cryptography cannot repair a hostile
endpoint; the design must therefore remove the relay from client delivery
(§4) rather than attempt to encrypt around it.

---

## 4. Trusted client delivery (finding 1)

### 4.1 What must be true

For the seal to mean anything, the device must run client code that the relay
operator cannot substitute. Formally: **the code origin must lie outside the
relay operator's control, or the code's integrity must be anchored somewhere the
relay cannot reach.**

### 4.2 The load-bearing insight: the relay cannot choose which client you load

Pairing begins with a QR rendered **by the host** — a machine the user already
trusts. The fragment it encodes is read by the camera and never traverses the
relay. So the host, not the relay, names the client origin:

```text
https://<client-origin>/#v=1&host=<hostId>&code=<6 digits>&pkPair=<b64u>&hostPk=<b64u>&relay=<wss-url>&alg=<suite ids>
```

A malicious relay cannot redirect the device to a hostile origin, because it
never sees or produces that fragment. This converts the problem from "trust the
relay's code" into "trust the origin the host named". That is a real reduction —
and it is a *relocation* of trust, not an elimination of it.

### 4.3 Option A — a separately controlled HTTPS origin

Serve the built client from an origin under a **different account or operator**
from the relay (a static host, a CDN bucket, a separate provider), and **delete
the relay's file-serving fallback** so the relay serves no HTML and no
JavaScript at all.

- **Mechanism.** The host's QR names the client origin; the client reaches the
  relay cross-origin over WSS. The relay already supports this through
  `RELAY_ALLOWED_ORIGINS` — no new relay capability is required.
- **Trust assumption.** The client-origin operator and its platform are not the
  relay operator and are not compromised at the same time.
- **Limitations, stated plainly.** Trust is **split, not removed**. Whoever can
  write to that origin can ship a hostile client to every device. It adds an
  operational dependency and a second thing to keep patched. If the "separate"
  origin is in fact administered by the same person with the same credentials,
  the separation is organisational only.
- **Why it still helps.** It decouples deployment: a relay redeploy, a relay
  compromise, or a malicious relay operator can no longer silently swap client
  code as a side effect of serving traffic.

### 4.4 Option B — a separately distributed and verifiable client

Distribute the client as a build artefact whose integrity third parties can
check independently of the relay:

- **Mechanism.** Signed release artefacts (Sigstore/cosign or a detached GPG
  signature), a published hash manifest, reproducible builds, and — where
  available — a transparency log. A verification tool runs **outside** the page.
- **Trust assumption.** The signing key and the publishing account are separate
  from the relay, and at least one user or third party actually verifies.
- **Limitations.** This is an **audit** mechanism, not a runtime guarantee. A
  user who does not verify gains nothing. It also does not stop a compromised
  signing key. And, crucially, it does **not** let the page verify itself
  (§4.6).

### 4.5 Option C — a packaged application with independent signing and updates

Ship the remote client as an installable application rather than a page: a Tauri
build for desktop/mobile, or a signed package from a store.

- **Mechanism.** The OS verifies code signatures and update signatures; the
  client reaches the relay only over WSS; the relay carries ciphertext and
  serves no code. The update channel is controlled by **its own** signing key,
  not the relay's deployment.
- **Trust assumption.** The OS and the distribution platform's signing PKI — and
  the application's own update key — none of which is the relay.
- **Why it is strongest.** The code-integrity check lives **outside** the
  untrusted data path. This is the only option that structurally removes the
  relay from client delivery instead of sharing the burden with it.
- **Limitations.** Distribution and store-review latency; multiple platforms to
  build; the store and the update-signing key become trust parties; and a
  correctly signed client can still be attacked through its own runtime
  (webview XSS, injected content) even though its code was authentic at install
  time.

### 4.6 Why "just move it to another URL" does not solve the problem

Moving the bundle to a different host changes **who** can substitute the code; it
does not let the client detect substitution. Specifically:

- **In-page integrity pinning fails.** A document cannot verify itself: the code
  that performs the hash comparison arrives from the very origin being checked,
  so a hostile origin simply serves a bundle that omits the check or reports
  success. The same argument defeats Subresource Integrity in this position,
  because the `integrity` attribute is written into an HTML document served by
  that origin. Pinning is only meaningful when the checker runs **outside** the
  page (a browser extension, a native wrapper, or a separate verifier origin).
- **Non-extractable WebCrypto keys do not help.** They prevent key *bytes* from
  being exported. They do not prevent a hostile client from *using* the key:
  injected code calls the same `deriveBits`/`decrypt`/sign operations and reads
  the results. The attack is use, not export. Non-extractability is retained
  (§11.3) for a different threat — storage theft — and must never be described
  as a defence against a hostile client.
- **Encryption does not help.** The hostile client decrypts by design.

There is no general way for a web client to prove its own integrity. Any claim
otherwise is wrong.

### 4.7 Recommendation for this project

1. **Now (minimum change that makes the seal meaningful).** Delete the relay's
   file-serving fallback: the relay serves `/healthz`, the two `/ws/*` routes,
   and **404 for everything else**. Publish the client to a **separate origin**
   named by the host's QR (Option A). Adopt Option B's signing and hash
   publishing as an audit aid.
2. **Target.** Ship the remote client as a **signed packaged application**
   (Option C) with an update channel signed by its own key. This project already
   builds Tauri for the host, so the same toolchain covers the client; the client
   then has no dependence on browser delivery at all.
3. **Never.** Describe in-page hash pinning or non-extractable keys as solving
   client integrity.

### 4.8 Operating rules that must be documented for users

The seal protects the device from the *relay*. It cannot protect a device from
*its own client code*. Users should connect only to client origins and relays
they trust, and — for the strongest posture — self-host both. When the relay
operator and the host owner are the same person, this threat degrades to
"compromised server" rather than "malicious operator".

### 4.9 Consequence for the Stage 1 artefacts

Once §4.7 lands, the Dockerfile's `web` stage, its `/srv/dist` copy and
`RELAY_WEB_ROOT` become dead weight and must be removed, not repurposed. That is
a **Stage 2 change**, recorded here so the Stage 1 artefacts are not mistaken for
the final shape.

---

## 5. Cryptographic implementation (findings 3 and 5)

### 5.1 Governing rules

- **No hand-written cryptographic primitives.** No custom key schedule, no custom
  ratchet, no custom nonce construction. Two thin envelopes remain (§7, §9);
  they carry bytes and add no cryptography.
- **No invented protocol.** Pairing is HPKE (RFC 9180) as specified. The session
  is the Noise `KK` pattern as specified. Every named suite is used verbatim.
- Versions below are those observed during this review. They are **pinned
  exactly** at implementation time and re-checked by `cargo audit` / `npm audit`
  in CI. Any advisory against a pinned version is a stop-and-review event, not a
  routine bump.

### 5.2 Exact suites

**Pairing (H0) — HPKE, RFC 9180 identifiers:**

```text
KEM : DHKEM(X25519, HKDF-SHA256)     0x0020
KDF : HKDF-SHA256                    0x0001
AEAD: ChaCha20-Poly1305              0x0003
MODE: base (0x00)                    // confidential, sender-anonymous — see §1.4
```

**Session (H1) — Noise:**

```text
Noise_KK_25519_ChaChaPoly_SHA256
```

with prologue `ASCII("app.timer/e2e/v1") ‖ hostId ‖ 0x00 ‖ deviceId` (§8.1) and no
modifier, no PSK, no fallback.

The standalone spike in §17 confirms the WASM `snow` path below, so the primary
`Noise_KK_25519_ChaChaPoly_SHA256` suite is used on both sides exactly as above.
Only if the WASM path were abandoned would the JS Noise stack force BLAKE2b
(`Noise_KK_25519_ChaChaPoly_BLAKE2b`) on **both** sides; the suite would then be
negotiated by version, never per connection.

### 5.3 Why these suites

- **HPKE base mode for pairing** because during pairing the device does not yet
  know any host key *usable for an authenticated KEM exchange* other than the
  ephemeral `pkPair` from the QR; base mode gives confidentiality to the host and
  is paired with the SAS + the post-approval `KK` channel for authentication
  (§7).
- **`KK`, not `XX`, for the session**, because after pairing both parties know and
  pin each other's static keys. `KK` is a two-message pattern that authenticates
  **both** sides by construction (the initiator is authenticated as soon as the
  responder processes message 1). `XX` exists to *transmit* statics that are not
  known in advance — which is the pairing case, not the session case — and it
  costs a third message for nothing here, while revealing statics on the wire.
- **SHA-256, not BLAKE2s**, as the primary hash because it has the widest set of
  independent, cross-language implementations and published vectors. The one
  exception is the browser Noise implementation, discussed in §5.5.

### 5.4 Host (Rust)

| Crate | Version | Purpose | Platforms | Maintenance / security notes |
| --- | --- | --- | --- | --- |
| `snow` | `0.10.0` | Noise `KK` handshake and transport states | all Rust targets; optional `no_std` | Pure Rust; tracks Noise spec revision 34. The default resolver already provides Curve25519, ChaCha20-Poly1305 and SHA-256 (BLAKE2b is a feature). Supports `KK`. |
| `hpke` | `0.14.1` | HPKE (RFC 9180) base mode for pairing, and `Export` for the SAS | all Rust targets | Pure Rust; RFC 9180 compliant, including `Export`. **Pin changed from `0.14.0` to `0.14.1`** — see §5.4.1. |
| `zeroize` | pin at impl | Zeroise key material on drop | all | Applied to every long-term and session secret. |
| `rustls` | pin at impl | WSS client to the relay | Windows, Linux, macOS | Unchanged from Stage 1. |
| `rustls-platform-verifier` | pin at impl | OS trust-store verification | Windows/macOS/Linux | Unchanged from Stage 1. |
| `rustls-pemfile`, `webpki-roots` | pin at impl | PEM loading; root fallback | all | Unchanged from Stage 1. |

**Alternative KEM/AEAD libraries.** If `hpke` is ever rejected, `hpke-rs` ≥ 0.8 is
the fallback (RUSTSEC-2026-0070 panic on export-only context and
RUSTSEC-2026-0071 nonce reuse both affect versions before 0.6.0). It must be
pinned to an exact version and pass the RFC 9180 vectors (§5.7) before use. No
alternative mixes with the `hpke` selection; one library is chosen for both sides
of the Rust code.

#### 5.4.1 `hpke` — 0.14.1 chosen over 0.14.0 (correction 4)

Revision 3 proposed `hpke` `0.14.0`. The registry was re-checked during this
correction pass. Versions observed:

| Version | Published | MSRV | Edition | Feature set |
| --- | --- | --- | --- | --- |
| `0.14.0` | 2026-07-09 | 1.85 | 2024 | `aes, alloc, chacha, getrandom, hkdfsha2, kat, mlkem, nistp, shake, x25519` |
| `0.14.1` | 2026-09-06 | 1.85 | 2024 | identical to `0.14.0` |
| `0.15.0` | 2026-10-25 | 1.87 | 2024 | — |
| `0.15.1` | 2026-10-28 | 1.87 | 2024 | — |

**Decision: pin `hpke` `0.14.1`.** Rationale:

- It is the **latest patch on the same minor line** as the version revision 3
  proposed, published two months later. Semver guarantees no API change, only
  fixes; picking the earlier patch of the same line buys nothing.
- Its **MSRV (1.85) and feature set are identical to `0.14.0`**, so the change is
  a patch bump with no ripple: the same feature selection and the same `Export`
  API the SAS depends on (§7.3).
- The `0.15.x` line is **not** taken: it raises the MSRV to **1.87**, which is a
  wider toolchain decision than this correction should force, and is reviewed
  separately.

The versions above were read from the crates.io registry during this task. **No
Cargo dependency was changed in this task** (a stated constraint of the
correction); the pin lives here as the specification until Stage 2. Any advisory
against `0.14.1` remains a stop-and-review event (§5.1).

### 5.5 Browser implementation

**WebCrypto primitives are not a Noise implementation.** `crypto.subtle` gives
DH, HKDF and AEAD; it does not give a Noise state machine, and assembling one
from those primitives is the hand-composed construction this review forbids.

Assessment of the real options:

| Candidate | Verdict | Reasoning |
| --- | --- | --- |
| `@chainsafe/libp2p-noise` (v17) | **Rejected** | Implements the `XX` pattern inside libp2p framing with an identity-key binding. Cannot express `KK`, and adding libp2p framing would change the Noise transcript so it would not interoperate with a standard peer. |
| `noise-handshake` / `noise-protocol` (Holepunch, `noise-handshake` 4.2.0) | **Assessed, conditional** | Genuinely supports multiple patterns, but the underlying `noise-protocol` package documents support for **`Noise_*_25519_ChaChaPoly_BLAKE2b`** only — i.e. **BLAKE2b**, not SHA-256. Adopting it would force both sides to BLAKE2b. It is maintained within a niche ecosystem and must be proven by vectors, not assumed. |
| **`snow` compiled to WebAssembly** (raw `extern "C"` ABI; `wasm-bindgen` **not** required) | **Recommended — proven** | The browser runs the *same* reviewed Rust implementation as the host, so interoperability is guaranteed by construction and one set of vectors covers both sides. The §17 spike built this for `wasm32-unknown-unknown` and completed the exact `KK` handshake against native Rust, with a hand-written marshalling wrapper that contains no cryptography and exposes no private keys. |
| `@noble/curves`, `@noble/hashes` | **Primitives only** | X25519/SHA-256/HKDF building blocks; not a Noise implementation. Pulled in transitively by the HPKE packages. |

**Recommendation — confirmed by the §17 spike.** Use the **WASM build of `snow`**
for the session handshake and transport, keeping the SHA-256 suite on both sides.
The spike answered the three questions that mattered:

- **It builds.** `snow` 0.10.0 compiles for `wasm32-unknown-unknown` once its
  default `use-getrandom` feature is disabled and a caller-seeded `CryptoResolver`
  supplies randomness (§17.2). `getrandom` is absent from the spike's lockfile.
- **It interoperates.** A WASM device (initiator) and a native Rust host
  (responder) completed the byte-exact `Noise_KK_25519_ChaChaPoly_SHA256`
  handshake and exchanged bidirectional transport messages (§17.3–§17.4).
- **It fails closed.** Wrong pinned statics and single-bit handshake tampering
  were rejected on both sides (§17.4).

Consequences to accept:

- A small marshalling surface is required (initiator/responder, `write_message`/
  `read_message`, transport `encrypt`/`decrypt`, static-key setters). The spike
  shows `wasm-bindgen` is **not** required: a raw `extern "C"` ABI over linear
  memory is sufficient. Either way it contains no cryptographic logic and never
  exports private keys (§17.6).
- The client CSP must permit WebAssembly (`'wasm-unsafe-eval'`).
- The module is a build artefact of `snow`; its build must be reproducible and
  its source pinned.

If a pure-JS dependency is mandated instead, the only credible path is the
Holepunch stack **with both sides moved to `Noise_KK_25519_ChaChaPoly_BLAKE2b`**,
after a spike proving `KK` support and byte-level interop against `snow`'s
BLAKE2b suite. Whichever path is chosen, **no Noise state machine is written by
hand**, and the choice stays gated by the §5.7 interop test before Stage 2.

**HPKE in the browser** uses `hpke-js` (pure TypeScript, no WASM):

| Package | Version | Purpose |
| --- | --- | --- |
| `hpke-js` | `1.8.1` | HPKE (RFC 9180) facade; the `CipherSuite` must be constructed explicitly |
| `@hpke/core` | `1.9.0` | HPKE core and ciphersuite registry |
| `@hpke/dhkem-x25519` | `1.8.x` | `DHKEM(X25519, HKDF-SHA256)` (over `@noble/curves`) |
| `@hpke/chacha20poly1305` | `1.8.x` | ChaCha20-Poly1305 AEAD (over `@noble/ciphers`) |
| `@noble/curves`, `@noble/hashes` | transitive | X25519, SHA-256, HKDF |

### 5.6 Browser support requirements

Checked, not assumed. A browser is supported only if **all** hold:

| Requirement | Baseline | Consequence if absent |
| --- | --- | --- |
| Secure context (HTTPS) | universal | Refuse to run. |
| WebAssembly | universal in current browsers | Cannot use the recommended Noise path; fall back to §5.5's conditional JS path, or refuse. |
| WebCrypto `crypto.subtle` | universal | Cannot protect key material at rest (§11); refuse rather than store plaintext. |
| IndexedDB storing `CryptoKey` by reference | universal | Fall back to the wrapped-blob keystore (§11.2). |
| WebCrypto **X25519** (`generateKey`/`deriveBits` with `name: "X25519"`) | **Chrome/Edge 133+, Firefox 130+, Safari 17+** | Only needed for the non-extractable key option (§11.2). Where absent, generate and hold the static key inside the WASM module and store it wrapped. |

The design **feature-detects** each of these at runtime. It never assumes an
algorithm is present because a browser is "modern".

### 5.7 Interoperability and test vectors

Two independent anchors, because "our two implementations agree with each other"
is not evidence:

1. **RFC 9180 official vectors.** Both the Rust host (`hpke`) and the browser
   (`hpke-js`) must pass the published test vectors for
   `DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 / ChaCha20-Poly1305`, base mode,
   including `Export`, before any integration work.
2. **Deterministic Noise transcripts.** Both sides expose a **test-only**
   constructor injecting fixed ephemerals, a fixed prologue and fixed nonces. A
   shared vector file pins, per case: the exact bytes of `KK` messages 1 and 2,
   the derived transport keys, and the ciphertext of a fixed first frame in each
   direction. Both suites must reproduce every byte.
3. **Upstream Noise vectors.** Where the chosen implementation ships vectors for
   its suite, run them unchanged on both sides.

---

## 6. Key material

| Key | Where generated | Lifetime | Where stored |
| --- | --- | --- | --- |
| Pairing key pair `(sPair, pkPair)` | Host, once per pairing session | Destroyed at code expiry or on approval | Memory only |
| Host identity `(sHost, H_pub)` | Host, once | Rotatable (§10.3) | Private key in a separate file with restrictive ACLs or DPAPI; public key in `online-settings.json` and in every QR |
| Device identity `(sDev, D_pub)` | Device, once per (host, device) | Until revoked or re-paired | Device: wrapped keystore (§11.2). Host: `D_pub` only |
| Device token | Host, at pairing | Until revoked | Device: keystore (readable by JS). Host: SHA-256 hex only, as today |
| Noise session keys | Derived by `KK` | One connection | Memory only, zeroised on drop |
| SAS | Derived by HPKE `Export` | One pairing | Displayed, never stored |

The host stores **public keys only** for devices. A stolen `network-devices.json`
therefore yields no capability to impersonate a device.

---

## 7. H0 — pairing, revised (finding 2)

The pairing step has three parts: a **confidential request**, a **human
comparison of a short authentication string**, and an **authenticated token
delivery**. Pairing is the only place where a key that is not yet pinned has to
be trusted, so it is the only place a human is in the loop.

### 7.1 QR fragment (host → device, out of band)

```text
https://<client-origin>/#v=1
  &host=<hostId>
  &name=<hostName>
  &code=<6 digits>
  &pkPair=<b64u 32B>
  &hostPk=<b64u 32B>
  &relay=<wss-url>
  &alg=hpke-x25519-hkdfsha256-chacha20poly1305+noise-kk-25519-chachapoly-sha256
```

- `pkPair` is 32 raw X25519 bytes, base64url without padding (43 characters).
- **`hostPk` is the host's long-term identity public key `H_pub`.** It is included
  (§7.8) so the device can pin the host identity **before any pairing traffic**
  and use it to authenticate the host's response cryptographically.
- `alg` pins both suites so a future change cannot be silently substituted.
- The fragment is never sent to a server by the browser, but **it is readable by
  any JavaScript that runs in the client origin** — hence §4.

### 7.2 Pairing request (device → host, through the relay)

Carried as a relay `data` payload:

```jsonc
{ "e2e": "pair", "v": 1, "enc": "<b64u 32B>", "ct": "<b64u>" }
```

- `enc ‖ ct` is the HPKE base-mode output; `enc` is the encapsulated ephemeral.
- HPKE `info` = `ASCII("app.timer/e2e/v1/pair")`
- HPKE `aad`  = `ASCII("app.timer/e2e/v1/pair|") ‖ hostId`
- plaintext (canonical JSON, UTF-8):

```jsonc
{ "v": 1,
  "code": "123456",
  "deviceStaticPk": "<b64u 32B>",
  "deviceName": "Ada's iPad",
  "clientOrigin": "https://<client-origin>",
  "rand": "<b64u 16B>" }
```

**The sender is anonymous by construction.** A request proves only that its
author could encrypt to `pkPair`, i.e. that it has seen the QR. It proves nothing
about identity. That is expected and is why §7.4 exists.

### 7.3 Short authentication string (SAS)

The SAS is the value the human compares (§7.4). It must be derived
deterministically from the *exact* exchange, bound to the exact pending request
and to the device public key, and identical on the two screens. It is produced
with HPKE's own `Export` — no hand-written KDF.

**Canonical length-prefixed input.** Every variable-length field is encoded as a
4-byte big-endian length followed by its bytes, so no two distinct tuples can
encode to the same byte string:

```text
LP(x) = be32(len(x)) ‖ x
```

with these field instances, in order:

```text
HOST_ID       = LP(ASCII(hostId))
CLIENT_ORIGIN = LP(ASCII(clientOrigin))   // exact string, as sent in the request (§7.2)
PK_PAIR       = LP(pkPair)                 // 32 bytes
ENC           = LP(enc)                    // 32 bytes
H_PUB         = LP(H_pub)                  // 32 bytes
D_PUB         = LP(deviceStaticPk)         // 32 bytes
RAND          = LP(rand)                   // 16 bytes, the request's own nonce
```

**Domain-separation label.** The label is the ASCII string, used verbatim:

```text
LABEL = ASCII("app.timer/e2e/v1/sas")
```

**HPKE exporter context and output length.**

```text
exporter_context = LABEL ‖ HOST_ID ‖ CLIENT_ORIGIN ‖ PK_PAIR ‖ ENC ‖ H_PUB ‖ D_PUB ‖ RAND
SAS_bytes        = HPKE_Context.Export(exporter_context, 8)      // 8 bytes
```

`Export` is guaranteed by RFC 9180 to return the same value to the sender and the
receiver of the same context. The context binds `enc` and `pkPair` (a different
pairing session yields a different value), `D_pub`, `H_pub`, `hostId` and
`clientOrigin` (a different device, host or origin yields a different value), and
**`RAND`, the request's own 16-byte nonce (§7.2)**, which binds the SAS to **one
exact request** so two requests opened under the same code cannot share a value.

**Digit conversion.**

```text
SAS_value  = be64(SAS_bytes) mod 100_000_000          // 8 digits
SAS_digits = zero-padded to 8, grouped 4-4            // e.g. "0482 9173"
```

The modulo bias over an 8-byte `Export` is negligible (≈ 2⁻³⁴ relative) and far
below human comparison error.

**Six digits vs eight digits vs words (evaluation).** The reviewer asked for an
explicit comparison.

| Scheme | Space | Guess probability per attempt | Usability |
| --- | --- | --- | --- |
| **6 digits** (revision 3) | 10⁶ | 1 in 1,000,000 | Familiar and short, but it is the weakest link once rate limits are the only other barrier, and a six-digit number is easy to mis-transcribe under pressure. |
| **8 digits** (chosen) | 10⁸ | 1 in 100,000,000 | Two groups of four; still short enough to read aloud and compare; 100× the entropy of six digits for two extra digits. |
| **4-word list** (e.g. BIP-39-style) | ≈ 4×10⁹–10¹¹ | 1 in ~10⁹–10¹¹ | Highest entropy and good aloud, but needs a *pinned* wordlist, a defined word-to-bits mapping and locale handling — extra specification surface for a modest gain over eight digits. |

**Decision: eight digits.** It is the best trade of entropy against the two things
that actually matter here — human transcription error and the operator's
willingness to compare. A word list is recorded as an acceptable equivalent if
readability is later shown to matter; the wordlist and mapping would then be
pinned by version, not chosen ad hoc.

**Collision and guessing.** An attacker who can submit requests can only hope to
guess the SAS. Each attempt is rate-limited and bounded by the ≤ 8 pending cap
(§7.5), further reducing the 1-in-10⁸ per-attempt probability. The SAS is a
**confirmation** factor layered on the cryptographic checks, not the primary
authentication: even a guessed SAS cannot complete the post-approval `KK`
handshake without `sDev` (§7.6, §7.7).

**Host-side validation of the expected client origin.** The host knows the
`clientOrigin` it served the QR from (a fixed, configured value for the
deployment, §4). When opening a request the host (1) reads `clientOrigin` from the
decrypted request, (2) compares it **exactly** (scheme + host + port, with no
normalisation shortcuts) against the configured expected origin, and (3) rejects
the request (`pair-err{bad-origin}`) on any mismatch. Because `clientOrigin` is
also inside `exporter_context`, a request with a forged origin derives a SAS that
cannot match the one shown on the legitimate device — the two checks are
independent and both fail closed.

**Binding to one exact request, replay and duplicates.** The approval names the
request identity `(SHA-256(enc), D_pub)` (§7.4); the SAS includes `enc`, `D_pub`
and the request `RAND`, so the compared value is valid for exactly one request and
cannot be re-shown for another. Replay and duplicate handling are defined in §7.5.

### 7.4 Human comparison and approval

1. The host shows a bounded list of **pending requests** (≤ 8), each entry showing
   `deviceName`, the `D_pub` fingerprint, and that request's SAS.
2. The device shows its **own** SAS, the `H_pub` fingerprint it pinned from the
   QR, and the host name.
3. The **operator compares** the number on the physical device against the number
   shown for the chosen entry, and approves **exactly that entry**.
4. The device's fingerprint is shown on both sides so a copied `deviceName`
   cannot impersonate a device.

This is the mechanism that answers §1.3: a photographer can *submit* a request,
but their SAS is derived from **their** `D_pub` and **their** `enc`, so it cannot
match the value on the device the operator is holding. A mismatch is a reject.

**Approval is bound to one exact request.** The approval names the request's
identity (`SHA-256(enc)`, equivalently `enc` itself, plus `D_pub`), and only that
request can be approved. There is no "approve the next request" operation.

### 7.5 Code consumption, replay, duplicates and competing requests

| Situation | Behaviour |
| --- | --- |
| **Duplicate request** (same `enc` resent) | Dropped at the pending map; the map is keyed by `enc`, so a replay never creates a second entry. |
| **Replayed request after approval** | Rejected: `pkPair` is destroyed on approval, so the request can no longer be opened. |
| **Delayed request after TTL** | Rejected: the code and `pkPair` are destroyed at expiry (default 120 s) and all pending entries are dropped. |
| **Competing requests** (attacker + legitimate) | Both are listed; the operator approves one; **all others are discarded** and can never be approved afterwards. |
| **Code consumption** | The code is consumed **on approval**, exactly once. |
| **Second approval attempt** | Refused; the code is already consumed and `pkPair` is gone. |

**Why the code is consumed on approval, not on first open.** Consuming on first
open would let anyone who photographs a QR burn the code and prevent the
legitimate device from ever pairing — a trivial denial of service. Consuming on
approval keeps the code single-use for the actual pairing while making the
attacker's request a harmless, rejectable candidate. Rate limiting (the existing
5-per-5-minutes rule, plus the ≤ 8 pending cap) bounds request flooding.

### 7.6 Authenticated token delivery (host → device)

Approval alone does not give the device any proof of **who** approved it, and
base-mode HPKE cannot supply that. So, on approval, the two sides run a
**Noise `KK` handshake** — the same machinery as the session (§8) — and the host
delivers the token inside it:

- The device is the **initiator**, with `rs = H_pub` pinned from the QR.
- The host is the **responder**, with `is = D_pub` taken from the approved request.
- Prologue = `ASCII("app.timer/e2e/v1/pair/kk") ‖ hostId ‖ 0x00 ‖ deviceId`.
- After the handshake, the host sends, sealed:

```jsonc
{ "v": 1,
  "deviceId": "d-…",
  "deviceToken": "<b64u 24B>",
  "role": "controller" | "display",
  "permissions": ["timerCreate", "…"],
  "requestNonce": "<b64u 16B>" }
```

The device records `H_pub` (verified equal to its pin) and `deviceId`, then stores
the token. Nothing is persisted on the host until this handshake has completed.

### 7.7 Authentication argument

- **How the host authenticates the device's public key.** `D_pub` arrives inside
  HPKE ciphertext that only the holder of `sPair` can open, but that alone is not
  authentication. The device is bound to its key by two further facts: the SAS
  the human compared is derived from `D_pub`, and the post-approval `KK`
  handshake requires the device to **prove possession of `sDev`** (the `ss`/`se`
  operations fail otherwise). The host persists `D_pub` only after that proof.
- **How the device verifies the host.** The device pins `H_pub` from the QR
  **before** any pairing traffic, and the `KK` handshake requires the host to
  prove possession of `sHost`. A relay — or anyone without `sHost` — cannot
  complete it. This is precisely what base-mode HPKE could not provide (§1.4).

### 7.8 Should `H_pub` be in the QR?

**Yes.** Reasons, and the alternative:

- It lets the device **pin the host identity out of band**, from the trusted host
  display, before any relay-mediated message is received. Without it the device
  would have to learn the host identity from a pairing message — a weaker anchor.
- It is what makes an **authenticated** host response possible (§7.6), because
  `KK` needs a pre-known responder static key.
- It bounds host-key **rollback**: a device pinned to a current `H_pub` will
  reject an older one (§13).
- Cost: one extra 43-character field. It does not make the QR secret — the QR is
  public either way (§1.3) — so including it changes no trust assumption.

### 7.9 Alternatives considered

**Noise `XK` for pairing** (device initiates, knows `H_pub`; host learns `D_pub`
during the handshake) would fold pairing into a single Noise handshake and drop
HPKE. It was not chosen because it asks the host to run a state machine against
an entirely unauthenticated peer *before* any human decision, and because the
review asked for the HPKE ciphersuite to be fixed. It is recorded here as a
viable simplification if HPKE is ever removed.

---

## 8. H1 — session, revised (findings 3 and 4)

### 8.1 Handshake frames and roles

```jsonc
{ "e2e": "hs", "v": 1, "step": 1, "deviceId": "d-…", "msg": "<b64u>" }
{ "e2e": "hs", "v": 1, "step": 2, "msg": "<b64u>" }
{ "e2e": "hs-err", "v": 1, "code": "<stable-code>" }
```

`Noise_KK_25519_ChaChaPoly_SHA256` is a **two-message** pattern:

```text
device (initiator)  -> e, es, ss      // step 1
host   (responder)  <- e, ee, se      // step 2
```

- **Device** (initiator): `local_private_key = sDev`; `remote_public_key = H_pub`
  — the host key **pinned from the QR** at pairing (§7.1). The pinned key is
  loaded from the device keystore and supplied **before** the initiator handshake
  state is constructed. `KK` requires exactly this pre-knowledge; the device never
  learns the host key from the handshake.
- **Host** (responder): `local_private_key = sHost`; `remote_public_key = D_pub`
  — the device key **loaded from the approved device registry** for the
  `deviceId` carried in step 1. It is supplied **before** the responder handshake
  state is constructed. **If `deviceId` is unknown or revoked, or has no stored
  `D_pub`, the host refuses the connection before any handshake state is built**
  and returns the §8.6 stable error. The device key is *not* learned from the
  handshake.
- `msg` is the literal Noise message emitted by the library. **No libp2p framing,
  no injected identity key, no extra bytes inside `msg`.** Any wrapping the
  transport needs lives in our envelope (§9), so the Noise transcript is exactly
  the standard one.

**Prologue** (mixed into the handshake hash):

```text
ASCII("app.timer/e2e/v1") ‖ hostId ‖ 0x00 ‖ deviceId
```

The `0x00` separator prevents `("ab","c")` and `("a","bc")` colliding. This is the
host/device ID binding: a transcript from one pair cannot be replayed against
another.

`deviceId` is sent in the clear in step 1. It is a random public identifier, not
a secret, and the relay could infer it from the route anyway. Using it to select
the pinned key before the handshake is what makes early refusal — and therefore
cheap revocation — possible.

**Public-key encoding.** X25519 public keys are 32 raw bytes, carried inside the
Noise messages exactly as the suite defines. Pinning compares raw bytes. For human
display the fingerprint is
`SHA-256("app.timer/e2e/v1/fp" ‖ pubkey)[0..16]`, rendered as 8 groups of 4 hex
characters.

**Ephemerals.** Generated by the Noise implementation from the OS CSPRNG, one per
handshake, never reused, zeroised when the handshake state is dropped.

### 8.2 Identity verification in the library's actual behaviour (correction 1)

`Noise_KK_25519_ChaChaPoly_SHA256` is a **pre-message** pattern: the initiator's
static public key is known to the responder, and the responder's static public key
is known to the initiator, **before the first message is sent**. In a `KK`
implementation each side sets its own static private key *and* the expected remote
static public key at build time. The pattern therefore performs **confirmation**,
not **discovery**. This is stated in terms of what a Noise implementation does, not
a hand-written computation.

- **Host (responder).** The host already holds `D_pub` for the named `deviceId`
  from the approved registry. It sets
  `local_private_key(sHost)`, `remote_public_key(D_pub)`, `prologue(…)` and then
  builds the **responder** — `D_pub` is in the handshake state *before* step 1
  arrives. After step 1 is processed, the responder's remote-static accessor
  returns the static key the initiator **proved possession of**; the host compares
  it against the stored `D_pub` as a defensive assertion. It is **not** used to
  obtain the key. Because the `ss` token requires the initiator's static *private*
  key, a peer without `sDev` cannot produce a step 1 that the `ss` operation
  accepts.
- **Device (initiator).** The device holds `H_pub` pinned from the QR. It sets
  `local_private_key(sDev)`, `remote_public_key(H_pub)`, `prologue(…)` and then
  builds the **initiator** — `H_pub` is in the state *before* step 1 is written.
  The `es` and `ss` operations consume it. A responder that does not hold `sHost`
  cannot complete the pattern. The device's remote-static accessor likewise
  **confirms** it equals the pin; it is not discovery.

**`get_remote_static()` — precisely.** It returns the remote party's static public
key *as bound into the handshake transcript*. In `KK` that key was already known to
the caller before the handshake; the accessor exists so the caller can assert that
the key actually used equals the expected one. Treating it as a way to *discover*
an unknown peer identity is `XX`-pattern usage and is **wrong for `KK`**. This
document does not rely on it for discovery anywhere.

### 8.3 Pinning checks and failure codes

| Side | Pre-handshake check | In-handshake check | On failure |
| --- | --- | --- | --- |
| Device (initiator) | load pinned `H_pub` and supply it as `remote_public_key` before building the state; refuse if no pin exists | after step 2, assert `get_remote_static() == H_pub` | Abort, zeroise, `hs-err{host-key-mismatch}` (or `host-key-changed`, §10.3), close |
| Host (responder) | look up `D_pub` in the approved registry for `deviceId`; **refuse before building the state if the device is unknown or revoked** | after step 1, assert `get_remote_static() == D_pub` | Abort, zeroise, `hs-err{authentication-failed}`, close |

### 8.4 Transport keys and direction separation

Noise's `Split()` returns two independent `CipherState`s: `c1`
(initiator → responder, i.e. **device → host**) and `c2` (host → device). They are
used exactly as returned. The two directions therefore use independent keys **and
independent nonce counters**, so a frame cannot be reflected into the opposite
direction. `dir` is not a field we add and check; it is enforced by which cipher
state is used.

### 8.5 Nonces and counters — library-owned (correction of revision 2)

Revision 2 specified a 13-byte nonce. That is wrong: ChaCha20-Poly1305 takes a
**96-bit** nonce. The Noise specification encodes the 64-bit counter as four zero
bytes followed by the 8-byte little-endian counter — 12 bytes — and the
implementation does this internally.

**Revision 3 specifies no nonce construction at all.** The `CipherState` owns a
strictly increasing counter; each `encrypt`/`decrypt` advances it. Because the
counter is part of the AEAD input, a frame that is replayed, reordered, dropped or
cross-injected arrives under the wrong counter and **fails authentication**. The
session is then torn down. No plaintext from a failed frame is ever used.

This is the conservative, fail-closed behaviour, and it is correct for a single
WebSocket stream per route where reordering does not occur in normal operation. A
malicious relay can force a teardown, but that is only denial of service, which is
already conceded (§13).

### 8.6 Authentication failure behaviour

On any failure — pin mismatch, AEAD tag failure, prologue mismatch, unknown or
revoked `deviceId`, or an application frame arriving before the handshake
completes — the receiver:

1. does **not** process any application message from that connection;
2. emits a terminal `hs-err` (or a sealed `err` on an established session) with a
   **stable, non-oracular** code, so a prober cannot distinguish "unknown device"
   from "wrong key";
3. closes the socket;
4. charges the online rate limiter for that route and source address;
5. logs the event **without** key material, frame contents, `deviceId` or token.

---

## 9. Application frames

```jsonc
{ "e2e": "m", "v": 1, "ct": "<b64u>" }
```

`ct` is the Noise **transport-mode** ciphertext for that direction, produced by
the library's `write_message`. Its plaintext is the **M6 frame verbatim**
(`ClientMessage` or `ServerMessage`), canonical JSON, unchanged.

Revision 2 wrapped each frame in a `{ "epoch", "seq", "dir", "body" }` object.
**That is removed**, because `seq`/`dir` duplicated the Noise counter and
direction ciphers (§8.4, §8.5) and `epoch` duplicated the connection lifecycle
(§10.1). Fewer bytes, no second sequencing scheme to get wrong, and no
opportunity for the two schemes to disagree.

Because the relay requires `payload` to be a **JSON object** of bounded size
([relay/src/wire.rs](../../relay/src/wire.rs), `payload_within`), and every E2E
frame is one, **the relay needs no change at all** to carry this. It validates
shape and size and understands nothing else. It never learns whether a payload is
a handshake message, an application message or a pairing request.

---

## 10. Rekey, rotation, termination (finding 4)

### 10.1 Per-connection (the v1 answer)

**Every reconnection is a fresh, authenticated `KK` handshake with fresh
ephemerals.** There is no session resumption, no ticket and no ticket-like
mechanism. Forward secrecy with respect to the static keys therefore holds for
every session, and there is no long-lived session key to rotate on a timer.

This is deliberately simpler than revision 2's in-session epoch scheme and it
removes a class of state-machine bugs. `KK` costs one extra round trip on
connect, which is irrelevant for a control channel.

### 10.2 In-session rekeying — deferred

**v1 does not rekey within a session.** A connection is instead recycled by the
client on a policy basis (a documented maximum age and/or a 2^20-frame
threshold), which produces a new handshake through §10.1.

If in-session rekeying is ever required, it uses the Noise specification's
`REKEY()` as implemented by the chosen library (`snow`'s transport rekey), never a
bespoke ratchet, and it would come back for review. Revision 2's custom
epoch protocol is withdrawn.

### 10.3 Static key rotation (correction 2)

Rotation is split by whether the device currently holds an **authenticated live
session** to the host.

**Device key.** Replace `D_pub` for a `deviceId` by **revoke and re-pair** from
inside a live sealed session (§11.6, §11.7). The new `D_pub` enters the registry
only after a fresh `KK` handshake proves possession. This is the v1 path.

**Host key — in-session only.** For a device that **already has an authenticated
live sealed session**, the host MAY announce a new `newH_pub` inside that session
(a sealed `host-key-changed` control frame carrying the new public key and an
activation time). Because the channel is already mutually authenticated and
forward-secret, the device can accept the change and re-pin; on reconnect it then
pins `newH_pub`. This is the **only** case in which a host-key change is accepted
without a new QR scan.

**Host key — offline device (fails closed).** A device that is offline cannot
receive an authenticated key-change through a session it has not established, and
the relay is untrusted. Therefore:

- A device that still holds a **stale host-key pin** at reconnect has its `KK`
  handshake against the new `sHost` **fail**; the device then **fails closed** —
  it aborts, keeps its old pin, and surfaces the distinct code
  `host-key-changed` (not `host-key-mismatch`) so the user is told to re-pair.
- The device **must not** silently trust the replacement key. It never accepts a
  host key from the relay, from a QR it did not just scan, or from any
  unauthenticated message.
- Recovery is a **fresh QR scan and re-pairing** (§7, §11.7). After the new
  pairing the device pins `newH_pub` and receives a new token.

**Signed key-transition records are deferred.** Making an offline host-key change
deliverable without a scan would require a signed, replay-protected key-transition
record authenticated by the *old* key and verifiable offline — essentially a small
PKI, with its own threat model (old-key compromise, transition replay, ordering).
**It is explicitly deferred to a future design review** and is **not** part of v1
or Stage 2: no such record is defined here, and v1 must not emit or accept one.

**No silent rollback or roll-forward of the host identity is possible.** The
device trusts only `H_pub` values it pinned from a QR, or an in-session change
received over an already authenticated live session.

### 10.4 Termination

A session ends on a WebSocket close, on a sealed `e2e:"bye"`, on idle timeout, or
on any §8.6 failure. Both sides zeroise key material. **No resumption:** a
reconnect is always a fresh `KK`.

---

## 11. Browser storage and client security

### 11.1 Generation

The device identity key is an X25519 pair generated once per (host, device). Two
paths, chosen by feature detection (§5.6):

- **Preferred where supported:** `crypto.subtle.generateKey({ name: "X25519" },
  extractable, ["deriveBits"])` (Chrome/Edge 133+, Firefox 130+, Safari 17+).
- **Fallback:** generate the pair inside the WASM Noise module.

### 11.2 Persistence

Private keys are **never** written to `localStorage` as plaintext. A WebCrypto
`CryptoKey` is not a string and cannot be stored there at all.

| Value | Store | Protection |
| --- | --- | --- |
| Device identity private key `sDev` | IndexedDB, key `e2e/<hostId>/<deviceId>` | See below |
| `deviceId`, `hostId` | IndexedDB (same record) | Public identifiers |
| `hostPk` (the pin) | IndexedDB (same record) | Authenticated with an HMAC (§11.2.1) |
| Device token | IndexedDB, separate record | Readable by JS by design; cannot be non-extractable |

**Two options for `sDev`, with an honest trade-off.**

- **(a) Non-extractable `CryptoKey`.** Stored in IndexedDB by reference;
  `extractable: false` is fixed at generation and cannot be added later. Strongest
  at-rest protection. **Requires that the Noise handshake can operate on a
  `CryptoKey`** — which WASM `snow` and every current JS Noise package cannot,
  because they take raw key bytes. This option is only available if a future
  implementation accepts a `CryptoKey` handle.
- **(b) Wrapped keystore (baseline).** Generate the pair, **wrap** the private
  scalar with a **non-extractable** AES-GCM `CryptoKey` held by the origin
  (`crypto.subtle.wrapKey`/`unwrapKey`), and store only the wrapped blob, IV and
  the wrapping-key handle in IndexedDB. The scalar is unwrapped into memory for
  the duration of a handshake and zeroised afterwards. Any Noise implementation
  can use it, and at rest it is protected by a key that cannot be exported.

Baseline is **(b)**; adopt **(a)** if a suitable implementation appears. Neither
is presented as a defence against hostile client code.

#### 11.2.1 Protecting the pin

If an attacker can rewrite the `hostPk` pin in IndexedDB they can pin a key of
their own. The pin record is therefore authenticated with an HMAC computed by a
**non-extractable** key that never leaves the crypto store; tampering makes the
pin unusable and forces re-pairing rather than silently accepting a new host key.
This raises the cost; it does not make the store tamper-proof.

### 11.3 What non-extractable keys do and do not buy

**Do:** prevent the key bytes from being read out of the browser, including by
injected script that only tries to *export* them; survive ordinary reloads; and
are scoped to the creating origin.

**Do not:** prevent a compromised client from **using** the key. Malicious
JavaScript in the same origin and context calls the same `deriveBits`/`decrypt`/
sign operations and reads the results. Non-extractable keys also do not protect
the device token, the plaintext, or anything else in the page's memory.
**Non-extractable WebCrypto keys are not protection against malicious JavaScript
executing in the same application context**, and this document does not claim
otherwise. Their real value is defence in depth against *storage* compromise (a
stolen profile, a filesystem backup) — a different threat from §1.2.

### 11.4 Optional strengthening

Where a platform authenticator exists, authentication can additionally be bound
to a **WebAuthn/passkey** credential: the device signs a value tied to the
handshake transcript with a hardware-backed key requiring user presence. This
makes *impersonation* much harder — injected code cannot forge the signature
without prompting the user — but it does **not** restore confidentiality, because
the hostile client still performs the key agreement and reads the plaintext. It
improves identity, not delivery. Listed as an option, not folded into the
baseline.

### 11.5 Lost storage

If IndexedDB is cleared, `sDev` is **gone and unrecoverable** — the host holds
only the public key and there is no key escrow. The device detects this on the
first handshake failure (`hs-err{authentication-failed}`) and:

1. discards the stale token and pin for that host;
2. shows "This device must be paired again with <host name>";
3. requires a fresh QR scan and a fresh human comparison and approval.

The stale registry entry on the host is removed when the user revokes the old
device, or superseded when the re-pair issues a new `deviceId`.

### 11.6 Revocation

The host deletes the registry entry (token hash **and** `D_pub`) and closes any
live tunnel for that device. Subsequent `KK` attempts fail at the pin lookup,
before any cryptography. The device sees `hs-err{authentication-failed}` and
returns to the pairing screen after clearing its local record.

### 11.7 Re-pairing

A new QR, a new single-use pairing key, a new code, a new human comparison. The
new `D_pub` replaces the old one for the new `deviceId`. Nothing auto-migrates a
device's key material.

---

## 12. Integration with M6, migration and downgrade protection

### 12.1 Where the seal sits

The E2E layer wraps the online leg only, at exactly one point: the tunnel's frame
boundary. Above it, nothing changes.

| Layer | Effect |
| --- | --- |
| `src-tauri/src/network/protocol.rs` | **Unchanged.** M6 messages travel verbatim inside the ciphertext. |
| Command pipeline (`handle_message` → `handle_command` → `execute`) | **Unchanged.** It receives the same decrypted bytes a LAN socket produces. |
| Permission enforcement | **Unchanged**, host-side and authoritative; a `display` device still has an empty permission set. |
| `DeviceRegistry` / token hashing / constant-time compare / revocation | **Extended by one field** (`D_pub`); the rest untouched. |
| Command idempotency (`command_memory`, 64, keyed by device id) | **Unchanged.** Above the transport, so it still gives exactly-once execution across reconnects. |
| Command rate limiter (20/s per device id) | **Unchanged.** |
| Revision-gated state and snapshot reconciliation | **Unchanged.** |

### 12.2 Mandatory re-pair for existing online devices

Every device paired under the Stage 1 online protocol must re-pair. There is no
in-place upgrade: it has no identity key and no pin, so there is nothing to
authenticate a bootstrap with, and inventing one would reintroduce the exposure
being removed.

Mechanism: the host's tunnel requires a completed `KK` handshake before it will
interpret anything else. A Stage 1 client sends a raw M6 message as its first
frame; the host refuses it with `hs-err{authentication-failed}` (uniform code,
§8.6) plus a separate, non-oracular UI hint on a **diagnostic** channel, and
closes. The client shows "This device must be paired again". Operators should
expect to re-issue QR codes; this is a release note, not a silent behaviour
change.

### 12.3 No fallback, by construction

- There is **no** configuration switch, negotiation field, "encryption optional"
  flag or version downgrade path on the online leg. Sealing is unconditional.
- A relay cannot strip the seal: a rewritten or removed `ct` fails authentication
  and teardown (§8.6).
- A relay cannot advertise "no E2E": the host never asks the relay what to do.
  Suites are pinned in the QR (`alg`) and in the host binary.
- The host accepts exactly one of two transports per connection, decided by which
  socket the connection arrived on, not by anything the peer says.

### 12.4 How the host distinguishes transports without weakening M6

| Transport | How created | Application-layer protection | M6 checks |
| --- | --- | --- | --- |
| `Local` (LAN) | M6 listener on port 8722, `ws://` | none — unchanged M6, trusted-LAN model | unchanged |
| `Online` (relay) | tunnel connection handed to the network engine | mandatory E2E seal (`KK`) | unchanged, applied to decrypted bytes |

The discriminator is `Connection.transport`, set by **which code path registered
the connection** — never by a claim from the peer. The online path's state machine
starts in `AwaitingHandshake` and reaches `Sealed` only by completing the `KK`
handshake; there is no transition from `AwaitingHandshake` to anything that
processes M6.

**M6 is not weakened** because it is not touched: the LAN leg keeps its exact
current rules and threat model, and the online leg gains protections and loses
none — every check that refused a request before still refuses it, on the same
decrypted bytes.

### 12.5 Two pairing registries

A **separate** online `PairingRegistry` is retained, so local and online codes
cannot invalidate one another. E2E changes only how the online code *travels*
(inside an HPKE ciphertext rather than a plaintext frame), not which registry
holds it, its TTL, its single-use rule, its approval binding or its rate limiter.

---

## 13. Threat-model table (finding 6)

### 13.1 What the relay can and cannot do

| Capability | Stage 1 (pre-E2E) | Revision 3 |
| --- | --- | --- |
| Read application messages | **Yes** | No |
| Read the device token | **Yes** | No |
| Read the pairing code | **Yes** | No |
| Replay a captured token to impersonate a device | **Yes** | No — the session also requires `sDev` |
| Impersonate the host to a device | No | No — the device pins `H_pub` and `KK` proves possession |
| Forge or modify an application message | **Yes** | No — AEAD |
| Substitute or reorder frames | **Yes, silently** | Only as a detectable failure that tears the session down |
| Serve hostile client code | **Yes** (confirmed, §1.2) | Not from the relay; still possible from a compromised **client origin** (§4.3) |
| Learn which hosts are online | Yes | Yes |
| Learn which devices connect, when | Yes | Yes |
| Learn message sizes and timing | Yes | Yes |
| Learn source addresses | Yes | Yes |
| Deny service (drop, delay, refuse) | Yes | Yes |
| Read persisted timer state, the device registry, or approve a pairing | No | No |
| Act as a second source of truth | No | No |

### 13.2 The attacks this revision is about

| Attack | Status | Mechanism / why |
| --- | --- | --- |
| **Relay modifies the delivered JavaScript** | **Mitigated by architecture, not crypto** | The relay no longer serves code (§4.7). Residual: whoever controls the *client origin* can still do it — trust is split, not removed. |
| **Attacker photographs a live pairing QR** | **Mitigated, conditional on the human** | The attacker can submit a request (§1.3), but its SAS is derived from its own `D_pub`/`enc` and cannot match the device the operator is holding (§7.4). Fails if the operator approves without comparing. |
| **Attacker submits its own device key during pairing** | **Mitigated, conditional on the human** | Same SAS/fingerprint comparison; additionally the post-approval `KK` requires possession of `sDev` (§7.7). |
| **Attacker replays an old HPKE pairing response** | **Mitigated** | Responses are bound to one exact approved request; `pkPair` and the code are destroyed on approval; the delivery channel is a fresh `KK` with fresh ephemerals (§7.5, §7.6). |
| **Attacker substitutes or rolls back the host identity key** | **Mitigated** | The device pins `H_pub` from the QR; `KK` uses it in `es`/`ss`, so a different key cannot complete the handshake. Rollback produces the distinct `host-key-changed` outcome, never silent acceptance (§8.2, §10.3). |
| **Browser storage theft** (profile/backup copied) | **Partially mitigated** | `sDev` is protected at rest by a non-extractable wrapping key (§11.2); the **device token is readable by JS** and is not protected against an offline reader. A stolen live profile can therefore yield the token. |
| **XSS / malicious same-origin JavaScript** | **Out of scope — stated, not mitigated** | Non-extractable keys do **not** help: the attacker *uses* the key rather than exporting it (§11.3). Mitigation is architectural — a packaged client (Option C), strict CSP, and not delivering code from the relay. |
| **Replayed/mutated transport frames** | Mitigated | Noise counter + AEAD; failure tears the session down (§8.5, §8.6). |
| **Metadata, timing, DoS** | **Out of scope** | Unchanged from revision 2; the relay still sees who connects and when, and can still deny service. |

### 13.3 The honest summary

The relay's remaining power is **denial of service plus metadata**, plus the
ability to serve a hostile client *if it is still in the delivery path*. It cannot
read a credential, forge a message, or act as a device. The one capability that
cryptography cannot take away — serving hostile client code — is handled by
**removing the relay from client delivery** (§4), not by encrypting around it. The
one capability that no cryptography can provide — a human who actually compares
the numbers — is handled by **making the comparison the approval step** (§7.4),
and it is the weakest link in the design.

---

## 14. Required security tests (finding 7)

### 14.1 Harness

Host-side tests run against an **adversarial in-process relay** that can tamper on
demand: rewrite a frame, replay a frame, drop a frame, reorder two frames,
substitute a `ct`, swap directions, and change the `e2e`/`v`/`step` fields.
Browser-side tests run in a headless browser against the same harness with
scripted tampering. A **shared vector file** is consumed by both the Rust and the
browser suites.

### 14.2 Negative-test matrix

| # | Attack | Expected result |
| --- | --- | --- |
| 1 | **Pairing QR photographed by an attacker** | The attacker's request opens and is listed; its SAS differs from the physical device's SAS; approval of it is refused for the legitimate device; the legitimate request still pairs. |
| 2 | **Competing legitimate and malicious requests** | Both listed; approving one discards the other permanently; exactly one device is registered. |
| 3 | **Mismatched authentication strings** | Approving an entry whose SAS does not match the device is not possible through the UI, and a forged approval for a non-matching request is refused by the host. |
| 4 | **Incorrect host identity key** (device pinned a different `H_pub`) | `KK` cannot complete; `hs-err{host-key-mismatch}`; no application frame is ever sent. |
| 5 | **Incorrect device identity key** (attacker static, claimed `deviceId`) | Host aborts after step 1 with `hs-err{authentication-failed}`; nothing from the M6 layer executes. |
| 6 | **Replayed HPKE pairing request** (`enc`/`ct` resent) | Deduplicated at the pending map; no second entry; no second approval possible. |
| 7 | **Replayed pairing response** | Rejected: `pkPair` and the code are gone; the `KK` channel is fresh per attempt. |
| 8 | **Host-key rollback** (older `H_pub` presented) | Refused with `host-key-changed`; the client does **not** silently re-pin. |
| 9 | **Wrong Noise pattern or protocol name** | Handshake fails immediately; assert the implementation refuses a mismatched prologue/suite rather than degrading. |
| 10 | **Rust/browser interoperability** | Both sides reproduce every byte of the shared `KK` transcript vector; RFC 9180 vectors pass on both. |
| 11 | **Modified handshake message** (flip a bit in step 1 or 2) | AEAD failure → `hs-err`, teardown, no plaintext used. |
| 12 | **Modified ciphertext** (flip a bit in an application frame) | AEAD failure → sealed `err`, teardown, rate limiter charged. |
| 13 | **Replayed transport frame** | Counter mismatch → authentication failure → teardown. |
| 14 | **Reordered transport frames** | Second frame fails authentication → teardown (fail-closed). |
| 15 | **Wrong nonce / sequence handling** (frame from another point in the stream) | Authentication failure → teardown; assert no resynchronisation is attempted. |
| 16 | **Unsealed application frame** (raw M6 message as the first frame) | Refused with `hs-err{authentication-failed}`; assert **no** code path reaches `handle_message` without a completed `KK`. |
| 17 | **Wrong host ID** (transcript from another host replayed) | Prologue mismatch → handshake hash differs → failure → teardown. |
| 18 | **Wrong device ID** (transcript replayed under another `deviceId`) | As above, plus the pin lookup finds a different `D_pub`. |
| 19 | **Cross-session injection** (frame from session A replayed into session B) | Distinct session keys → AEAD failure → teardown. |
| 20 | **Expired device credential** (token rotated) | Host-side auth refuses; tunnel closed; client returns to pairing. |
| 21 | **Revoked device credential** | Refused at the pin lookup before any cryptography; live tunnel closed; re-auth impossible. |
| 22 | **Duplicate pairing approval** (operator double-clicks, or two approvals race) | Exactly one token and one registry entry; the second approval is refused; the code is consumed exactly once. |
| 23 | **Downgrade attempt** (strip the seal, claim an older version) | `hs-err{authentication-failed}`, teardown; assert no negotiation field exists and no code path bypasses `KK`. |
| 24 | **Modified browser-client delivery** | Architectural assertion, not cryptographic: (a) the relay returns 404 for `/` and `/assets/*`; (b) the relay image contains no HTML or JS; (c) the client is served only from the configured client origin. A hostile-client scenario *demonstrates* that the seal does not help, keeping that fact from being forgotten. |

### 14.3 The relay cannot decrypt (required test)

A dedicated test captures **every byte** the relay forwards during (a) a full
pairing and (b) a full session, and asserts:

- neither the device token (24 bytes) nor the pairing code (6 digits) nor any
  static private key nor any `deviceName` appears in any captured buffer;
- the frames are opaque to the relay's own parser: replaying them through
  `relay::wire::parse` yields no application-level field;
- a modified capture cannot be decrypted by either endpoint.

### 14.4 Property tests

- Permission enforcement, revocation and command idempotency behave identically
  over the sealed tunnel and over a LAN socket, including "same command id twice
  across a reconnect ⇒ executed once".
- Forward secrecy: after compromising `sHost` and `sDev`, a recorded transcript
  still cannot be decrypted.
- Logs contain no token, code, `deviceId`, key material or payload, on both sides.

### 14.5 Cross-language vectors

1. **RFC 9180 official vectors**, passed by `hpke` (host) and `hpke-js` (browser)
   for the §5.2 ciphersuite, base mode, **including `Export`**.
2. **Shared `KK` transcript vectors**: fixed ephemerals, fixed prologue, fixed
   nonces → exact bytes of messages 1–2, the split transport keys, and the
   ciphertext of a fixed first frame in each direction. Both suites must reproduce
   every byte. The §17 spike already produces such vectors for `snow` (native
   side); the remaining work is the browser-side reproduction with the production
   wrapper.
3. **Upstream vectors** for the chosen Noise implementation's suite, run
   unchanged on both sides.

---

## 15. Unresolved risks

Stated honestly. None of these is hidden by the design above.

1. **The human comparison is the weakest link.** Every pairing attack is stopped
   by an operator correctly comparing eight digits and a fingerprint. Fatigue,
   haste or a remote-access session defeats it. There is no cryptographic
   substitute; alternatives (out-of-band code entry, WebAuthn-bound approval) only
   move the same human step.
2. **Client delivery splits trust; it does not remove it.** With Option A, whoever
   controls the client origin can still ship a hostile client. Option C moves the
   anchor to the OS and a store, which are still trust parties. There is no design
   in which a web client proves its own integrity.
3. **The browser Noise implementation is settled in principle, not yet in
   production.** The mainstream JS Noise packages implement BLAKE2b, not SHA-256,
   so the SHA-256 suite implies a WASM build of `snow`. The §17 spike **proved
   this feasible** — it builds for `wasm32-unknown-unknown` and interoperates with
   native Rust on the exact suite, without `wasm-bindgen`. What remains is to
   productionise it: reproducible build, pinning and review (§15.4).
4. **WASM `snow` is a project-owned artefact.** The spike wrapper is
   non-cryptographic, but the production wrapper must still be built reproducibly,
   pinned and reviewed; it becomes part of the trusted computing base of the
   client. The spike's export whitelist and no-private-key-exposure property
   (§17.1) are the starting constraints, not a finished review.
5. **Endpoint compromise.** Malware on the host, or XSS on the trusted origin,
   defeats the seal. Non-extractable/wrapped keys raise the cost of *storage*
   attacks only.
6. **The device token is readable by client JavaScript.** It cannot be
   non-extractable. A hostile or stolen client profile yields it.
7. **Metadata is not protected.** Host ids, connect times, source addresses, sizes
   and timing remain visible and are enough for traffic analysis.
8. **Denial of service remains fully possible.** A relay can drop, delay, reorder
   or refuse; the design makes reordering *safe*, not *survivable*.
9. **Long-lived static keys.** A leaked device identity key permits impersonation
   until revocation; rotation bounds but does not remove this.
10. **No formal proof or audit.** This is a design. Until it is implemented and
    tested against an adversarial relay, the Stage 1 limitation still applies in
    production.
11. **The 6-digit code is not the security boundary.** Security rests on the
    QR-carried keys plus the human comparison. This must be documented for
    operators so nobody mis-reasons about a photographed QR.
12. **Migration is disruptive.** Every existing online device must re-pair, and now
    also have a human compare a number. That is the correct security choice and a
    real usability cost.
13. **Token delivery still needs a channel the device can trust before it has a
    token.** That channel is `KK` against the QR-pinned `H_pub`. If `H_pub` is
    stale (host key rotated without the device knowing), pairing fails with
    `host-key-changed` and the user must re-scan — correct, but it is a failure
    mode operators will see.

---

## 16. Decisions requested from the reviewer

1. **Client delivery.** Approve removing the relay's file-serving fallback
   (`ServeDir`/`RELAY_WEB_ROOT`/the Dockerfile `web` stage) and choose the
   mechanism: Option A now with Option C as target (§4.7), or Option C directly.
2. **Pairing.** Approve the revised H0: `H_pub` in the QR, an **eight-digit** SAS
   derived by HPKE `Export` (§7.3) and compared by a human, approval bound to one
   exact request, code consumed on approval, and token delivery over `KK` (§7).
3. **Session pattern and suites.** Approve `Noise_KK_25519_ChaChaPoly_SHA256` with
   the §5.2 HPKE ciphersuite, and the §5.5 decision path — WASM `snow`, now
   **proven feasible** by the §17 spike — or the BLAKE2b JS alternative.
4. **Dependencies.** Approve `snow` 0.10.0 and `hpke` **0.14.1** (chosen over
   0.14.0, §5.4.1; or `hpke-rs` ≥ 0.8) on the host, and `hpke-js` 1.8.x +
   `@hpke/*` 1.9.x on the browser, with the §5.7 interop test as a gate.
5. **Rekey.** Confirm that v1 uses a fresh `KK` per connection and defers
   in-session rekeying (§10).
6. **Migration.** Confirm mandatory re-pairing with no fallback path (§12).
7. **Scope.** Confirm that metadata protection, cover traffic, DoS resistance and
   same-origin-XSS protection remain out of scope (§2, §13.2).
8. **Rotation.** Confirm that v1 host-key rotation is in-session only for a live
   authenticated session, that an offline stale pin fails closed and needs a fresh
   QR scan, and that signed key-transition records are deferred (§10.3).
9. **Noise/WASM spike.** Accept the §17 spike results as the feasibility gate for
   the WASM `snow` path — or direct an alternative; the pattern and cipher suite
   would not be switched silently.

---

## 17. Standalone Noise/WASM feasibility spike (correction 5)

A standalone proof of concept was built **outside the application source tree**
(`spike/noise-kk-wasm/`, workspace `temp_20261113_01`) and executed. It does
**not** modify the application, its `Cargo.toml`/`Cargo.lock`, the frontend, the
relay or the deployment configuration, and it is **not** Stage 2. The full report
is the companion deliverable `m7-noise-wasm-spike-report.md`; the results are
summarised here.

### 17.1 What was proven

| Claim | Result |
| --- | --- |
| `snow` 0.10.0 compiles to the intended WASM target (`wasm32-unknown-unknown`) | built — 152,988 bytes |
| Secure browser randomness is available | via `globalThis.crypto.getRandomValues` |
| Browser and Rust complete the exact `Noise_KK_25519_ChaChaPoly_SHA256` handshake | byte-exact, 48-byte messages |
| Both sides derive compatible transport states | identical handshake hash, identical ciphers |
| Bidirectional encrypted messages work | 3 frames each way |
| Wrong static keys fail closed | rejected on both sides |
| Modified handshake messages fail closed | every single-bit tamper rejected |
| The wrapper does not expose private key material | no enumerable state; export surface contains no secret |

**50 assertions passed, 0 failed.**

### 17.2 Toolchain and build

- rustc / cargo `1.100.0`; targets `wasm32-unknown-unknown` and
  `x86_64-pc-windows-msvc`; Node `v24.19.0`.
- **No `wasm-bindgen` and no `wasm-pack`.** The ABI is a raw `extern "C"` surface
  over WASM linear memory, proving the wrapper need not pull in a binding
  generator.
- The one real obstacle: `snow`'s default features enable `use-getrandom`, and
  `getrandom` 0.3 does not build for `wasm32-unknown-unknown` without a `wasm_js`
  backend. Solved by building `snow` with
  `default-features = false, features = ["default-resolver", "use-curve25519",
  "use-chacha20poly1305", "use-sha2"]` and supplying a caller-seeded `ChaCha20Rng`
  through a custom `CryptoResolver`. **`getrandom` is absent from the spike's
  `Cargo.lock`.** The suite and pattern are unchanged.

### 17.3 Deterministic test vectors

Full transcript in the report; the pinned bytes:

```text
prologue            = 6170702e74696d65722f6532652f7631...  (ASCII "app.timer/e2e/v1" ‖ hostId ‖ 0x00 ‖ deviceId)
H_pub               = b755ced64d4a27ce32afcf199f18a3ed1f31897028b0ff6e55191ea449db2644
D_pub               = b6c6192e66300f4bbb4e3d870bfd02e416154ebb06661a70a84ea376244b3c20
KK message 1 (48 B) = a28d6dd77eb4e12f9347071f7f369fa14a91ebf01368e59364983492e10a6729855000986723201e9b2af85767fd92b0
KK message 2 (48 B) = 3ec09a863f81f9adc6ef1c865295fada64550430b4b31aa40ef6c9b593290b5facd7757a8c60472ebcc73691e12d8a0d
handshake hash      = a9f00d3c5e734a4e0aff04ca9bd7f4d452ba82f85a9f429464d3d070a3bc3581
```

### 17.4 Negative results

- Device built with the **wrong pinned host key** → the responder rejects the
  connection (`ERR_READ`); the device itself never reports success.
- Device static key not present in the responder's registry → rejected.
- Single flipped bit in message 1 (ephemeral or payload tag) → rejected.
- Single flipped bit in message 2 → rejected; the endpoint is thereafter
  permanently unusable (further calls return `ERR_MODE`), i.e. fail-closed.

### 17.5 Bearing on this design

- §5.5's recommended WASM `snow` path is **feasible as specified**, on the exact
  suite; the `Noise_KK_25519_ChaChaPoly_BLAKE2b` fallback is not needed.
- §8's "pins supplied before the handshake" requirement is implemented and
  exercised by the spike (results §2: "identity is pre-known, not discovered").
- No pattern or cipher suite was changed to make the spike pass.

### 17.6 Not covered (explicit scope limit)

The spike is a feasibility proof, not Stage 2. It does **not** cover the pair HPKE
flow, the relay envelope, `wss://`/rustls transport, the application frames, the
CSP policy, integration with the Tauri app, or production key storage. Those remain
Stage 2 and stay gated by the §5.7 interop vectors.

---

Stage 2 (host tunnel, relay fallback removal, client-origin move, frontend
integration) will not begin until this revision is approved or amended.
