# app.timer online relay

An optional, separately deployable service that lets a paired phone, tablet or
computer reach a Windows **app.timer** host over the internet — across two
different networks — without the host opening a port, holding a public address
or touching a router.

The host dials **out** to the relay and keeps that socket open. Nothing ever
connects *to* the host from the internet, so no port forwarding, public IP or
inbound firewall rule is involved.

---

## What this is, and what it is not

The relay is a **dumb pipe**. It moves opaque JSON objects between one host
socket and the device sockets attached to it, terminates TLS on both legs, and
keeps **nothing** on disk. It has no timer engine, no session state and no idea
what any forwarded message means, so it cannot become a second source of truth
even by accident.

**This is not end-to-end encryption.** TLS ends *at the relay* on both legs, so
the operator of this process can read and alter anything a device sends to a
host and anything a host sends back — including pairing codes and device
tokens. The relay is trusted for the confidentiality of the transport, and is
deliberately **not** trusted with authority over timer state. Read
[`../docs/m7-online-networking.md`](../docs/m7-online-networking.md) section 9.5
for the full trust boundary. An application-layer end-to-end seal is designed
separately in [`../docs/m7-e2e-security-design.md`](../docs/m7-e2e-security-design.md)
and is **not** implemented yet.

What the relay can never do, by design: start a timer, read a preset, mint a
device token, approve a pairing, or read device credentials at rest (device
tokens are hashed on the host; host credentials live only in this process's
memory).

---

## What it serves

| Route | Purpose |
| --- | --- |
| `GET /healthz` | Health check. Returns counts and uptime only — no host ids, no tokens. |
| `GET /ws/host` | The leg the Windows host dials **out** to. Authenticated by the credential in the opening `hello`. |
| `GET /ws/device/{hostId}` | The leg a remote device dials. Knowing `hostId` grants nothing: the host still has to pair the device. |
| everything else | The built remote application (`index.html` plus assets), so a deployment serves the controller and the relay from one origin. |

`/healthz` returns, for example:

```json
{"status":"ok","protocol":1,"hosts":1,"devices":2,"uptimeSeconds":137}
```

Keep-alive uses WebSocket **control** frames (the relay sends `ping`; every
conforming client answers `pong` automatically) plus an idle read timeout. There
are no JSON heartbeat messages on the relay envelope.

---

## Configuration

Everything is read from the environment. Nothing is hardcoded: no domain,
certificate path, credential or host identifier has a working default, and the
service refuses to start without TLS material unless it is explicitly told to
run insecurely *and* is bound to a loopback address.

Copy [`.env.example`](.env.example) to `relay.env` and fill in the placeholders.

| Variable | Meaning | Default |
| --- | --- | --- |
| `RELAY_BIND` | Address and port to bind. | `0.0.0.0:8443` with TLS, `127.0.0.1:8788` insecure |
| `RELAY_TLS_CERT` | PEM certificate chain (leaf first). | — required unless insecure |
| `RELAY_TLS_KEY` | Unencrypted PEM private key matching the certificate. | — required unless insecure |
| `RELAY_HOSTS` | Comma-separated `<host-id>:<credential>` pairs. Credentials must be at least 16 characters. | empty (no host may register) |
| `RELAY_WEB_ROOT` | Directory holding the built remote application. | `dist` |
| `RELAY_PUBLIC_ORIGIN` | Pins the same-origin WebSocket check to exactly this origin. | derived from the request |
| `RELAY_ALLOWED_ORIGINS` | Extra origins allowed to open a device socket. | empty |
| `RELAY_MAX_HOSTS` | Most hosts the relay will route at once. | `64` |
| `RELAY_MAX_DEVICES_PER_HOST` | Most devices per host. | `32` |
| `RELAY_MAX_CONNECTIONS` | Most live connections. | `512` |
| `RELAY_MAX_FRAME_BYTES` | Largest single WebSocket frame accepted. Ceiling `65536`. | `16384` |
| `RELAY_MAX_PAYLOAD_BYTES` | Largest forwarded application payload. May not exceed the frame limit. | `8192` |
| `RELAY_DATA_FRAMES_PER_SEC` | Forwarded frames per route, per direction. | `40` |
| `RELAY_CONN_PER_MIN` | WebSocket handshakes per source address. | `30` |
| `RELAY_LOG` | `error`, `warn`, `info` or `debug`. | `info` |
| `RELAY_ALLOW_INSECURE` | Serve plaintext. **Loopback only** — see below. | off |

### Generating a host credential

```sh
openssl rand -base64 32
```

Put the result in `RELAY_HOSTS` next to the host id the Windows application
shows, for example:

```sh
RELAY_HOSTS=h-example01:9Zr2...long-random-value...
```

The credential is a shared secret between the host and this relay. It travels
only in the host's opening `hello`, is compared in constant time, and is never
logged.

### The insecure switch

`RELAY_ALLOW_INSECURE=1` exists **only** so a developer can run the whole system
on one machine without a certificate. The relay refuses to start with it unless
`RELAY_BIND` is a loopback address; a network-reachable insecure bind is a hard
start-up error. There is no configuration path anywhere that weakens
certificate verification.

`RELAY_ALLOW_INSECURE=1` is not to be used in production. A misconfigured
deployment fails loudly at start-up rather than quietly serving plaintext.

---

## Running it locally (development)

The relay is a standalone Rust binary. The quickest loop is the insecure
loopback shortcut:

```sh
cd relay
RELAY_ALLOW_INSECURE=1 RELAY_BIND=127.0.0.1:8788 \
  RELAY_HOSTS=h-example01:correct-horse-battery-staple \
  RELAY_WEB_ROOT=../dist \
  cargo run
```

This serves plaintext `ws://127.0.0.1:8788` and the built remote application
from `../dist` (run `npm run build` at the repository root first). It is exactly
the shape the end-to-end tests use.

To exercise the **real** TLS path locally, generate a certificate the operating
system trusts — for example with [`mkcert`](https://github.com/FiloSottile/mkcert):

```sh
mkcert -install
mkcert -cert-file relay/tls/fullchain.pem -key-file relay/tls/privkey.pem localhost 127.0.0.1
```

```sh
cd relay
RELAY_BIND=127.0.0.1:8443 \
  RELAY_TLS_CERT=tls/fullchain.pem RELAY_TLS_KEY=tls/privkey.pem \
  RELAY_HOSTS=h-example01:correct-horse-battery-staple \
  RELAY_WEB_ROOT=../dist \
  cargo run
```

Now `https://127.0.0.1:8443/` serves the remote application and the host's real
certificate validation is exercised.

---

## Running it in production

### 1. Obtain a certificate

Any certificate the Windows host and the remote browsers will trust works. The
simplest is Let's Encrypt via `certbot`, which leaves you with:

```text
/etc/letsencrypt/live/<your-host>/fullchain.pem
/etc/letsencrypt/live/<your-host>/privkey.pem
```

They must be readable by the relay process, and the key must be **unencrypted**
PEM. Renewal is a matter of restarting the relay after the certificate is
renewed.

### 2. Run the binary

```sh
cargo build --release        # relay/target/release/app-timer-relay
```

```sh
RELAY_BIND=0.0.0.0:8443 \
  RELAY_TLS_CERT=/etc/letsencrypt/live/<your-host>/fullchain.pem \
  RELAY_TLS_KEY=/etc/letsencrypt/live/<your-host>/privkey.pem \
  RELAY_HOSTS=h-yourhost:<your-host-credential> \
  RELAY_WEB_ROOT=/srv/app-timer/dist \
  ./target/release/app-timer-relay
```

The relay logs one JSON object per line to stdout, which container log drivers
and `journald` already understand. It shuts down gracefully on `SIGINT` or
`SIGTERM`, telling connected devices why before it closes.

Because the relay terminates TLS itself, a bare VM with a certificate needs no
reverse proxy. If you already run one, front the relay with it and set
`RELAY_PUBLIC_ORIGIN` to the public origin so the same-origin WebSocket check
matches exactly.

### 3. Or run the container

From the repository root:

```sh
cp relay/.env.example relay/relay.env     # then edit it
mkdir -p relay/tls                        # put fullchain.pem + privkey.pem here
docker compose -f relay/docker-compose.yml up --build
```

The image is multi-stage: Node builds the remote application, Rust builds the
relay, and the final image contains only the stripped binary and the built
`dist/`. The container runs as an unprivileged user and writes nothing to disk.

---

## Testing

```sh
cd relay
cargo test
cargo build --release
```

The end-to-end tests drive the real listener on an ephemeral loopback port, so
the router, both WebSocket upgrades, the two session state machines and the
routing table are exercised together over real sockets.

---

## Deployment status

**Nothing has been deployed by this milestone.** There is no hosting provider,
no domain, no certificate and no credential in this repository or in the
documentation. Every example above is a placeholder an operator replaces. This
directory contains the artefacts needed to deploy the relay; running them is the
operator's decision.
