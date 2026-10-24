# M2 notes

## WebTransport stack (Task 4 spike, 2026-10-24)

Decision: **raw quinn 0.11 + `web-transport-proto` 0.6 framing + our own pure-Rust QUIC crypto**
(`crates/brisk-bridge/src/quic_crypto.rs`). Neither `wtransport` nor `web-transport-quinn` is used.

Why not the two candidates: both call quinn's rustls glue (`quinn::crypto::rustls`:
`wtransport` downcasts to `quinn::crypto::rustls::HandshakeData` in `connection.rs:402`;
`web-transport-quinn` builds `QuicClientConfig` in `client.rs`). quinn-proto compiles that glue only
with its `rustls-ring` or `rustls-aws-lc-rs` features, i.e. ring or aws-lc (C/asm), which
`scripts/check-pure-rust.sh` forbids. And no pure-Rust rustls provider implements `rustls::quic`:
rustls-rustcrypto 0.0.2-alpha has `quic: None` on every suite and `todo!()` in `quic.rs`;
rustls-pure 0.15.3 has `quic: None`.

What `quic_crypto.rs` adds (about 400 lines with tests):
- `rustls::quic::Algorithm` for AES-128-GCM: packet protection (`aes-gcm`) and AES-128 header
  protection (`aes`), RFC 9001 sections 5.3-5.4.
- A provider: rustls-rustcrypto (key exchange, ECDSA, randomness) with only
  `TLS13_AES_128_GCM_SHA256`, which carries that algorithm. TLS 1.3 only, no 0-RTT, no client auth.
- quinn's `crypto::ServerConfig` and `crypto::Session` over `rustls::quic::ServerConnection`: the
  server half of quinn-proto 0.11.19 `src/crypto/rustls.rs` (MIT/Apache-2.0), with the Retry tag
  computed with `aes-gcm` instead of ring.
- HMAC-SHA256 stateless-reset key and HKDF-SHA256 / AES-256-GCM token key (quinn-proto's
  `ring_like.rs` equivalents).
- Tests: the QUIC v2 server Initial vector (draft-ietf-quic-v2-10 A.3, the one rustls checks its
  ring backend with) end to end and back, the RFC 9001 A.4 Retry tag, 4- vs 5-bit header masking
  and pn-length handling, token and reset key round trips and tampering. All 7 mutants tried on
  this file are caught (2 only after the masking test was added; see the Task 4 report).

`wt.rs` (about 200 lines) does the HTTP/3 + WebTransport subset Chrome needs: our SETTINGS
(extended CONNECT, datagrams, WebTransport), the browser's CONNECT on its first bidi stream (200
only for `/<session token>`), server-initiated uni streams (`0x54, session id`), and the browser's
own uni streams (messages to the bridge; its HTTP/3 control/QPACK streams are drained).

Purity: `scripts/check-pure-rust.sh` passes. It now lists the native targets instead of
`--target all`, because quinn-proto 0.11 depends on ring **unconditionally for
`wasm32-unknown-unknown` only** (`[target.'cfg(all(target_family = "wasm", target_os = "unknown"))'.dependencies.ring]`).
That edge is never built for bawn or any native target (`cargo tree --target x86_64-unknown-linux-gnu -i ring`
prints nothing). **Decision for Toby**: accept this scoping of the guard, or ask for a different
fix (for example a `[patch]` of quinn-proto).

Spike time: about 30 min to the first "hello" uni stream in headless Chromium, over loopback and
over bawn's Tailscale IP.

### Pitfalls found

- Chrome (Playwright build 1248, `chromium_headless_shell`) speaks the draft-02 style: its SETTINGS
  carry `0x2b603742` (WEBTRANSPORT_ENABLE_DEPRECATED) = 1 and no `WEBTRANSPORT_MAX_SESSIONS`.
  `Settings::enable_webtransport` sends both styles, so this works.
- Chrome opens QPACK uni streams that can stay silent. Reading each browser uni stream's type
  inline in the accept loop blocked every later stream (our "need a key frame" messages arrived
  only when the connection closed). Each uni stream now gets its own task.
- `SendStream::stopped()` resolves once the peer *acknowledged* all stream data, which QUIC does
  when the bytes reach Chrome's buffers, not when the page reads them. So a page that stops reading
  only back-pressures the bridge once Chrome's flow-control windows or stream credits run out
  (seconds). The page-side `decodeQueueSize > 2` rule and its key-frame request cover a slow
  decoder; the bridge's resets cover a slow network.
- A tab that dies without closing keeps the QUIC connection until the 10 s idle timeout. The
  bridge then resets streams nobody acks; IDR requests back off from 100 ms to at most every 2 s
  while nothing is acknowledged, so Sunshine is not asked for 10 IDRs a second for nobody.

## Certificates and the secure context

Chosen: **a self-signed ECDSA P-256 certificate, valid 13 days, pinned by the page with
`serverCertificateHashes`**, and the page served on `http://localhost` through an SSH port forward.

- `tailscale cert` needs root or operator rights on bawn (`Access denied: cert access denied ...
  Use 'sudo tailscale set --operator=$USER'`), i.e. a tailscaled config change, which is out of
  bounds for agents. Not used. If Toby wants a real certificate later: `sudo tailscale set
  --operator=toby` once, then `tailscale cert bawn.tail674625.ts.net`, and the page could be served
  over https from bawn directly.
- `serverCertificateHashes` needs no CA, but Chrome accepts only ECDSA P-256 certificates valid
  for at most 14 days. `scripts/brisk-up.sh` makes one with `openssl req -x509 -newkey ec
  -pkeyopt ec_paramgen_curve:prime256v1 -days 13` into `~/.config/brisk/tls/` (0700, outside the
  repo; `*.pem` is gitignored anyway) and renews it after 10 days. The bridge prints the SHA-256 of
  the certificate in the page URL (`&hash=`), so a renewed certificate needs no other change.
- WebTransport and WebCodecs need a secure context. `http://localhost` is one; `http://<tailscale
  ip>` is not. So the page (static `web/dist`, served by `python3 -m http.server --bind 127.0.0.1
  8443`) is reached from the laptop with `ssh -N -L 8443:127.0.0.1:8443 bawn`, while WebTransport
  itself goes straight to `https://100.80.84.102:4433/<token>` over Tailscale (UDP; an SSH forward
  cannot carry it). The certificate hash replaces hostname checks, so the IP works.
- Exposure: the bridge binds bawn's Tailscale IP only (it refuses `0.0.0.0`), the page server binds
  127.0.0.1, and a WebTransport session needs the random 128-bit token from the page URL.

## Wire format (bridge to page)

One unidirectional stream per frame: 21-byte little-endian header
`{frame_idx u32, flags u8 (b0 IDR), capture_ns u64, bridge_ns u64}`, Annex B H.264, FIN. Stream send
priority = frame index (newer first). `capture_ns` and `bridge_ns` are both CLOCK_MONOTONIC on bawn,
so the page can show `bridge_write - capture` without any clock sync. The page sends one-byte uni
streams back: `0x01` = "my decoder needs a key frame" (gap, decoder error, decode queue > 2).

Drop policy (`session.rs::Pacer`, tests in `tests/backpressure.rs`): more than 3 unacknowledged frame
streams = behind: reset them all, drop until the next IDR, request an IDR (at most every 100 ms,
backing off to 2 s while nothing is acked). The bridge holds at most 4 frames per viewer. A new
viewer replaces the old one (reload) and starts by waiting for an IDR.

## Running it

- Up: `scripts/brisk-up.sh [--secs N]`. Builds (capped, `nice`), makes or renews the certificate,
  switches Sunshine to the patched build if it is FREE (`sunshine-brisk.sh install` + `enable`,
  restarts kept 100 s apart), serves the page on 127.0.0.1:8443, and runs the bridge in the
  foreground. It prints the `ssh -L` line and the page URL.
- Down: Ctrl-C (the bridge cancels its own Sunshine session), then `scripts/brisk-down.sh`: stops the
  bridge and page server if still running, then `sunshine-brisk.sh rollback` (stock Sunshine,
  refuses while busy).
- Headless check on bawn: `cd web && node scripts/headless.mjs '<page URL>' --secs 20 [--stall 3000]`.

## What the headless Chromium run proves, and what it cannot

Playwright's `chromium_headless_shell-1248` on bawn (software, no GPU):
- proves: WebTransport connect with a pinned self-signed certificate over the Tailscale IP, the
  token check, per-frame uni streams and the header, the SPS to `avc1.640c2a` codec string,
  `isConfigSupported` (prefer-hardware: false, no-preference: true, prefer-software: true), real H.264
  decode of Sunshine's stream at 1920x1080 in its software decoder with 0 decode errors, the
  present loop, the page-to-bridge key-frame message, the bridge's resets and IDR back-off (a killed
  tab, until the idle timeout), that a 3 s reader stall causes no decode errors (and no bridge
  resets: Chrome buffers), and the bridge-side latency gate (both stamps on bawn).
- cannot prove: Chrome on Toby's laptop (its hardware decoder, its present timing, the real
  Tailscale network path, GPU compositing), visible video on a real display, input-to-photon.
  Decode/present numbers from headless are bawn software-decode numbers, not the laptop's.

## Measured on bawn (2026-10-24, patched Sunshine, Desktop app, 1080p60 H.264, static screen)

10 min, bridge on bawn's Tailscale IP, headless Chromium on bawn (`headless.mjs --secs 600
--stall 3000`). Both latency stamps are CLOCK_MONOTONIC on bawn.

| quantity | value |
|---|---|
| tap frames / sent to the page / decoded by the page | 36556 / 34837 / 34619 (the page was open 600 of 630 s) |
| decode errors | 0 |
| `bridge_write - capture` (bridge report, n 29986) | p50 6.0, p99 8.0 ms |
| `frame_processing_latency` = `tap - capture` (n 31456) | p50 5.9, p99 7.9 ms |
| `bridge_write - tap` (tap socket + bridge, n 34837) | p50 0.1, p99 0.2 ms |
| M2 latency gate (p50 6.0 <= 5.9 + 2) | PASS |
| page `bridge_write - capture` (last 600 frames) | p50 6.6, p99 7.8 ms |
| headless software decode (output - decode call) | p50 4.1, p99 11.4 ms |
| headless present (output - rAF paint) | p50 9.9, p99 19.4 ms |
| page drops: 177 (start-up wait for an IDR, and the IDR wait after the 3 s stall) | 2 key requests |
| bridge resets / drops / IDR requests | 40 / 569 / 12, all in the 10 s after the headless browser was killed (dead tab until the idle timeout) |
| bridge CPU / RSS | about 2 % / 6.5 MB; headless Chromium about 70 % of a core |

Histogram percentiles are 0.1 ms bucket upper edges. Static screen only; a moving scene is not
measured. Not measured: laptop Chrome (decode, present, network), see below.

## Toby to run (laptop Chrome)

1. On bawn (needs Sunshine idle): `cd ~/Projects/brisk && scripts/brisk-up.sh --secs 660`
2. On the laptop: `ssh -N -L 8443:127.0.0.1:8443 bawn` (leave it open).
3. In Chrome on the laptop, open the `page` URL that brisk-up printed
   (`http://localhost:8443/?wt=https://100.80.84.102:4433/<token>&hash=<sha256>`).
4. The stats box should show within a few seconds: `codec avc1.640c2a supported true hw
   prefer-hardware` (or `no-preference` if Chrome has no hardware H.264 decode on the laptop),
   `1920x1080`, `received` and `decoded` climbing about 60 per second, `decode errors 0`,
   `decodeQueueSize` 0-2, `bridge_write-capture p50` about 5-7 ms, and the bawn desktop visible.
5. Leave the tab in front for 10 minutes. Pass: `decode errors 0` at the end and the desktop moving
   when something changes on bawn. Note `decode p50/p99` and `present p50/p99` (they go into
   docs/m0-baseline.md "client decode" and "client present queue"), and `dropped`/`key requests`.
6. When `--secs` runs out (or Ctrl-C) brisk-up prints the bridge report: copy the
   `bridge_write-capture`, `frame_processing(tap-capture)` and `gate(...)` lines.
7. `scripts/brisk-down.sh` (back to stock Sunshine).
