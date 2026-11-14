# app.timer â€” M7 standalone Noise/WASM feasibility spike report

**Status: proof of concept complete, all assertions pass. Not Stage 2.**
This spike is a self-contained experiment created **outside the application source
tree**. It does not modify, build or link the application, its `Cargo.toml` /
`Cargo.lock`, the frontend, the relay or the deployment configuration.

Companion design document: `m7-e2e-security-design.md` (revision 4), Â§5.5 and Â§17.

---

## 1. Question the spike answers

The design's recommended browser Noise implementation is **`snow` compiled to
WebAssembly** (`m7-e2e-security-design.md` Â§5.5), so the browser and the host run
the *same* reviewed Rust implementation of
`Noise_KK_25519_ChaChaPoly_SHA256`. Before that recommendation can be relied upon,
four things had to be shown to be true and reproducible:

1. `snow` compiles to the intended WASM target (`wasm32-unknown-unknown`).
2. Secure browser randomness is available and usable.
3. A browser-side (WASM) endpoint and a native Rust endpoint complete the **exact**
   `Noise_KK_25519_ChaChaPoly_SHA256` handshake and derive compatible transport
   states.
4. The wrapper fails closed (wrong pins, tampered messages) and does **not**
   expose private key material.

**The spike changed no Noise pattern and no cipher suite.** It uses
`Noise_KK_25519_ChaChaPoly_SHA256` verbatim, including the prologue
`ASCII("app.timer/e2e/v1") â€– hostId â€– 0x00 â€– deviceId`.

---

## 2. What was proven (summary)

| Claim | Result |
| --- | --- |
| `snow` 0.10.0 compiles to `wasm32-unknown-unknown` | **Yes** â€” artefact built (Â§5) |
| Secure browser randomness available | **Yes** â€” `globalThis.crypto.getRandomValues` |
| Browser â‡„ Rust complete the exact `KK` handshake | **Yes** â€” byte-exact, 48-byte messages |
| Both sides derive compatible transport states | **Yes** â€” identical handshake hash |
| Bidirectional encrypted messages work | **Yes** â€” 3 frames each direction |
| Wrong static keys fail closed | **Yes** â€” rejected on both sides |
| Modified handshake messages fail closed | **Yes** â€” every single-bit tamper rejected |
| Wrapper does not expose private key material | **Yes** â€” no enumerable state, no secret in the export surface |

**Result: 50 assertions passed, 0 failed (exit code 0).**

---

## 3. Layout

All paths relative to the workspace root
`the spike workspace root`:

```text
spike/noise-kk-wasm/
  Cargo.toml                # spike-only dependency shaping (Â§4)
  Cargo.lock                # getrandom is absent (committed lock)
  src/
    lib.rs                  # shared Noise KK core (Endpoint wrapping snow)
    wasm.rs                 # extern "C" ABI over WASM linear memory
    bin/interop_peer.rs     # native Rust peer; line-oriented stdio RPC
  js/
    noise_kk.js             # JS wrapper (browser + Node); no build step
    run_spike.mjs           # cross-implementation + negative test driver
  results.txt               # captured output of a green run
```

Nothing under this directory is referenced by the application.

---

## 4. Versions and the one real build obstacle

**Toolchain used for the recorded run:**

```text
rustc  1.100.0 (0ef1bc386 2026-11-09)
cargo  1.100.0 (3acc79c0c 2026-11-05)
node   v24.19.0
targets installed: wasm32-unknown-unknown, x86_64-pc-windows-msvc
```

**Key dependencies (from `Cargo.lock`):** `snow 0.10.0`, `chacha20poly1305 0.10.1`,
`chacha20 0.9.1`, `curve25519-dalek 4.1.3`, `sha2 0.10.9`, `rand_chacha 0.3.1`,
`rand_core 0.6.4`, `subtle 2.6.1`, `zeroize 1.9.1`, `poly1305 0.8.0`.

**Obstacle: `getrandom` on `wasm32-unknown-unknown`.** `snow`'s default features
enable `use-getrandom`, and `getrandom` 0.3 does not build for
`wasm32-unknown-unknown` unless the caller opts into a wasm backend (`wasm_js`,
which needs `wasm-bindgen` glue). The spike therefore takes `snow` **without**
default features and enables only the primitives this protocol needs, supplying
randomness from a caller-seeded resolver:

```toml
# spike/noise-kk-wasm/Cargo.toml
[dependencies]
snow = { version = "0.10.0", default-features = false, features = [
    "default-resolver",
    "use-curve25519",
    "use-chacha20poly1305",
    "use-sha2",
] }
rand_core = "0.6"
rand_chacha = "0.3"

[profile.release]
opt-level = "s"
lto = true
panic = "abort"
```

A custom `CryptoResolver` returns a `ChaCha20Rng` seeded by the caller for the
`Random` slot and delegates every other primitive to snow's `DefaultResolver`.
**`getrandom` is absent from the committed `Cargo.lock`.** The Noise suite is
unaffected: `curve25519` + `chacha20poly1305` + `sha2` are exactly the primitives
`Noise_KK_25519_ChaChaPoly_SHA256` names.

**No `wasm-bindgen` and no `wasm-pack` were used,** by design: the ABI is a raw
`extern "C"` surface over WASM linear memory, so the spike also demonstrates that
the production wrapper need not depend on a binding generator.

---

## 5. Reproducible commands

From `spike/noise-kk-wasm` (PowerShell):

```powershell
# 1. Build the WASM library for the intended browser target.
cargo build --release --target wasm32-unknown-unknown

# 2. Build the native Rust interop peer (host/responder side of the test).
cargo build --release --bin interop-peer

# 3. Run the driver: WASM device (initiator) <-> native host (responder).
node js/run_spike.mjs
```

`run_spike.mjs` exits `0` on success, `1` on a test failure, `2` on an abort, and
writes `results.txt`.

**Artefacts produced (this run):**

| Artefact | Size (bytes) | SHA-256 |
| --- | --- | --- |
| `target/wasm32-unknown-unknown/release/noise_kk_spike.wasm` | 152,988 | `606C53B416437829ECC3418C06C80753BD2B35FCBAEC2194E5CA0FB1582D5013` |
| `target/release/interop-peer.exe` | 235,008 | `D205F58E2051C0F37973DD5F425A61CE3B5931CE89F66960EDF5FF551A024193` |

---

## 6. How the cross-implementation test works

- The **WASM build is the device / initiator**; the **native build is the host /
  responder**. The JS wrapper loads the WASM bytes, calls `spike_new` with the
  role, static private key, the **pre-known expected remote static public key**,
  the prologue, an RNG seed and (optionally) a fixed test ephemeral, then drives
  `spike_write` / `spike_read` / `spike_into_transport`.
- The native peer is driven over a line protocol on stdin/stdout
  (`HELLO`, `NEW`, `WRITE`, `READ`, `TRANSPORT`, `STATE`, `PIN`, `REMOTE_STATIC`,
  `HS_HASH`, `X25519_PUB`, `QUIT`); hex encodes bytes and `-` denotes empty.
- The driver compares the two sides **byte-for-byte** and asserts fail-closed
  behaviour for the negative cases.

The prologue is built as `ASCII("app.timer/e2e/v1") â€– hostId â€– 0x00 â€– deviceId`,
with `hostId = "h-6f1c2a4e"` and `deviceId = "d-90ab34cd"`.

**Test section map** (as printed by the driver):

| Section | Covers |
| --- | --- |
| 0 | module identity, ABI version, exact export whitelist |
| 1 | deterministic `KK` handshake, byte-exact messages, equal handshake hash |
| 2 | KK identity is **pre-known, not discovered** (pin supplied before the handshake) |
| 3 | transport mode, bidirectional encrypted messages, handle invalidation |
| 4 | wrong static keys fail closed |
| 5 | tampered handshake messages fail closed |
| 6 | secure browser randomness is available and usable |
| 7 | wrapper exposes no private key material |

---

## 7. Test vectors (deterministic run)

Fixed inputs (secret test seeds are RFC 8032-style test vectors, not production
keys):

```text
HOST_STATIC_PRIV    = 9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60
DEV_STATIC_PRIV     = 4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb
ROGUE_STATIC_PRIV   = 833fe62409237b9d62ec77587520911e9a759cec1d19755b7da901b96dca3d42
HOST_FIXED_EPHEMERAL= c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7
DEV_FIXED_EPHEMERAL = f5e5767cf153319517630f226876b86c8160cc583bc013744c6bf255f5cc0ee5
RNG_SEED            = 000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f
```

Derived and transcript bytes:

```text
prologueHex          = 6170702e74696d65722f6532652f7631682d366631633261346500642d3930616233346364
hostStaticPublic     = b755ced64d4a27ce32afcf199f18a3ed1f31897028b0ff6e55191ea449db2644
deviceStaticPublic   = b6c6192e66300f4bbb4e3d870bfd02e416154ebb06661a70a84ea376244b3c20

# Noise_KK_25519_ChaChaPoly_SHA256, prologue as above
handshakeMessage1    = a28d6dd77eb4e12f9347071f7f369fa14a91ebf01368e59364983492e10a6729855000986723201e9b2af85767fd92b0   # 48 bytes (e â€– tag)
handshakeMessage2    = 3ec09a863f81f9adc6ef1c865295fada64550430b4b31aa40ef6c9b593290b5facd7757a8c60472ebcc73691e12d8a0d   # 48 bytes
handshakeHash        = a9f00d3c5e734a4e0aff04ca9bd7f4d452ba82f85a9f429464d3d070a3bc3581

transportDeviceToHost = [
  "71e753065742243acdc8a25b83e45f9f41373d159206a86b84a45360520605",
  "08492b8f27c489f13812c27e8284612c209cd5d996a5f984173178d9be90ab903e19077b41790b61cd64cfad323b18746670ad5bfd14",
  "e9ebff1ed125f6420796a4ec70b9983645f1828ccc22cea11379b7467d0f29"
]
transportHostToDevice = [
  "a2ad66caebf1faab4708a9f655a9742329e99703da0a156a2dfea53066dd80",
  "e94347ca43ff10e585df79f6501f323574bfe64018aadb8da65266fe14f76e",
  "fc47d08fcada18387dc97d3f28c97697eac5414fbfe6b8bc5c75f72316df6b6ac9b95fdc77705fdda4b2"
]
```

Each transport entry is a chunk of a message encrypted by one side and decrypted
by the other; the driver asserts that every chunk is `plaintext + 16-byte AEAD
tag` and that no ciphertext equals its plaintext.

**Independent anchor.** These vectors pin the exact transcript for *this* suite so
a future change to the wrapper, the build flags or the dependency versions will be
caught by a byte mismatch. They complement â€” they do not replace â€” the RFC 9180
official vectors and the upstream Noise vectors required by
`m7-e2e-security-design.md` Â§5.7.

---

## 8. Negative results (fail-closed)

| Case | Observed |
| --- | --- |
| Device built with the **wrong pinned host key** | The responder rejects the connection; the device itself never reports handshake success. |
| Device static key **not in the responder's registry** | Rejected (unknown device). |
| Single flipped **payload-tag** bit in message 1 | Rejected. |
| Single flipped **ephemeral** bit in message 1 | Rejected. |
| Single flipped bit in message 2 | Rejected by the WASM side. |
| Any call after a fatal read | Returns `ERR_MODE`; the endpoint is permanently unusable (fail-closed). |

A successful exchange was re-run between the failing cases to prove the rejections
were caused by the tampering, not by a stuck endpoint.

---

## 9. Private-key exposure

- The JS `Endpoint` carries **no enumerable state at all**:
  `Object.getOwnPropertyNames(endpoint).length === 0` and
  `JSON.stringify(endpoint) === '{}'`. Module state is held in a private field.
- The WASM export surface is a fixed whitelist and contains **no** symbol whose
  name mentions `priv`, `secret`, `ephemeral`, `seed` or `scalar`. The only key
  material that crosses into WASM does so as **arguments to `spike_new`**; there is
  no export that *returns* a private key.
- `remote_static()` and `handshake_hash()` expose public values only.

---

## 10. Explicit non-goals / not covered

This is a feasibility proof, **not** Stage 2. It does not cover, and does not
claim to cover:

- the pair **HPKE** flow or the SAS;
- the **relay** envelope or `wss://` / rustls transport;
- the **application frames** (Â§9 of the design), rate limiting or the two pairing
  registries;
- the browser **CSP**, IndexedDB key storage, or any production key handling;
- **integration** with the Tauri app.

Those remain Stage 2 and stay gated by the design's Â§5.7 interop test. The spike
proves only that the WASM `snow` path is real, interoperable and fail-closed on
the exact suite.

---

## 11. Blocker report

**No blocker.** Every claim in Â§1 was demonstrated. `snow` 0.10.0 builds for
`wasm32-unknown-unknown` with caller-supplied randomness, and its transcripts
interoperate byte-for-byte with a native Rust peer. Because the spike succeeded,
**no alternative was substituted** and **no Noise pattern or cipher suite was
changed.**
