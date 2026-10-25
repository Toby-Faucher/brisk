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
- `stopped()` returning `Some(code)` means the browser sent STOP_SENDING; that is not an ack.
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
  127.0.0.1, and a WebTransport session needs the random 128-bit token from the page URL
  (compared in constant time). A connection that sends a wrong token or a malformed CONNECT, or no
  session within 10 s, is closed (code 0x101). At most 8 handshakes run at once and at most 10
  connections are open; more are refused. Only an authenticated connection gets the tasks that
  read its uni streams (`tests/wt.rs`).

## Wire format (bridge to page)

One unidirectional stream per frame: 21-byte little-endian header
`{frame_idx u32, flags u8 (b0 IDR), capture_ns u64, bridge_ns u64}`, Annex B H.264, FIN. Stream send
priority = frame index (newer first). `capture_ns` and `bridge_ns` are both CLOCK_MONOTONIC on bawn,
so the page can show `bridge_write - capture` without any clock sync. The page sends one-byte uni
streams back: `0x01` = "my decoder needs a key frame" (gap, decoder error, decode queue > 2). While
it waits, every dropped frame asks again (throttled to one ask per 100 ms): an ask can be lost, or
the IDR it brought can be dropped for a backed-up queue, and Sunshine sends no periodic IDR.

Drop policy (`session.rs::Pacer`, tests in `tests/backpressure.rs`): more than 3 unacknowledged frame
streams = behind: reset them all, drop until the next IDR, request an IDR (at most every 100 ms,
backing off to 2 s while nothing is acked). The bridge holds at most 4 frames per viewer. A new
viewer replaces the old one (reload) and starts by waiting for an IDR.

## Audio (Task 5)

Audio keeps Sunshine's normal Moonlight path (RTP on UDP 48000, AES-CBC with `SS_ENC_AUDIO`,
RS 4+2 FEC, Opus 5 ms stereo CBR). The bridge runs M1's `AudioFec` + `AudioCipher`
(`session.rs::AudioRx`) and sends each Opus packet **unchanged** as one WebTransport datagram:
quarter stream id (1 byte, session 0), then `{seq u16, ts_48k u32}` little-endian, then the Opus
bytes. Sunshine's RTP audio timestamp counts ms (`timestamp += packetDuration`, stream.cpp:1901),
so `ts_48k = rtp_ts * 48`. Sunshine's stereo packets are about 320 bytes (512 kbit/s); with the
header that is far below the 1200-byte QUIC minimum MTU (`tests/audio_fwd.rs::datagram_fits_mtu`).
A packet over the connection's limit (Sunshine allows up to 1400) is counted `too_large` and skipped.

Back-pressure: datagrams are never retransmitted or acked. quinn's datagram send buffer is capped
at `wt::AUDIO_SEND_BUFFER` = 4 KiB (about 11 packets, 55 ms); a send into a full buffer evicts the
oldest queued datagram, counted as `evicted` (exactly one per send for CBR packets). Between the
RTP socket and the forward loop a 64-packet channel drops new packets when full (`queue_full`).
Audio never requests an IDR. The bridge report prints one `audio` line.

Page (`web/src/lib/audio.ts`, `audio-worklet.ts`): WebCodecs `AudioDecoder` `{codec:"opus",
sampleRate:48000, numberOfChannels:2}` (no description), planar PCM posted to an AudioWorklet
ring (`Playout`). The AudioContext is created inside the **Connect** button's click (Chrome's
autoplay policy); the page now waits for that click before it connects.
- Ring: starts (and restarts) at 20 ms buffered, never holds more than 40 ms (a burst drops the
  oldest audio: `overflows`).
- A seq gap counts `lost` and is filled with the last packet repeated once, fading to zero, then
  silence (at most 40 ms), and the next packet fades in over 2 ms. Opus PLC is not exposed by
  WebCodecs.
- A packet older than expected counts `late` and is dropped.
- Starvation (ring runs dry): classified when audio resumes. Ended by a seq gap, or shorter than
  200 ms: `underruns` (a glitch). At least 200 ms with no gap: `pauses` (the host went quiet;
  Sunshine may stop sending while silent): not a glitch.
- Drift: when the ring's average fill (about 1.4 s time constant) is more than 5 ms off the
  20 ms target, one frame per 128-frame quantum is dropped or repeated (`drift -dropped/+repeated`).
  Limit: no interpolation, at most 0.8 %; fine for crystal drift (tens of ppm, measured about
  30 ppm below), audible only if clocks were far apart.
- AudioData timestamps are rewritten by Chrome's decoder (contiguous), so seqs go through a FIFO
  next to the decoder (Opus gives one output per chunk, in order).
- `non-silent` counts decoded packets with any sample above -60 dBFS: real audio, not zeros.

## Running it

- Up: `scripts/brisk-up.sh [--secs N]`. Builds (capped, `nice`), makes or renews the certificate,
  switches Sunshine to the patched build if it is FREE (`sunshine-brisk.sh install` + `enable`,
  restarts kept 100 s apart), serves the page on 127.0.0.1:8443, and runs the bridge in the
  foreground. It prints the `ssh -L` line and the page URL.
- Down: Ctrl-C (the bridge cancels its own Sunshine session), then `scripts/brisk-down.sh`: stops the
  bridge and page server if still running, then `sunshine-brisk.sh rollback` (stock Sunshine,
  refuses while busy).
- Headless check on bawn: `cd web && node scripts/headless.mjs '<page URL>' --secs 20 [--stall 3000]`.
  It clicks Connect itself (a trusted click, so the AudioContext runs).

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

Histogram percentiles are 0.1 ms bucket upper edges.

About 14 % of frames carry `capture_ns = 0` (31456 of 36556 tap frames had a capture time). The
patch writes 0 and sets tap flag b2 (`no_capture_ts`) under the same condition, an empty
`packet.frame_timestamp` (`patches/sunshine/0001-brisk-tap.patch`, header offsets 5 and 24).
Sunshine leaves it empty when it encodes without a new captured image: its encode loop waits
`max_frametime` for an image and, on a static screen, re-encodes the previous one to hold a
minimum frame rate ("Encode at a minimum FPS to avoid image quality issues with static content",
`src/video.cpp` around line 2472), or encodes a requested IDR from the last image without
popping a new one. These frames have no capture time to measure from, so both latency histograms
and the gate use only the frames with `capture_ns != 0`; `bridge_write - tap` covers all frames.
(The 60 recorded fixtures in `fixtures/tap/` happen to contain none: flags 0 or 1 only.) Static screen only; a moving scene is not
measured. Not measured: laptop Chrome (decode, present, network), see below.

### Audio, measured on bawn (2026-10-24, loopback, headless Chromium, 10 min)

`BRISK_LISTEN=127.0.0.1:4433 scripts/brisk-up.sh --secs 700`, then `headless.mjs --secs 600`; a
10 s 440 Hz tone was played into `sink-sunshine-stereo` at about 300 s (`pw-play --target`).

| quantity | value |
|---|---|
| page: datagrams / decoded / decode errors | 120045 / 120045 / 0 (200 per second) |
| page: non-silent packets | 2001 (= the 10 s tone; the desktop is silent otherwise) |
| page: lost / late / pauses / overflows | 0 / 0 / 0 / 2 (the 2 overflows: start-up burst) |
| page: underruns | 7 in 10 min (headless main thread also software-decodes 1080p60 video) |
| page: bufferMs (end) / AudioContext output latency | 18.9 ms / 42.7 ms (null sink's reported latency) |
| page: drift | 908 frames dropped, 0 repeated (start-up included; about 30 ppm) |
| bridge: packets / FEC recovered / lost / errors | 139969 / 1 / 0 / 0 |
| bridge: sent / evicted / too_large / queue_full | 127066 / 3862 / 0 / 0 |

Sunshine sent audio continuously (200 packets/s) while the host was silent. The evictions all fall
in the two 10 s windows after a headless browser was closed (dead tab until the idle timeout):
sent minus what the two pages received is 4004.

Proves: datagram framing end to end, Opus decode of Sunshine's real stream in WebCodecs, the
worklet ring running on a running AudioContext started by a click, the counters, real (non-zero)
audio decoded. Cannot prove: that it is audible (headless Chromium has no real output device, its
null sink still clocks the worklet), laptop Chrome's audio device clock and latency, the Tailscale
path. That is Toby's listening check below.

## Toby to run (laptop Chrome)

1. On bawn (needs Sunshine idle): `cd ~/Projects/brisk && scripts/brisk-up.sh --secs 660`
2. On the laptop: `ssh -N -L 8443:127.0.0.1:8443 bawn` (leave it open).
3. In Chrome on the laptop, open the `page` URL that brisk-up printed
   (`http://localhost:8443/?wt=https://100.80.84.102:4433/<token>&hash=<sha256>`).
   Click **Connect** (it starts audio; Chrome only allows sound after a click).
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

Audio, in the same run (play something on bawn: music in a browser on bawn's desktop, or
`pw-play --target sink-sunshine-stereo <file.wav>` over ssh):
- The `audio` lines should show `running`, `packets` and `decoded` climbing about 200 per second,
  `decode errors 0`, `buffer` between about 15 and 30 ms, `non-silent` climbing while sound plays.
- Listen for 10 minutes. Pass: no audible clicks or dropouts, and `lost` and `underruns` stay at
  or near 0 (note their final values and `drift -x/+y`).
- Glitch or silence? A glitch you hear should line up with `lost` or `underruns` going up. A quiet
  host shows as `non-silent` stopping, or `pauses` going up if Sunshine stops sending; neither is a
  glitch. If you hear a glitch and no counter moved, note the time: it is the laptop's audio device
  or Chrome, not the stream.
- Note `output` (ms the laptop's audio device adds after the ring) for the A/V budget.
- At the end, copy the bridge's `audio` line (`lost`, `evicted`, `queue_full` should be 0 or small).
