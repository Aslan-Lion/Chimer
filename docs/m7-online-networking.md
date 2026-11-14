# app.timer — Milestone 7: Online Networking

**Document status: design, with Stage 1 (relay hardening) complete.** This
document is the "document the proposed relay architecture and identify any
protocol changes required" step required by the M7 brief. It records what will
be built, what will deliberately **not** change, and the exact trust boundaries
the implementation must respect.

The relay crate described here is implemented. The **Windows host online engine
and the remote-side integration are not** — they are Stage 2 and beyond, and
they wait for the security design in
[`m7-e2e-security-design.md`](./m7-e2e-security-design.md) to be reviewed and
approved.

Claims in this document are design intent unless marked otherwise. Verified
results are labelled `implementation-complete`, `build-verified`,
`deployment-verified` or `physically-tested`. **Nothing has been deployed.**

---

## 1. What M7 adds, and what it must not touch

M6 is a **purely local** networking layer:

- the host runs a blocking `std::net::TcpListener` accept loop;
- the remote browser is served the frontend bundle by that same server (`serve_asset`);
- the remote talks plain `ws://` to `<host-lan-ip>:8722/ws`;
- every timer decision is made in Rust (`TimerEngine`, `SessionEngine`);
- the wire vocabulary lives in `src-tauri/src/network/protocol.rs`.

M7 adds a second, **internet** transport to the same host engine. It does not add a second timer
engine, a second command pipeline, a second device registry, a second remote application, or a
second protocol vocabulary.

Guiding rule for the whole milestone:

> **The relay is a dumb, untrusted pipe. The Windows host stays the only authority.**

Concretely, the relay must be replaceable by an attacker without granting them any authority over
the timer. It *does* see the plaintext of the frames it forwards — including device tokens and
pairing codes — so the confidentiality of that content is a separate problem, addressed by
[`m7-e2e-security-design.md`](./m7-e2e-security-design.md). See §9.5.

---

## 2. Topology

```
 ┌──────────────────────────────┐
 │  Remote browser              │   phone / tablet / laptop
 │  (app.timer remote app)      │   on any network, e.g. mobile data
 └──────────────┬───────────────┘
                │  WSS  (TLS 1.2/1.3, certificate validated by the browser)
                │  wss://<relay>/ws/device/<hostId>
                ▼
 ┌──────────────────────────────┐
 │  Online relay service        │   Rust. Axum + axum-server + rustls.
 │                              │   Separate deployable binary. Stateless.
 │  · terminates TLS            │   No timer logic. Nothing on disk.
 │  · authenticates hosts       │   (RELAY_HOSTS credentials)
 │  · routes opaque payloads    │
 │  · rate limits + size limits │
 │  · serves the remote SPA     │
 │  · /healthz                  │
 └──────────────┬───────────────┘
                │  WSS  (TLS, certificate validated by the host process)
                │  wss://<relay>/ws/host        ← OUTBOUND from the host
                ▼
 ┌──────────────────────────────┐
 │  Windows app.timer host      │   Tauri 2 desktop application
 │                              │
 │  network/online/  (planned)  │   relay client + route table
 │        │                     │
 │        ▼                     │
 │  network/engine.rs (M6)      │   the ONE command pipeline
 │        │                     │
 │        ▼                     │
 │  TimerEngine / SessionEngine │   the ONE source of truth
 └──────────────────────────────┘
```

Key properties this shape buys:

| Requirement | How it is met |
| --- | --- |
| No port forwarding, no public IP, no inbound firewall rule | The host dials **out** to the relay and holds the socket open. Nothing ever connects *to* the host from the internet. |
| Host is not a public server | No listener is bound for the online path. The only host listener remains the M6 LAN one. |
| Relay is not a timer authority | The relay never parses a timer command. It forwards JSON it does not understand. |
| Relay cannot be a second source of truth | It stores nothing on disk: no tokens, no codes, no timer state, no queue. A relay restart drops tunnels; clients reconnect and re-sync from the host. |
| Local M6 keeps working with no internet | The online engine is independent; if it cannot reach the relay it reports an error and the LAN listener is untouched. |

---

## 3. Components

### 3.1 Relay service (crate `relay/`) — `implementation-complete`

A standalone Rust binary, **not** part of the Tauri executable and not linked into it. It is built
from its own crate with its own `Cargo.toml` and `Cargo.lock`, so it can be versioned, built and
deployed on its own cadence.

Technology choice: **axum 0.8 + axum-server 0.9 (the `tls-rustls-ring` feature, i.e. rustls) +
tokio**, with `tower-http`'s `ServeDir` for the static SPA. That is the smallest well-tested stack
for "terminate TLS, speak RFC 6455 to many clients, multiplex". Rationale and alternatives are in
§13. `tokio-tungstenite` appears only as a **dev-dependency**: the end-to-end tests use it as a
client. The relay itself speaks WebSocket through `axum::extract::ws`, so no hand-rolled RFC 6455
codec exists in the relay.

Module layout:

```text
relay/src/main.rs      entry point, graceful shutdown, maintenance pruning
relay/src/lib.rs       state(), serve(), the documented trust boundary
relay/src/config.rs    environment parsing + validation (nothing defaulted dangerously)
relay/src/tls.rs       rustls server configuration, loaded from PEM
relay/src/wire.rs      the relay/1 envelope types and codec limits
relay/src/registry.rs  host table, route table, opaque payload routing
relay/src/limits.rs    sliding-window rate limiting
relay/src/http.rs      router: /healthz, /ws/host, /ws/device/{hostId}, SPA fallback
relay/src/logging.rs   structured, secret-free JSON logging
```

Responsibilities, and nothing else:

1. HTTPS/WSS listener with a TLS certificate from configuration.
2. Host handshake: authenticate the host against env-provisioned credentials; register its route
   (one host id ⇒ at most one live host socket).
3. Device handshake: resolve `<hostId>`, allocate an unguessable per-connection `route` id, tell
   the host a device arrived.
4. Forward `data` envelopes verbatim between a route and its host.
5. Enforce connection limits, message-size limits, and rate limits.
6. Serve the built remote SPA (`dist/`) over HTTPS so the phone has something to load.
7. Serve `/healthz`.
8. Log operational events **without secrets or payloads**.
9. Shut down gracefully on SIGINT/SIGTERM, telling connected devices why.

### 3.2 Host online engine (planned, `src-tauri/src/network/online/`)

Lives inside the existing Tauri process, beside `NetworkEngine`.

- **Transport**: a blocking `rustls` client over `std::net::TcpStream`
  (`rustls::StreamOwned<ClientConnection, TcpStream>`), which implements `Read + Write`. That
  means the *existing* M6 codec in `transport.rs` (`read_request`, `write_websocket_handshake`,
  `read_frame`, `write_text`, `write_ping`, `write_pong`, `write_close`) is reused unchanged for
  the online leg. No second frame codec is written for the host.
- **TLS verification**: `rustls-platform-verifier` against the **operating system trust store**.
  There is no "accept invalid certificate" switch anywhere in the codebase, and no code path that
  can construct a client config with a no-op verifier.
- **Lifecycle**: one supervisor thread that owns the outbound socket, reconnects with bounded
  exponential backoff, and on reconnect re-registers and re-syncs the host state.
- **Route table**: `routeId -> local connection id`, plus the reverse, so the engine's existing
  `send`/`close_connection` reach a tunnel connection exactly like a LAN one.

### 3.3 Remote application (planned, extended not duplicated)

`src/services/remote.ts` already owns connection, pairing, credentials, heartbeat, bounded
backoff (`min(500 * 2^(n-1), 15000)` ms) and revision-gated snapshot reconciliation. M7 teaches
**that same singleton** about a second endpoint. `RemoteApp.tsx`, `RemoteController.tsx` and
`RemoteDisplay.tsx` are unchanged in structure; no second remote application is created.

---

## 4. Relay wire protocol (`relay/1`) — `implementation-complete`

A small envelope protocol that **wraps** the existing M6 application messages. It is versioned
separately (`"protocol":1`) from the application protocol (`PROTOCOL_VERSION = 1`).

All frames are RFC 6455 **text** frames. Unknown `t` values are rejected with
`{"t":"error","code":"malformed"}`. Every frame is JSON with a `t` discriminator. Envelope tags are
kebab-case; field names are camelCase.

**Keep-alive is done with WebSocket control frames**, not JSON: the relay sends `ping` and every
conforming client answers `pong` automatically, alongside an idle read timeout. There is no JSON
`ping`/`pong` envelope and no `heartbeatMs` field. The application protocol's own
`{"type":"ping"}` travels through as an ordinary `data` payload.

### 4.1 Host leg — `wss://<relay>/ws/host`

Client → server:

```jsonc
{ "t": "hello", "protocol": 1, "hostId": "h-…", "hostToken": "…" }
{ "t": "data",  "route": "<routeId>", "payload": { /* M6 ServerMessage */ } }
{ "t": "close", "route": "<routeId>", "reason": "revoked" }   // reason optional
```

Server → client:

```jsonc
{ "t": "ready",  "protocol": 1, "hostId": "h-…" }
{ "t": "open",   "route": "<routeId>", "peer": { "address": "203.0.113.9" }, "userAgent": "…" }
{ "t": "data",   "route": "<routeId>", "payload": { /* M6 ClientMessage */ } }
{ "t": "closed", "route": "<routeId>", "reason": "peer-gone" }
{ "t": "error",  "code": "…", "message": "…" }                // then the relay closes
```

### 4.2 Device leg — `wss://<relay>/ws/device/<hostId>`

Client → server:

```jsonc
{ "t": "hello", "protocol": 1 }
{ "t": "data",  "payload": { /* M6 ClientMessage */ } }
```

Server → client:

```jsonc
{ "t": "ready",    "protocol": 1, "route": "<routeId>", "hostId": "h-…", "hostOnline": true }
{ "t": "data",     "payload": { /* M6 ServerMessage */ } }
{ "t": "shutdown", "reason": "The relay is shutting down." }
{ "t": "error",    "code": "host-offline" | "unauthorized-host" | "rate-limited"
                  | "too-large" | "malformed" | "unsupported-protocol" | "shutting-down",
  "message": "…" }
```

The route id is 32 CSPRNG bytes, base64url without padding. It is the *only* thing that lets a
device's traffic be addressed; it is never reused and never derived from the host id. Another
device cannot address someone else's route without guessing 256 bits.

### 4.3 Plain HTTP on the same port

```text
GET /healthz   → 200 {"status":"ok","protocol":1,"hosts":N,"devices":M,"uptimeSeconds":S}
GET /          → the remote SPA (index.html)
GET /assets/…  → the built assets (ServeDir fallback)
```

`/healthz` exposes counts and uptime only — no host ids, no tokens, no timer content. A request for
an unknown path is a plain `404`.

### 4.4 Error codes

`malformed`, `too-large`, `unsupported-protocol`, `unauthorized-host`, `host-conflict`,
`unknown-host`, `host-offline`, `rate-limited`, `limit-exceeded`, `shutting-down`, `internal`.

These are relay-level codes and are deliberately disjoint from the M6 `ProtocolError` codes
(`unauthenticated`, `forbidden`, `timer-active`, …) so the two layers never look alike in logs or
in the remote UI. A unit test asserts the disjointness.

---

## 5. Application protocol changes: **none**

This is the most important line in the document.

> **`src-tauri/src/network/protocol.rs` is not modified by M7.**

`ClientMessage` (`hello`/`pair`/`authenticate`/`command`/`snapshot`/`ping`) and `ServerMessage`
(`welcome`/`pairing-pending`/`paired`/`pairing-rejected`/`device-updated`/`revoked`/`library`/
`snapshot`/`state`/`presentation`/`command-result`/`error`/`pong`) travel through the relay
unchanged. `PROTOCOL_VERSION` stays `1`. `RemoteAction` keeps its nine actions, including M6's
`timer.create`.

**Why this is possible:** the online transport terminates in a *virtual connection* inside
`NetworkInner.connections`. The engine cannot tell a tunnel connection from a LAN socket, so
`handle_message`, `handle_pair`, `handle_authenticate`, `handle_command`, `execute`,
`broadcast_state`, `send_initial_state`, `close_device_connections` and `authenticated_targets`
are all reused with no branching on transport — except in the two places where a branch is
genuinely required (which pairing registry to consult, and how to shut the socket down).

### 5.1 The only host-side source changes, and why each is required (planned)

| Change | Reason | Additive? |
| --- | --- | --- |
| `enum Closer { Local(TcpStream), Tunnel(mpsc::Sender<Outbound>) }` replaces `Connection.closer: TcpStream` | `close_connection` must be able to end a virtual connection. `Closer::Tunnel` enqueues `Outbound::Close`, which the tunnel writer turns into `{"t":"close"}`. | Yes — the local variant behaves exactly as today. |
| `enum Outbound` gains `Close` | The signal a tunnel connection is finished. Never sent to a local socket (the local writer simply treats it as a terminal frame). | Yes |
| `Connection` gains `transport: Transport` and `peer_label: String` | So the Device Manager can distinguish local from online, and so `handle_pair` knows which `PairingRegistry` to use. | Yes |
| `NetworkInner` gains `online_pairing: PairingRegistry`, `online_pairing_limiter: RateLimiter`, `online_routes: HashMap<String, String>` | A second, independent pairing-code surface (see §6) and route bookkeeping. | Yes |
| `DeviceView` gains `transport: "local" \| "online"`, `PairingRequest` gains `transport` | UI/IPC shape only. | Yes |
| `NetworkEngine` gains `register_tunnel_connection`, `tunnel_connection_closed`, `online_pairing_code`, `approve_online_pairing` | The seam the online engine calls. All of them funnel into existing private code. | Yes |

There are **no changes** to the device registry, the token format, the token hashing, the
permission model, the command-idempotency memory, the command rate limiter, or the revision
bookkeeping.

### 5.2 IPC and event additions (planned; not wire-protocol changes)

New Tauri commands (mirroring the existing `network_*` surface):

```text
online_status            online_configure(enabled, relayUrl, hostId, hostToken?)   online_refresh
online_connect           online_disconnect        online_reconnect
online_issue_pairing     online_pending_requests  online_approve_pairing  online_reject_pairing
online_reveal_host_token
```

New event channels in `src-tauri/src/events/mod.rs`, in the same style as the M6 trio:

```text
online://status-changed     online://pairing-changed
```

`network://devices-changed` is reused unchanged — online devices appear in the same list, tagged
with `transport`, so the Device Manager stays one component.

### 5.3 Frontend additions (planned)

| File | Change |
| --- | --- |
| `src/types/network.ts` | Online status/pairing types, `transport` on `DeviceView`/`PairingRequest`. No change to `ClientMessage`/`ServerMessage`. |
| `src/services/network.ts` | Online IPC wrappers + `onOnlineStatus`/`onOnlinePairing` listeners. |
| `src/stores/networkStore.ts` | Online slice + actions, same `run()` pattern. |
| `src/services/remote.ts` | Endpoint selection (`local` \| `online`), relay URL parsing from the URL fragment, route through the same socket lifecycle. |
| `src/components/networking/OnlineNetworkingCard.tsx` | New; mounted in `Settings.tsx` directly under `NetworkingSettingsCard`. |
| `src/components/networking/DeviceManagerCard.tsx` | Local/online badges and filtering. |

---

## 6. Pairing workflow (online)

The M6 pairing machinery is reused; only the code's *surface* is separate.

1. Operator enables **Online networking** in Settings and supplies the relay URL.
2. The host supervisor connects out to `wss://<relay>/ws/host` and sends `hello` with its host id
   and relay credential. On `ready`, Settings shows "Connected to relay".
3. The operator presses **Show online pairing code**. The host issues a code from the *online*
   `PairingRegistry` (6 digits, CSPRNG, `PAIRING_TTL_MS = 120 s`, single-use, one live code at a
   time) and renders a QR encoding
   `https://<relay-web-host>/#host=<hostId>&pair=<code>`.
   The code travels in the **fragment**, exactly as M6 already does, so the browser never sends it
   to the relay as part of a request.
4. The phone scans the QR, loads the SPA **from the relay**, and opens
   `wss://<relay>/ws/device/<hostId>`.
5. The relay allocates a route and sends `open` to the host.
6. The host registers a virtual connection marked `Transport::Online` and sends
   `ServerMessage::Welcome` — the same welcome a LAN device gets.
7. The phone sends `ClientMessage::Pair { code, name }`. The host validates it against the
   **online** `PairingRegistry`, under the **online** pairing rate limiter, and emits
   `ServerMessage::PairingPending`.
8. The host emits `online://pairing-changed`; Settings shows the request in the Online section.
9. The operator approves **as controller** or **as display**. The host issues a device token
   (`random_token(24)`, stored only as SHA-256), sends `ServerMessage::Paired`, and records the
   device in the shared `DeviceRegistry`.
10. The phone stores `{deviceId, token}` in `localStorage` and sends `authenticate`.
11. The host replies with the authoritative `Snapshot` (and `Library`), and from then on
    `State`/`Presentation` broadcasts reach it exactly like a LAN device.

**A device that merely knows the relay URL and the host id gets nothing.** It still needs a
single-use code plus a human approval, and on the next connect it needs a token whose hash is
already in the host's device registry. The relay cannot mint either.

Why a *separate* online `PairingRegistry` rather than sharing the LAN one: the brief requires the
two modes to be independently enable/disable-able and independently configurable. A shared
registry would mean issuing a LAN code silently kills an open online code (and vice versa), which
is a surprising coupling. The two registries have identical rules, identical TTLs, identical
single-use semantics and identical brute-force limiter shapes; only the bucket is separate.

An already-paired device is never re-paired: it authenticates directly with its stored token.
A device paired over LAN can authenticate over the relay and vice versa — the credential is the
identity, not the transport.

---

## 7. Connection modes and duplicate suppression

`NetworkEngine` keeps the M6 local mode. `OnlineEngine` owns the online mode. Each has its own
`enabled` flag, its own configuration and its own status. Neither reads the other's state, so:

- online disabled or unreachable ⇒ local mode and the desktop timer are unaffected;
- local disabled ⇒ the relay tunnel is unaffected.

On the **remote** side there is at most **one live socket at a time**, so a device can never
generate duplicated commands by holding two transports. Selection order:

1. If the page URL carries `#host=<hostId>` (or `#relay=<url>`), the device is in **online** mode
   and uses the relay.
2. Otherwise the page was served by the host over the LAN, so the device uses **same-origin**
   `ws(s)://<location.host>/ws` — i.e. exactly M6 behaviour.
3. A device in online mode that fails to reach the relay falls back to the LAN endpoint **only**
   if it learned one, and vice versa; the previous socket is always closed before the new one is
   opened.

The stored credentials are shared between modes, so a fallback never costs a re-pairing.

---

## 8. Reliability

| Failure | Behaviour |
| --- | --- |
| Host loses internet | Supervisor socket errors → `state: reconnecting`, backoff. LAN mode and the desktop timer are untouched. |
| Remote loses internet | Browser socket closes → existing backoff → on reopen: `hello` → `authenticate` → `snapshot`. |
| Relay restarts | Host reconnects and re-registers; devices reconnect and re-authenticate. All routes are new; nothing is resumed from stale state. |
| Host app restarts | Host re-registers with the same persisted host id; devices reconnect and authenticate with their stored tokens; the timer is restored from the host's own persistence, then broadcast. |
| Browser refresh | Credentials come from `localStorage`; pairing is not repeated. |
| Auth expired / revoked | Host answers `unauthenticated`/`revoked`; the remote drops stored credentials and shows the pairing screen. Revocation deletes the hash, so an old token cannot be replayed through the relay either. |
| Wi-Fi ⇄ mobile data | Both legs are ordinary TCP reconnects; the IP change is irrelevant to the relay (the route id is the identity, not the address). |
| Many devices at once | Relay caps devices per host (`RELAY_MAX_DEVICES_PER_HOST`); the host caps its own accepted connections; both count independently. |

**Stale state is never trusted.** Every `state`/`presentation`/`library` message is revision-gated,
and every (re)connect ends in a full `snapshot` computed by the host at that moment. The remote
holds no independent countdown — this is unchanged from M6.

**Commands never execute twice.** M6's per-device command-id memory (`COMMAND_MEMORY = 64`) is the
mechanism, and it sits *above* the transport, so it already covers relay retries: the host feeds
every inbound `command` through `handle_command` step 3, which replays the cached result instead of
re-executing. M7 adds no relay-level dedup because none is needed — the identity that matters
(device + command id) is in the payload, which the relay forwards verbatim.

---

## 9. Security model

### 9.1 Authentication layers

| Layer | Who proves what | Secret | Where verified |
| --- | --- | --- | --- |
| TLS (host leg) | Host proves it is talking to the configured relay | relay certificate | **host**, against the OS trust store |
| TLS (device leg) | Browser proves it is talking to the configured relay | relay certificate | **browser**, standard TLS validation |
| Relay host auth | Host proves it may register that host id | relay host credential (env-provisioned) | relay, constant-time compare |
| Device pairing | Human + a single-use code | 6-digit code, 120 s, one live | host, constant-time compare, rate limited |
| Device identity | Device proves it was paired | 24-byte device token, stored host-side only as SHA-256 | host, constant-time compare |
| Authorisation | — | — | host, per-action permission check before `execute` |

Every existing M6 check survives verbatim: role defaults (a `display` device has an empty
permission set, so it is read-only *by construction*), per-action permission enforcement, the
command rate limiter keyed to the persistent device id, and the revoked-device re-check.

### 9.2 Rate limits

| Bucket | Where | Limit |
| --- | --- | --- |
| Pairing attempts (LAN) | host | 5 / 5 min per address — unchanged |
| Pairing attempts (online) | host | 5 / 5 min per route |
| Commands | host | 20 / 1 s per **device id** — unchanged |
| Host registrations | relay | per source address |
| Device connections | relay | per source address, and per host |
| Data frames | relay | per route, per direction |

The command limiter stays keyed to the device id, not the socket, so a device cannot reset its
budget by reconnecting — the same property M6 established.

### 9.3 Attack surfaces the relay closes

- **Relay impersonation (host side):** TLS with real validation; no bypass flag exists.
- **Unauthorised host registration:** relay rejects a `hello` whose `hostId`/`hostToken` pair is
  not in the configured credential set, and rejects a second live socket for a host id already
  registered (`host-conflict`).
- **Unauthorised device registration:** there is no such thing at the relay. A device socket
  grants *transport*, never authority.
- **Route hijacking:** route ids are 256-bit CSPRNG, allocated per connection, never logged as
  anything but an opaque short prefix, and a host can only send to a route it was told about.
- **Oversized payloads:** relay enforces a frame cap *before* buffering (via axum's
  `max_frame_size`), and the host re-enforces `MAX_MESSAGE_BYTES` on the inner message. A relay
  cannot use size as a memory-exhaustion vector against the host.
- **Message flooding:** relay per-route and per-address rate limits; host command limiter.
- **Cross-site WebSocket hijack (device side):** the browser leg terminates at the relay, so the
  relay applies the same `Origin` policy M6's `origin_is_allowed` implements — the relay's own
  origin and configured extra origins are allowed, others refused. An absent `Origin` (a
  non-browser client) is allowed through, because it must still pair and authenticate.
- **Arbitrary remote access:** there is no message type that reads a file, runs a command, or
  touches anything outside the timer/session/presentation vocabulary. The relay cannot invent one,
  and the host would reject an unknown action with `unknown-action` if it tried.

### 9.4 Logging discipline

Never logged, at any level, by either side: device tokens, pairing codes, host credentials, relay
credentials, timer names, preset/session names, or message payloads. The logger *drops* any field
whose name matches a secret-like fragment (`token`, `code`, `credential`, `secret`, `password`,
`authorization`, `payload`) even if a caller passes one by mistake. Logged: timestamps, event
kind, host id, opaque route prefix (first 6 chars), peer address, connection counts, error codes,
durations.

### 9.5 Trust boundaries — stated plainly

The relay **terminates TLS on both legs**. It therefore holds the plaintext of every forwarded
frame while it is in flight, and it can read, drop, delay, reorder or alter any of it.

Therefore:

- **This is not end-to-end encryption.** WSS protects each hop, not the path. Nothing in this
  document or the implementation claims otherwise.
- The relay can observe: which host ids are online, when, from which IPs; which devices connect
  and when; the volume and timing of traffic; and — because it forwards application JSON verbatim
  — the *content* of timer commands and the pairing codes and **bearer device tokens** travelling
  in them.
- **A relay operator, or anyone who compromises the relay, can therefore steal a device bearer
  token and replay it to impersonate that device.** The relay cannot *mint* a token and cannot
  approve a pairing, but a captured token is a usable credential for as long as it remains valid.
  This is a real limitation of a TLS-terminating relay and it is **not** mitigated by anything
  described in this section.
- What the relay still **cannot** do, by design: read the device registry, read persisted timer
  state (state flows host → device and the relay's copies are transient), or act as a second
  source of truth.
- Operational consequence, documented rather than hidden: **the relay operator is trusted with
  confidentiality.** Anyone who must not see timer content or device credentials should self-host
  the relay, and should treat the mitigation below as required rather than optional.

> **Mitigation (designed, not implemented).** An application-layer end-to-end seal between the
> Windows host and each remote device — authenticated encryption with the relay forwarding opaque
> ciphertext, credentials encrypted for the intended device, and replay/impersonation protection —
> is designed in [`m7-e2e-security-design.md`](./m7-e2e-security-design.md). It is **out of scope
> for Stage 1** and is not present in this code. The Windows host tunnel and the remote-side
> integration will not be built until that design is reviewed.

### 9.6 What is deliberately *not* built

No remote shell, no filesystem access, no screen access, no arbitrary desktop data, no relay-side
persistence of anything, no cloud timer execution, no relay-side timer state. The relay's disk
footprint is zero: configuration and credentials come from the environment at start-up and live
only in memory.

---

## 10. Configuration and persistence

### 10.1 Host: `online-settings.json` (planned, beside `network-devices.json`)

```jsonc
{
  "version": 1,
  "enabled": false,
  "relayUrl": "",          // e.g. "wss://relay.example.com" — supplied by the operator
  "hostId": "h-…",         // generated locally on first enable, CSPRNG, not a secret
  "hostToken": null        // relay credential; omitted when supplied via the environment
}
```

Rules: written with the same temp-file-plus-rename discipline as M6; never contains a device token
or a pairing code; `hostToken` is omitted entirely when the environment variable
`APP_TIMER_RELAY_TOKEN` is set, and the environment always wins over the file. The file is only
read to reconnect; it is never logged.

The device registry is **not** duplicated online — it stays the single M6 `network-devices.json`.

### 10.2 Relay: environment only

Everything is env-provisioned; see [`relay/README.md`](../relay/README.md) for the full table and
[`relay/.env.example`](../relay/.env.example) for a copyable template.

```text
RELAY_BIND=0.0.0.0:8443
RELAY_TLS_CERT=/etc/relay/tls/fullchain.pem
RELAY_TLS_KEY=/etc/relay/tls/privkey.pem
RELAY_HOSTS=h-abc123:<credential>,h-def456:<credential>
RELAY_WEB_ROOT=/srv/app-timer/dist
RELAY_MAX_HOSTS=64
RELAY_MAX_DEVICES_PER_HOST=32
RELAY_MAX_FRAME_BYTES=16384
RELAY_MAX_PAYLOAD_BYTES=8192
RELAY_DATA_FRAMES_PER_SEC=40
RELAY_CONN_PER_MIN=30
RELAY_ALLOWED_ORIGINS=
RELAY_LOG=info
RELAY_ALLOW_INSECURE=0
```

**No production domain, certificate or credential is hardcoded anywhere in the code.** The example
file ships with empty/placeholder values and the service refuses to start without TLS material —
unless `RELAY_ALLOW_INSECURE=1` **and** the bind address is loopback, which exists only so a
developer can run the whole thing on one machine. A non-loopback insecure bind is a hard start-up
error, and there is a test asserting it.

---

## 11. Server-side product surfaces (planned)

### 11.1 Host Settings — the Online Networking card

Sits directly under the existing Local Networking card in `Settings.tsx`, using the same UI
primitives (`Card`, `Toggle`, `NumberField`, `Button`, `Badge`):

- Enable / disable online networking.
- Relay server URL (text field) with save/connect.
- Connection status: Disabled / Connecting / Connected / Reconnecting (n) / Error, plus
  connected-since.
- Host registration status and the **host identifier** (copyable).
- Online pairing QR code + 6-digit code + expiry countdown (reusing the M6 pairing panel layout
  and `qrcode` rendering).
- Pending online pairing requests with Approve as controller / As display / Reject.
- Clear connection and authentication errors, with operator guidance keyed to the relay error
  codes.
- Reconnect and Disconnect actions.

### 11.2 Host Device Manager

One list, unchanged component, with a `Local` / `Online` badge per device and a coarse
"where it is" line (LAN address for local; relay-reported peer address for online). Permissions,
role editing, revocation and disconnect keep working identically for both, because the underlying
`DeviceRegistry` is shared.

### 11.3 Remote application

- Online host selection: the QR fragment carries `host` (and optionally `relay`); a manual entry
  form lets a user type a relay URL + host id + code when they cannot scan.
- Pairing screen: unchanged flow, with the transport named ("Connecting through the relay").
- Controller and display: unchanged.
- Reconnect: existing bounded backoff, extended to re-resolve the endpoint when a fallback exists.
- Reconciliation: full `snapshot` after every reconnect.
- Errors: relay-level codes get their own guidance strings ("The host is offline", "The relay
  refused this connection", …), kept distinct from M6 device errors.

---

## 12. Test plan

Unit and integration tests are added on both sides; the existing M1–M6 suites must stay green.

**Relay (`cargo test` in `relay/`) — `implementation-complete`**

The end-to-end suite (`relay/tests/end_to_end.rs`) drives the real listener on an ephemeral
loopback port, so the router, both WebSocket upgrades, the two session state machines and the
routing table are exercised together over real sockets. Client frames are written as literal JSON
so a change to the Rust types cannot make the tests agree with a bug. Covered: host registration
with a good/bad/unknown credential, duplicate host rejection, malformed first frame, device
routing in both directions, host isolation, unknown host, host offline, malformed host id,
route close on device departure, multiple routes, connection rate limiting, shutdown notice,
the health check, and 404s. Unit tests cover the envelope codec, configuration validation, TLS
material loading, origin policy, logging redaction and the rate limiter.

**Host (`cargo test`) — planned**

- relay envelope encode/decode round-trip; unknown `t` refused; oversized frame refused before buffering
- relay URL validation (scheme, host, port, path normalisation); `ws://` permitted only for loopback
- **TLS certificate validation**: an in-process rustls server with a self-signed certificate must be
  *refused* by the host client config
- host registration/authentication: correct credential accepted, wrong credential refused, duplicate
  host id refused, constant-time comparison used
- online pairing: issue → claim → approve; expiry; wrong code; replay of a used code; duplicate claim
- the online pairing registry and the local one do not interfere
- device revocation over a tunnel ends the virtual connection and refuses re-authentication
- permission enforcement on a tunnel connection (display device cannot command; missing
  `timerCreate` refused)
- remote `timer.create` over a tunnel reaches the existing engine guard and is refused while a
  timer is live
- command idempotency across a simulated reconnect (same command id twice ⇒ executed once)
- multiple devices: `broadcast_state` reaches every authenticated tunnel connection, and state is
  revision-gated
- snapshot reconciliation: a fresh tunnel connection receives a full snapshot with the host's revision
- relay disconnection and recovery: the supervisor reconnects and re-registers
- invalid/unauthorised relay frames: bad envelope, unknown route, data for a dead route

**Verification commands**

```text
cargo test            (src-tauri)
cargo build           (src-tauri)
npm run typecheck
npm run build
npm run tauri build
cargo test ; cargo build --release   (relay/)
```

---

## 13. Deployment approach

- **Artifact:** one static Linux binary (`cargo build --release` in `relay/`), plus the deployment
  artefacts now present in `relay/`:
  - [`relay/Dockerfile`](../relay/Dockerfile) — multi-stage: Node builds `dist/`, Rust builds the
    relay, and the final image contains only the stripped binary, `dist/`, and an unprivileged
    user. No secrets are baked in.
  - [`relay/docker-compose.yml`](../relay/docker-compose.yml) — a single-VM deployment descriptor
    that reads `relay.env` and mounts `./tls` read-only.
  - [`relay/.env.example`](../relay/.env.example) — every variable, with placeholder values only.
  - [`relay/README.md`](../relay/README.md) — local development and production instructions,
    including how to obtain a certificate.
- **TLS:** terminated by the relay itself from `RELAY_TLS_CERT`/`RELAY_TLS_KEY`, so a bare VM with
  a certificate works with no extra moving parts. If the operator already runs a reverse proxy,
  the relay can sit behind it and `RELAY_PUBLIC_ORIGIN` pins the same-origin check to the public
  origin.
- **No hosting provider is chosen.** No domain, certificate, cloud account or paid service is
  assumed to exist. The documentation explains how to obtain a certificate and run the binary, and
  is explicit that **nothing has been deployed by this milestone**.
- **Local development:** run the relay on `127.0.0.1` with either a `mkcert`-style certificate
  trusted by the OS (preferred, so the host's real TLS validation is exercised) or
  `RELAY_ALLOW_INSECURE=1` for the loopback shortcut, then point the host's relay URL at it.

---

## 14. Files created and modified

**Relay — created and implemented (`implementation-complete`)**

```text
relay/Cargo.toml, relay/Cargo.lock
relay/src/main.rs                         entry point, config, graceful shutdown
relay/src/lib.rs                          state(), serve(), trust boundary
relay/src/config.rs                       environment parsing + validation
relay/src/tls.rs                          rustls server configuration
relay/src/wire.rs                         relay/1 envelope types + codec limits
relay/src/registry.rs                     host table, route table, opaque routing
relay/src/limits.rs                       sliding-window rate limiting
relay/src/http.rs                         /healthz + /ws routes + SPA fallback
relay/src/logging.rs                      structured, secret-free logging
relay/tests/end_to_end.rs                 end-to-end suite over real sockets
relay/Dockerfile                          multi-stage image build
relay/docker-compose.yml                  single-VM deployment descriptor
relay/.env.example                        every variable, placeholders only
relay/README.md                           local dev + production deployment
docs/m7-online-networking.md              this document
docs/m7-e2e-security-design.md            end-to-end security design (design, not built)
```

**Host and frontend — planned, not started (Stage 2+)**

```text
src-tauri/src/network/online/mod.rs       OnlineEngine, status, settings, supervisor
src-tauri/src/network/online/envelope.rs  relay/1 mirror for the host
src-tauri/src/network/online/client.rs    blocking rustls WSS client + TLS config
src-tauri/src/network/online/tunnel.rs    route table, virtual connections, reconnect
src-tauri/src/network/online/persistence.rs  online-settings.json
src/components/networking/OnlineNetworkingCard.tsx
src-tauri/Cargo.toml                      + rustls, rustls-platform-verifier, rustls-pemfile, webpki-roots
src-tauri/src/lib.rs                      manage OnlineEngine; register online_* commands
src-tauri/src/events/mod.rs               online:// channel names
src-tauri/src/network/mod.rs              pub mod online; document the two transports
src-tauri/src/network/engine.rs           Closer enum, Transport, tunnel seam methods, online pairing
src-tauri/src/network/model.rs            Transport enum; DeviceView/PairingRequest transport field
src-tauri/src/network/commands.rs         online_* Tauri commands
src-tauri/src/network/qr.rs               online pairing URL builder
src/types/network.ts                      online types; transport on device/request views
src/services/network.ts                   online IPC wrappers + listeners
src/stores/networkStore.ts                online slice + actions
src/services/remote.ts                    online endpoint selection + relay URL parsing
src/pages/Settings.tsx                    mount OnlineNetworkingCard
src/components/networking/DeviceManagerCard.tsx   local/online distinction
```

**Explicitly not modified:** `src-tauri/src/network/protocol.rs`, `devices.rs`, `tokens.rs`,
`rate_limit.rs`, `transport.rs`, and every timer/session/display module.

---

## 15. Carried-over housekeeping flagged with this document

The temporary M6 diagnostics ("Remove this module, and every call to it, once LAN pairing has been
confirmed on real hardware") are still present in `src-tauri/src/network/engine.rs`
(`mod diagnostics`) and `src/services/remote.ts` (five `console.info`/`console.warn` calls). M7
will extend the same diagnostic channel to the online leg while it is being verified, and the
removal decision is raised with the operator rather than taken silently — removing them now would
delete the only evidence trail for the online path at exactly the moment it needs one.

---

## 16. Assumptions this design makes

1. The relay is operated by the same person who operates the host (self-hosted), or by someone
   they trust with confidentiality — see §9.5. Nothing in the design depends on this being true for
   *authority*, but confidentiality and replay resistance do (see the security design).
2. The Windows host has outbound TCP access to the relay's port. A network that blocks all
   outbound traffic cannot use M7 (and would not be able to use any relay-based design).
3. A certificate for the relay's domain can be obtained by the operator. Obtaining one is
   documented, not automated, and no certificate is bundled.
4. The remote SPA is served by the relay in the default deployment, so the phone has a URL to load.
   `#relay=<url>` also lets the operator serve the same bundle from any static host instead.
