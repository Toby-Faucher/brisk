# M2 notes

> Superseded for running: `scripts/brisk-up.sh` now runs `brisk-hostd`, which serves the page and
> starts/stops the bridge (no `--secs`, no python page server). See docs/ui-notes.md. The run
> recipes below are the historical M2 ones (`brisk-up.sh --secs N`); measurements are unchanged.

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
  --operator=toby` once, then `tailscale cert bawn.tail0.ts.net`, and the page could be served
  over https from bawn directly.
- `serverCertificateHashes` needs no CA, but Chrome accepts only ECDSA P-256 certificates valid
  for at most 14 days. `scripts/brisk-up.sh` makes one with `openssl req -x509 -newkey ec
  -pkeyopt ec_paramgen_curve:prime256v1 -days 13` into `~/.config/brisk/tls/` (0700, outside the
  repo; `*.pem` is gitignored anyway) and renews it after 10 days. The bridge prints the SHA-256 of
  the certificate in the page URL (`&hash=`), so a renewed certificate needs no other change.
- WebTransport and WebCodecs need a secure context. `http://localhost` is one; `http://<tailscale
  ip>` is not. So the page (static `web/dist`, served by `python3 -m http.server --bind 127.0.0.1
  8443`) is reached from the laptop with `ssh -N -L 8443:127.0.0.1:8443 bawn`, while WebTransport
  itself goes straight to `https://100.64.0.1:4433/<token>` over Tailscale (UDP; an SSH forward
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
`0x02` = "send a key frame when you can" (the page's replay buffer, every 10 s while it is on): the bridge
requests an IDR at most once a second for it (`Pacer::refresh_key`, apart from the want-key limiter) and
drops nothing: the decoder is fine, unlike `0x01`, which drops every delta until the IDR arrives.

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

Reorder (`AudioRx`): AudioFec passes data through as it arrives and rebuilds a lost packet only
when the block's parity comes, after the block's later data. So on a seq gap the bridge holds the
packets behind it (never otherwise: no added delay without loss), at most the rest of that 4-packet
block, until the rebuild fills the gap (they go out in order, `reordered`), the next block starts
(its parity was sent before), or 20 ms pass (`REORDER_HOLD_NS`); then the held packets go out in
order and the missing seqs count `skipped`. A rebuild that comes after that is dropped (`late`).

Counters at two layers: bridge `lost` is AudioFec giving up on a block (beyond FEC), `skipped` the
reorder giving up; page `lost` is every seq gap the page sees, so it also includes bridge
`skipped`/`evicted`/`queue_full` and anything lost between bridge and browser.

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
  Limit: no interpolation, at most 0.8 %; fine for crystal drift (tens of ppm), audible only if
  clocks were far apart. The real host/client drift is not measured yet (see below).
- AudioData timestamps are rewritten by Chrome's decoder (contiguous), so seqs go through a FIFO
  next to the decoder (Opus gives one output per chunk, in order).
- `non-silent` counts decoded packets with any sample above -60 dBFS: real audio, not zeros.
- `underruns [a b c ms]`: how long each of the last 20 underruns kept the ring empty (until the
  next packet; playback then waits for the 20 ms target on top).
- The decoder is re-created after an error at most 5 times in a row without a decoded packet;
  then audio stops and `last error` says so.

## Input (Task 6)

Wire format (page to bridge): the page opens **one WebTransport bidirectional stream** (one stream
keeps a key-down ahead of its key-up; separate uni streams would not be ordered) and writes
frames `len u8` + `len` bytes of body. The body is `kind u8` + fields, little-endian:

| kind | body | meaning |
|---|---|---|
| `0x10` key | `vk u8, down u8 (0/1), modifiers u8` | Windows virtual-key code; modifiers 1 shift, 2 ctrl, 4 alt, 8 meta |
| `0x11` move | `dx i16, dy i16` | relative motion |
| `0x12` button | `button u8 (1 L, 2 M, 3 R, 4 X1, 5 X2), down u8` | mouse button |
| `0x13` scroll | `amount i16` | vertical wheel, 120 = one notch, positive = up |
| `0x14` heartbeat | (none) | "something is still held": sent every 250 ms while any key or button is down; never reaches Sunshine |

The stream header is Chrome's (`0x41`, session id). Gamepads (`0x15`, `0x16`) and rumble from the
bridge (`0x20`, the only thing the bridge writes on this stream) came in M3: docs/controller-notes.md.
The one-byte "need a key frame" uni streams (above) are unchanged.

Page (`web/src/lib/input.ts`):
- A click on the video focuses the canvas and asks for Pointer Lock. The click itself is never sent
  (it happens before the lock). Keys are sent while the canvas has focus. Motion, buttons and
  wheel are sent only while locked.
- Keys go by `KeyboardEvent.code` (the physical key, so AZERTY's `q` key is still `KeyA` = 0x41 and
  the host's layout decides the character): letters, digits, F1-F24, arrows, editing keys, left
  and right modifiers as VK_LSHIFT 0xA0 / VK_RSHIFT 0xA1 / VK_LCONTROL 0xA2 / ... / VK_LWIN 0x5B
  (like moonlight-qt), numpad (NumpadEnter = VK_RETURN, as Moonlight sends it), punctuation by
  US position (VK_OEM_*). Mapped keys get `preventDefault` (Chrome ignores it for its reserved
  shortcuts such as Ctrl+W, which is fine). Repeats are not sent: Sunshine repeats held keys itself
  (`key_repeat_delay`, input.cpp).
- **Escape is never sent.** While locked, Chrome leaves Pointer Lock on Escape itself; the page
  then also drops keyboard focus and releases everything. Without the lock (keyboard-only focus),
  Escape drops focus. So Escape always gets you out. Limit: no Escape reaches the host in M2 (Big
  Picture "back" is Backspace or B on a controller). [CALL] for Toby: a different release combo
  if a game needs Escape.
- Motion is summed and sent at most every 8 ms (`FLUSH_MS`): a 1 kHz mouse is 125 messages a
  second. Motion is also flushed before any key or button so order holds. If the stream's
  writer is backed up (`desiredSize <= 0`), motion and wheel are dropped (`dropped`), keys and
  buttons never.
- The first non-zero motion after a lock request is dropped: it is the cursor jumping into the
  lock, not the user (headless Chromium: -640,-360 on its 1280x720 page, sent *before*
  `pointerlockchange`, which is why the flag is set on the click).
- While any key or button is held, a heartbeat frame goes out every 250 ms (`HEARTBEAT_MS`); the
  interval runs exactly while something is held and stops with the last release or a release-all.
- Everything held is released (key-ups and button-ups) on window `blur`, `visibilitychange` to
  hidden, `pagehide`, Pointer Lock loss, and transport close.

Bridge (`crates/brisk-bridge/src/input.rs`):
- Each viewer gets a task that accepts its bidi streams (`wt::Session::accept_bi`, header
  checked by `wt::is_webtransport_bi`); each stream gets its own reader task. Readers push into one
  bridge-wide `InputQueue`; one `send_loop` (beside `forward` in `main.rs`) hands events to
  `ControlHandle::send_input`. Video and audio never wait on input.
- Unknown kind, wrong length, vk 0 or 0xFF, unknown modifier bits, button outside 1..5, `down`
  not 0/1: dropped and counted `bad`; the stream and session go on. A frame cut off by the stream's
  end is ignored.
- The queue knows what is held. A press of a held key and a release of a key that is not held are
  dropped (repeats, or the release of a press that was dropped). Motion and scroll merge into a
  queued motion/scroll right before them (`coalesced`), never across a key or button.
- At most 32 droppable events (motion, scroll, presses) are queued; when full the oldest motion or
  scroll is evicted, or, with only presses queued, the new press or motion is dropped
  (`dropped`). Releases are never dropped and never count against the 32 (each needs an earlier
  press, so they stay bounded too).
- Held-key watchdog: while the queue holds any key or button down, a stream that sends no frame
  (heartbeat or other) for 1 s (`HOLD_TIMEOUT`) gets everything released (`held_timeouts`). A lost
  network, a closed lid or a frozen tab would otherwise leave the key down on bawn, auto-repeating,
  until QUIC's 10 s idle timeout. The timer is re-armed by every frame and is not armed at all while
  nothing is held. Heartbeats count as `heartbeats`, not `received` or `bad`.
- `send_input` errors: `Closed` = the control session is over, the bridge stops. `Timeout` (not
  drained for 1 s) is not fatal: that event is dropped (`timeouts`), except a release, which goes
  back to the front of the queue and is retried. Any other error drops the event, logged and
  counted `errors`, with no retry (so it cannot spin).
- A stream that ends (tab closed, reload, connection lost or idle-timed-out) releases every held
  key (in vk order) and button (`released`). When the bridge stops (Ctrl-C, `--secs`, host
  ended), it sends a release for **every key and button pressed during the session** straight on
  the control session before the ENet disconnect: a superset, because up to 64 encoded commands
  (`CMD_QUEUE`) can still sit in the `ControlHandle` channel when the control loop is cancelled, and
  their key-ups have already left the held set. Releasing a key the host does not hold does
  nothing. If the ENet send window (256 in flight) filled first, the rest are skipped. Sunshine also resets its input state when a session ends (`input::reset`,
  input.cpp), so a key could stay down only while a live session's control path is stuck.
- What can still be lost at stop: presses, motion and other non-release commands in that channel
  (harmless to lose); releases are covered by the superset above, and Sunshine's own session-end
  input reset covers anything else.
- The report prints `input received sent bad coalesced dropped timeouts errors released held_timeouts heartbeats`.

Headless (chromium_headless_shell-1248 on bawn): **Pointer Lock works** on a CDP click (a trusted
gesture); `document.pointerLockElement` is the canvas. But under the lock each CDP
`Input.dispatchMouseEvent` move becomes a jump from the parked cursor and back (`+640,+360` then
`-640,-360`), so CDP cannot drive `movementX/Y` there. `headless.mjs --input` therefore sends keys
with CDP key events (trusted, through `keydown`/`keyup` and the `code` table) and motion with page
`MouseEvent`s carrying `movementX/Y` (same handler, not trusted). Proven headless: lock request on
click, the click not forwarded, keys by code with modifiers, motion coalescing and framing, the
bridge mapping, Sunshine decoding them. Not proven headless: real mouse `movementX` values under
Pointer Lock on the laptop, mouse buttons and wheel live (not sent: the host shows Steam Big
Picture), and what the host does with the keys (Sunshine's log shows the decode, not the effect).

### Input, live on bawn (2026-10-24, patched Sunshine, Steam Big Picture on screen)

Safe input only: Shift, `a`, `b`, and +-5 px moves that end where they started; no clicks, no
other keys. Sunshine's `min_log_level = verbose` log was only grepped for its packet lines and
counted (it holds session keys and cookies); no raw lines are kept.

| run | page sent | bridge received / sent / bad | Sunshine decoded |
|---|---|---|---|
| 3 short runs (first harness, before the lock-jump fix) | 30 keys, 7 moves | 37 / 37 / 0 | 30 keyboard packets, 7 relative moves (3 x `-640,-360`, and one each of `+5,0`, `-5,0`, `0,+5`, `0,-5`), 0 buttons, 0 scroll |
| M2 gate run (10 min, 20 rounds every 30 s) | 120 keys, 80 moves | 200 / 200 / 0 | 120 keyboard, 80 relative moves (20 each of dx +5, -5, dy +5, -5), 0 buttons, 0 scroll |

Keyboard packets decode like Moonlight's (docs/moonlight-compare.md: keyAction 3, keyCode 8041 for
`a`): keyAction `00000003` / `00000004` for down / up, keyCode `8041` (`a`), `8042` (`b`), `80A0`
(left Shift), every down with its up (25 each over all runs). Relative moves decode with the
deltas sent.

Deviation: before the lock-jump fix, the first three runs each sent one `-640,-360` relative move
(headless Chromium's cursor jumping into the lock, see above), so bawn's pointer moved toward the
top-left corner three times. The other 4 of those 7 moves were the four +-5 px moves of the third
run's one completed round (deltas from the grep of Sunshine's `deltaX`/`deltaY` lines taken right
after those runs). No click or other key went with them. The gate run sent none (the 80
moves are all +-5).


## Running it

- Up: `scripts/brisk-up.sh` (now runs hostd; the old `--secs N` bridge argument is gone, see docs/ui-notes.md). Checks the listen address first (IPv4:PORT, not 0.0.0.0),
  builds inside the same memory-capped scope as the Sunshine build (`scripts/capped.sh`; it refuses
  to build if the cap is not enforced), makes or renews the certificate, switches Sunshine to the
  patched build only if it is FREE (checked before `sunshine-brisk.sh install` writes the drop-in,
  then `enable`; restarts kept 100 s apart), runs brisk-hostd in the foreground (it serves the page on
  127.0.0.1:8443 and runs the bridge when a stream starts). It prints the `ssh -L` line and the page URL.
- Down: Ctrl-C in brisk-up's terminal (brisk-up sends the bridge SIGTERM, which it handles like
  SIGINT, and cancels its own Sunshine session), then `scripts/brisk-down.sh`: SIGTERM to hostd
  (which stops its bridge) if still running, then `sunshine-brisk.sh rollback` (stock Sunshine, refuses
  while busy) whenever the brisk build runs *or* its drop-in exists. Stub tests:
  `scripts/test-brisk-up-down.sh`, `scripts/test-brisk-up-exit.sh`, `scripts/test-sunshine-brisk.sh`.
- Headless check on bawn: `cd web && node scripts/headless.mjs '<page URL>' --secs 20 [--stall 3000] [--input 30]`
  (`--input`: safe key and motion rounds, see Input).
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
| page: drift | 908 frames dropped, 0 repeated (see below: not a drift measurement) |
| bridge: packets / FEC recovered / lost / errors | 139969 / 1 / 0 / 0 |
| bridge: sent / evicted / too_large / queue_full | 127066 / 3862 / 0 / 0 |

The 908 dropped frames are not a drift figure: most are the start-up burst draining from the
40 ms cap down to the 25 ms deadband edge (about 720 frames), so real clock drift was not measured
(if the remaining ~190 frames were all drift that would be ~7 ppm, but the split is an estimate,
not a measurement). This run predates the reorder
buffer (it had no `reordered`/`skipped` counters; 1 FEC recovery in 10 min on loopback).

Sunshine sent audio continuously (200 packets/s) while the host was silent. The evictions all fall
in the two 10 s windows after a headless browser was closed (dead tab until the idle timeout):
sent minus what the two pages received is 4004.

Proves: datagram framing end to end, Opus decode of Sunshine's real stream in WebCodecs, the
worklet ring running on a running AudioContext started by a click, the counters, real (non-zero)
audio decoded. Cannot prove: that it is audible (headless Chromium has no real output device, its
null sink still clocks the worklet), laptop Chrome's audio device clock and latency, the Tailscale
path. That is Toby's listening check below.

### M2 gate run: video + audio + input (2026-10-24, Task 6)

`scripts/brisk-up.sh --secs 650` (bridge on bawn's Tailscale IP, patched Sunshine, app 870779643,
1080p60 H.264, static Big Picture screen), then `headless.mjs --secs 600 --stall 3000 --input 30`
(headless Chromium on bawn, software decode). Both latency stamps are CLOCK_MONOTONIC on bawn.

| quantity | value |
|---|---|
| tap frames / sent to the page / decoded by the page | 37666 / 34821 / 34610 |
| decode errors | 0 |
| `bridge_write - capture` (bridge report, n 30054) | p50 5.9, p99 8.1 ms |
| `frame_processing_latency` = `tap - capture` (n 32442) | p50 5.9, p99 8.0 ms |
| `bridge_write - tap` (n 34821) | p50 0.1, p99 0.2 ms |
| M2 latency gate (p50 5.9 <= 5.9 + 2) | PASS |
| page `bridge_write - capture` (last 600 frames) | p50 5.7, p99 6.1 ms |
| headless software decode / present (last 600 frames) | decode p50 1.1, p99 8.7 ms; present p50 13.0, p99 20.3 ms |
| page drops / key requests / streams reset | 168 (start-up IDR wait and after the 3 s stall) / 2 / 0 |
| bridge browser_leg reset / dropped / IDR requests | 40 / 549 / 12 (the 10 s after the headless browser closed: dead tab until the idle timeout, as in Task 4) |
| audio, bridge | packets 129991, recovered 0, lost 3, errors 0, reordered 1, skipped 3, late 0, sent 122048, evicted 1918 (dead-tab window), too_large 0, queue_full 0 |
| audio, page | datagrams 120044 / decoded 120044 / decode errors 0, lost 0, late 0, underruns 9 (1-7 ms each), pauses 0, overflows 1, buffer 22.9 ms, drift -669/+0, non-silent 0 (host silent) |
| input, page | 120 keys, 80 moves, 0 buttons, 0 scrolls, 0 dropped, Pointer Lock granted |
| input, bridge | received 200, sent 200, bad 0, coalesced 0, dropped 0, timeouts 0, released 0 |
| input, Sunshine log | 120 keyboard packets, 80 relative moves |
| bridge CPU / RSS | about 2.4 % / 6.6 MB |

The bridge's 3 audio `lost` (FEC could not rebuild them; this run went over the Tailscale IP, Task
5's over loopback) were not seen by the page (page `lost` 0 counts every seq gap it sees), so they
fell outside the page's 600 s: before it connected or after it closed.

## [U] items resolved during M2

From `research/m2-tap-design.md` (section numbers), `docs/design.md` and `docs/m0-baseline.md`.
Resolved means measured or exercised on bawn; "laptop" items stay open until Toby's run.

| [U] | status | evidence |
|---|---|---|
| §1 `steady_clock` is `CLOCK_MONOTONIC` | resolved | `bridge_write - capture` (bridge clock) p50 6.0 ms vs `frame_processing` p50 5.9 ms (Sunshine's stamp): the two clocks agree to 0.1 ms; another clock would give nonsense |
| §2 extra kernel copy costs tens of µs | resolved | Task 3: tap `recv` p50 13-41 µs, p99 159-204 µs; `bridge_write - tap` p50 0.1, p99 0.2 ms over 10 min |
| §2 a 1080p IDR is 100-500 KB | partly: static screen only | `fixtures/tap/`: IDR 24.3 KB, P-frames median 7.8 KB, max 31 KB (static screen; 20 Mbit/s asked, about 15 Mbit/s granted). A moving scene is not measured |
| §4.1 200 audio datagrams/s are fine for QUIC | resolved | Task 5: 120045 datagrams in 10 min, 0 lost on loopback; this gate run below over the Tailscale IP |
| §4.2 WebCodecs Opus stereo needs no description | resolved | Task 5: 120045 decoded, 0 errors with `{codec:"opus", sampleRate:48000, numberOfChannels:2}` |
| §4.3 Opus PLC is not exposed by WebCodecs | confirmed | no PLC option in `AudioDecoder`; the page conceals (Audio section) |
| §4.5 AudioContext needs a user gesture | resolved | the Connect click; headless (a CDP click is a trusted gesture) reports `running` |
| §6 Chrome WebTransport + `serverCertificateHashes` shipping behaviour | resolved for Chromium 1248 (headless) | WebTransport stack section; laptop Chrome still open (Toby) |
| §6 server-side stream priority API | resolved | quinn `SendStream::set_priority` (`wt::Session::open_uni`) |
| §6 RFC 6381 `avc1` byte order | resolved | `codecFromSps` gives `avc1.640c2a` from Sunshine's SPS; `isConfigSupported` accepts it and frames decode |
| §6 Linux Chrome hardware decode | open (laptop) | headless: `prefer-hardware` false, `no-preference` true (software) |
| §7 wtransport or web-transport-quinn | resolved: neither | WebTransport stack section (both need ring/aws-lc) |
| design.md §8.3 `num_reorder_frames=0` in the SPS VUI | resolved | Task 3: VUI present, `max_num_reorder_frames=0`, `max_dec_frame_buffering=1` (whether VAAPI or Sunshine wrote it is still open) |
| m0-baseline: is Moonlight Qt on Toby's client | resolved | moonlight-qt 6.1.0 on the laptop (docs/moonlight-compare.md) |
| Task 6: Pointer Lock in headless Chromium | resolved | granted on a CDP click; CDP motion under lock is unusable (Input section) |
| Task 6: Sunshine decodes the page's keys like Moonlight's | resolved | keyAction 3/4, keyCode 0x8041 for `a` (Moonlight's own run: keyAction 3, 8041); live counts below |
| Task 6: Sunshine's Linux keymap knows VK_LSHIFT 0xA0 etc. | open | Sunshine decodes keyCode 0x80A0; the VK to evdev table is in its inputtino submodule, empty in the pinned tree. Toby's step 9 (Shift+letter) checks the effect |

Not covered here: the build-section [U]s (§5, Task 2), SEQPACKET limits (§7 risk 2, Task 3).

## Known follow-ups (after Toby's laptop run)

- Wheel scale on Linux Chrome: about 53 px per notch; if one notch does nothing on bawn, snap the
  sum to whole notches.
- `AvPing` media sockets bind 0.0.0.0 (M1 code); bind them to the local address that matches
  `--host`.
- Timing-dependent tests that may flake in CI: `control_handle_idr_is_rate_limited_to_one_per_100ms`,
  `stream_tap_receives_frames_from_the_tap`, `stream_tap_falls_back_to_rtp`,
  `full_send_buffer_drops_oldest`, `a_send_that_just_fits_evicts_nothing`.

## Toby to run (laptop Chrome): video, audio and input in one sitting, then Moonlight

About 30 minutes. Bawn shows Steam Big Picture; nothing here clicks anything risky on it.

Setup
1. On bawn (needs Sunshine idle; brisk-up refuses otherwise): `cd ~/Projects/brisk && scripts/brisk-up.sh --secs 1800` (30 min; Ctrl-C when done)
2. On the laptop: `ssh -N -L 8443:127.0.0.1:8443 bawn` (leave it open).
3. In Chrome on the laptop, open the `page` URL that brisk-up printed
   (`http://localhost:8443/?wt=https://100.64.0.1:4433/<token>&hash=<sha256>`).
   Click **Connect** (it also starts audio; Chrome only allows sound after a click).

Video (first minute)
4. The stats box should show within a few seconds: `codec avc1.640c2a supported true hw
   prefer-hardware` (or `no-preference` if Chrome has no hardware H.264 decode on the laptop),
   `1920x1080`, `received` and `decoded` climbing about 60 per second, `decode errors 0`,
   `decodeQueueSize` 0-2, `bridge_write-capture p50` about 5-7 ms, and the bawn desktop visible.
   `key requests` climbing while `streams reset` stays 0 means network reordering of per-frame
   streams, not a bug; note the count.

Input (about 5 minutes; do it early so the long soak below runs with input used)
5. The `input` line says `click the video to capture`. **Click the video once.** Chrome hides the cursor and shows
   "Press Esc to show your cursor"; the input line says `captured (Esc releases)`. That click is not sent to bawn.
6. **Getting out: press Esc once.** Chrome leaves Pointer Lock, the page drops keyboard capture and
   releases every key and button on bawn; the line says `click the video to capture`. Escape itself
   is never sent to bawn (so Big Picture "back" is Backspace here). Switching tab, Alt-Tab or
   minimising also releases everything.
7. Mouse: move it slowly, then quickly in circles. The bawn cursor should follow with no jump when
   the lock starts, and `moves` climbs (at most about 125 a second however fast the mouse is).
   Left click on a harmless spot of Big Picture, right click, scroll the wheel a few notches (a
   list scrolls one item per notch or so), the side buttons if the mouse has them: `buttons` and
   `scrolls` climb. Watch for: a stuck button (dragging without holding), scroll running the wrong
   way or several notches per click.
8. Keyboard: arrows and Backspace in Big Picture's menus; type in a text field (Big Picture's search,
   or a terminal on bawn's desktop if one is open): letters, digits, Shift+letters, punctuation
   `-=[];',./\` and the numpad. Characters follow bawn's keyboard layout, not the laptop's. Hold
   a letter for 2 s: it repeats on bawn (Sunshine repeats held keys) and stops the moment you let
   go. Watch for: a key that keeps repeating after release (stuck), or wrong characters.
9. Stuck-key test: hold Shift, then press Esc while still holding it; release Shift. Type a letter:
   it must be lowercase. Same with holding a mouse button and Alt-Tab away: back on the page,
   nothing on bawn is still pressed.
9b. Network-drop test: open a text field on bawn, hold a letter key (it repeats on bawn), and while
   holding it cut the laptop's network (unplug its cable or turn Wi-Fi off). The repeating on bawn
   must stop within about 1-2 s. Cut it on the laptop side, not by stopping the bridge
   (`Ctrl-Z`/`kill -STOP` on bawn would freeze the bridge's watchdog with the key still down).
   Reconnect and reload the page (the old session ends after 10 s); the bridge report's `input`
   line then shows `held_timeouts 1`.
10. Do not use Ctrl+Alt+Shift+<key> on the page: that is Sunshine's own shortcut prefix (it would
    act on bawn's Sunshine, e.g. switch displays). Chrome keeps its own shortcuts (Ctrl+W, Ctrl+T,
    Ctrl+Tab) even while captured.
11. Note `keys`, `buttons`, `moves`, `scrolls`, `dropped` (should be 0) and `release-all`.

Audio (in the same run; play something on bawn: music in a browser on bawn's desktop, or
`pw-play --target sink-sunshine-stereo <file.wav>` over ssh)
12. The `audio` lines should show `running`, `packets` and `decoded` climbing about 200 per second,
    `decode errors 0`, `buffer` between about 15 and 30 ms, `non-silent` climbing while sound plays.
- Glitch or silence? A glitch you hear should line up with `lost` or `underruns` going up. A quiet
  host shows as `non-silent` stopping, or `pauses` going up if Sunshine stops sending; neither is a
  glitch. If you hear a glitch and no counter moved, note the time: it is the laptop's audio device
  or Chrome, not the stream.
- Note `output` (ms the laptop's audio device adds after the ring) for the A/V budget.

Soak (10 minutes, the M2 gate on the real path)
13. Leave the tab in front, sound playing, for 10 minutes; move the mouse and type now and then.
    Pass: `decode errors 0` at the end, the desktop moving when something changes on bawn, no audible
    clicks or dropouts (`lost` and `underruns` at or near 0), no stuck input.
14. Write down from the stats box: `decode p50/p99` and `present p50/p99` (they go into
    docs/m0-baseline.md "client decode" and "client present queue", laptop column), `arrival jitter`,
    `delay over best`, `dropped`/`key requests`, the audio line (`lost`, `underruns [.. ms]`,
    `drift -x/+y`, `buffer`, `output`) and the input line.
15. Close the tab (that releases everything on bawn), then press **Ctrl-C** in the brisk-up terminal
    (brisk-up passes it on to the bridge as SIGTERM)
    (closing the tab alone does not stop the bridge: it waits for a new viewer until `--secs` runs
    out). brisk-up then prints the bridge report: copy the `bridge_write-capture`, `frame_processing(tap-capture)`, `gate(...)`,
    `audio` and `input` lines (`input bad` should be 0; `released` counts keys the bridge let go
    of for you).
16. `scripts/brisk-down.sh` (stock Sunshine back; it refuses while a session is busy).

Moonlight head-to-head (same laptop, same network path, stock Sunshine, about 5 minutes)
17. After brisk-down, start moonlight-qt on the laptop and stream bawn's same app at 1920x1080 60 fps,
    H.264, 20 Mbit/s (the Brisk settings), same decoder choice you want to compare (Settings ->
    Video decoder: hardware first; note which one Moonlight actually used).
18. Press **Ctrl+Alt+Shift+S** in the stream to show Moonlight's performance overlay. Do the same
    things as above for 2-3 minutes (mouse, keys, sound). Write down: `Incoming frame rate from
    network`, `Decoding frame rate`, `Rendering frame rate`, `Frames dropped by your network
    connection`, `Frames dropped due to network jitter`, `Host processing latency` (Sunshine's
    `frame_processing_latency`, compare with the bridge's `frame_processing` line), `Average network latency`, `Average
    decoding time`, `Average frame queue delay`, `Average rendering time`. Take a screenshot of the
    overlay. These are the Moonlight column in docs/m0-baseline.md (client decode = average decoding
    time, present queue = frame queue delay + rendering time).
19. Feel test, both ways: which one's mouse feels faster, any stutter. A note is enough.
20. Quit with **Ctrl+Alt+Shift+Q** (ends the stream and quits the app on bawn), check
    `curl -s http://127.0.0.1:47989/serverinfo | grep -o '<state>[^<]*'` on bawn says
    `SUNSHINE_SERVER_FREE`.
