# Brisk: design

Status: DRAFT for Toby's approval. Nothing is built. Date: 2026-10-24.
Tags: [V] checked in source/docs this session, [U] unverified or from memory, [CALL] my choice, not agreed.

## TL;DR

1. Brisk = the fastest, cleanest web Moonlight client. Browser plays your Sunshine games.
2. Why: moonlight-web has bad latency, clunky UI, an HTML/C++ mess.
3. It is step 1 of a bigger "control everything from one site" project.
4. Pure Rust, from scratch: protocol client + bridge. No C/C++ in the bridge.
5. Bridge runs on bawn next to Sunshine. Browser: Chrome only, WebTransport + WebCodecs.
6. Sunshine gets a small patch (a "tap"): encoded frames skip FEC and AES and go straight to the bridge.
7. Network: Tailscale mostly, LAN sometimes. Use of a real `*.ts.net` cert, so no cert-hash hacks.
8. Mostly controller games, some desktop. Controller input is the priority.
9. Moonlight has no protocol tests, so we verify with golden captures from real Sunshine plus live tests.
10. Measure latency FIRST (M0, software timestamps only), then build up a ladder. Fancy research is optional and kept out of core.

## 1. Intent and why

Toby plays mostly controller games on bawn (Dell OptiPlex, Arch) from other machines, over Tailscale. Existing web clients feel laggy and look clunky. Brisk should feel as close to local as the physics allows, and have a clean UI and clean code. The bridge and protocol code become the base for the larger one-site-control-everything project, so the code has to be something Toby wants to live in.

## 2. Goals, non-goals, anti-goals

Goals
1. Lowest practical input-to-photon latency on a Tailscale path, and a visible latency number in the UI.
2. Smooth: stable frame pacing beats a low average.
3. Controller first (rumble back, low-jitter input), desktop (mouse, keyboard) second.
4. Small clean Rust codebase, one language end to end for protocol + bridge. Web UI is as small as WebCodecs allows.

Non-goals (for now)
1. Firefox, Safari, mobile. Chrome only. Firefox is later; do not design it out.
2. Non-Sunshine hosts (GFE). Targeting Sunshine only lets us drop old protocol generations (see section 5).
3. Multi-user, public internet exposure, other transports like WebRTC.
4. The "control everything" site beyond this client.

Anti-goals (things that would make this a failure)
1. A feature-for-feature moonlight-web clone with the same lag.
2. A pile of tunables nobody can reason about. Adaptive logic must have a safe fixed fallback.
3. Latency claims without a measurement behind them.
4. Core scope absorbing research tracks (section 8).

## 3. Constraints (given)

1. Pure Rust for protocol + bridge. The Sunshine tap is the one allowed non-Rust piece (a patch to Sunshine itself).
2. Bridge on the same box as Sunshine. Sunshine is C++, commit d0b4a88 was the one read [V].
3. Browser to bridge: WebTransport (QUIC/HTTP3) carrying video/audio/input, WebCodecs for decode.
4. Chrome only now.
5. Licensing: Toby does not care.
6. bawn is reached over the network. Never touch bond0 / NetworkManager profiles or disk layout without asking. Root commands go through `sudoq`. Builders must follow this.

## 4. Decisions Toby made (as made)

1. Fastest, most performant, cleanest web Moonlight client.
2. Pure Rust from scratch for protocol and bridge.
3. WebTransport + WebCodecs.
4. Bridge on the same box as Sunshine.
5. Browser over Tailscale mostly, LAN sometimes. Mostly controller games, some desktop.
6. Option B: patch/tap Sunshine (tap `videoBroadcastThread`, before FEC + encryption, `src/stream.cpp` ~line 1536 [V]) instead of speaking plain Moonlight to Sunshine over loopback.
7. Chrome only now, Firefox later.
8. Licensing: do not care.
9. "If Moonlight has tests we must pass them." Result: it has none (section 7).
10. Front end: Astro + Svelte + Bits UI (headless Svelte components; decided 2026-10-24, Toby chose Bits UI after I flagged that Base UI is React-only [U, not verified here]).
11. Visual direction: take design inspiration from Linear (decided 2026-10-24). My reading, [CALL] until Toby reacts: dark-first with a light theme, dense and calm, keyboard-first (a command palette for apps, settings and connect/disconnect), restrained accent color, fast and minimal motion, nothing that competes with the video. In-stream UI (latency/stats overlay, menu) stays out of the way and never touches the video path. Details are for the UI task; use the frontend-design skill then and show mockups before building.

## 5. Architecture sketch (my reading of the above; details are the builder's)

```
Sunshine (patched) --tap: encoded frames--> Brisk bridge (Rust) --WebTransport--> Chrome (WebCodecs)
     ^----- Moonlight control/input (loopback) ------'<--- input datagrams, control ---'
```

Facts about the Moonlight side [V unless marked]:
1. moonlight-common-c has no protocol tests and no pairing/HTTP code (each client app does that). The source is the spec; no written protocol doc was found in either repo.
2. Pieces to implement: pairing (HTTP/HTTPS, PIN-derived AES-128 ECB, SHA-256 RSA), RTSP/SDP (about 70 `x-nv/x-ml/x-ss` attributes, version-gated), control stream on ENet UDP 47999 (the hardest part: versioned packet tables, AES-GCM wrapper), input (rides control), audio (Opus, RS 4+2, AES-CBC), video (RTP + RS FEC over GF(2^8), nanors).
3. Sunshine-only targeting lets us ignore Gen3/4/5 packet tables, CBC input and the legacy 47995 path; only the "Gen7Enc" paths plus Sunshine extensions (`0x55xx` input, rumble `0x5500`..) are needed. Sunshine reports appversion `7.1.431.-1`.
4. Client can already ask for IDR (`stream.cpp:1206`) and reference-frame invalidation (`:1222`). Bitrate is set once at init (`video.cpp:2218`) and FEC percent is a global read per frame (`stream.cpp:1611`): runtime changes need Sunshine patches.
5. ENet: [CALL] write a minimal Rust reimplementation of only what Sunshine uses (no C FFI, per the pure-Rust rule). ENet is under-documented; this is the main M1 effort risk. Unverified: whether an existing Rust crate is good enough.

## 6. What is left to the builder

1. Tap mechanism: how frames get from Sunshine to the bridge (unix socket / shm), framing, and what metadata rides along (frame index, type, capture timestamp).
2. How the bridge establishes the session. [CALL] Bridge still acts as a normal Moonlight client over loopback for pairing, RTSP, control and input, and only the video path is tapped.
3. Wire format on WebTransport: datagrams vs streams per media type, packetization, frame header.
4. Decoder/renderer choice (Canvas / WebGL / WebGPU `importExternalTexture`), worker layout, audio via AudioWorklet.
5. Rust crate choices (QUIC: quinn is the obvious candidate [U], not evaluated), build layout, UI approach.
6. Test harness design (section 7). Fixtures must be scrubbed of keys/PINs before they enter the repo [CALL].
7. Whether rumble/gyro/DualSense features (WebHID, Chromium only) are in v1.

## 7. Verification (answer to "pass Moonlight's tests")

1. There are no tests to pass [V]. Instead:
2. Golden fixtures: capture real Sunshine traffic (pairing, RTSP, control, input, video/audio packets) and replay them against our parsers byte for byte.
3. Port the vectors from Sunshine's `tests/unit/test_http_pairing.cpp` (841 lines, parameterised handshake, best reuse candidate [V]), plus `test_crypto.cpp`, `test_rtsp.cpp`, `test_stream.cpp`. Those are server-side tests used as vectors, not a client harness.
4. FEC: nanors has its own Perl RS tests [V]; cross-check our RS against nanors on random schemes. (Sunshine has no FEC/RTP framing tests [V].)
5. Live integration tests against a real Sunshine on bawn: pair, launch, stream N seconds, check frame count, no decode errors, input echo.
6. Latency verification: see ladder.

## 8. Techniques folded in (ranked practical wins; most are [U] from agent research, gains are estimates)

1. Measure first: photodiode rig (display flash to sensor) plus in-band timestamps. Software-only photon measurement was judged unsound [U].
2. Check direct vs DERP on Tailscale. DERP is TCP (head-of-line blocking) [U]; one path per peer. Surface the path in the UI.
3. Zero-reorder stream: no B-frames, `num_reorder_frames=0` in SPS VUI, one frame per chunk [U]. Sunshine's NVENC already uses P1 and zeroReorderDelay and a one-frame VBV [U, from agent reading].
4. Skip FEC and AES on the loopback leg (the tap), not on the browser leg (QUIC encrypts it; FEC there is added back adaptively in item 7). Estimated 1-5 ms and 1-3 cores [U estimate].
5. Tiny playout buffer sized by a delay quantile, drop-oldest, present on arrival (render only the newest frame).
6. Input redundancy: each datagram carries the last N gamepad states.
7. Per-frame adaptive FEC for the browser leg (needs the FEC percent patch).
8. RFI (reference-frame invalidation) instead of IDR on loss.
9. Host capture pacing: KMS capture is timer-paced (average wait half a frame) [V in source]; PipeWire/portal capture can be event-driven. Game render queue (1-2 frames) is the biggest unmeasured term. Measure before changing capture.
10. WebTransport `outgoingMaxAge` / `sendOrder` to expire stale data (in spec; Chrome shipping status [U]).
11. `tailscale cert` for a trusted `*.ts.net` cert so plain `new WebTransport(url)` works, no `serverCertificateHashes` [V per docs, untested here]. Our bridge binds UDP itself and reloads cert files on renewal (about 90 days).

## 9. OPTIONAL research tracks (NOT core scope)

Details and caveats in `research/novelty-loop.md`. Search budget ran out, so no novelty claim is verified. Nothing here is claimed as new; all of it is "no prior art found in limited searching".
1. C17: client-side rotation reprojection learned from video (online identification of input to camera motion on unmodified games). Novelty about 65%, unverified. Sim looks good but is optimistic (plant is in model family). Real-game test needed. Active EA patent US10773162 and US10595041 claims unread: freedom-to-operate risk. Blockers listed in the log.
2. Fugu-style per-frame delivery-latency PMF driving a joint bitrate/FEC/playout MPC (C8). Partially done in the literature (Fugu, Nebula, Vidaptive, etc.); the delta is narrow. Needs logged data from Toby's own path first.
Gate: only start after M3, and only if M0 numbers show the corresponding term is large.

C17 offline test result (2026-10-24; the full write-up stayed local and is not in the repo): FAIL vs the threshold (RMS <= 0.5 deg and p95 <= 1 deg at 80 ms, >= 70% better than no warp). Software-rendered scene only (no real game), best case 1.22 deg RMS / 1.94 deg p95 on decoded frames. The input->rotation map is learnable (0.02-0.46 deg given ground-truth rotations); the bottleneck is measuring rotation from decoded frames (parallax, ADS+aim assist, idle jitter, stick input worse than repeat-last-rotation). Simulated improvement ratios are inflated (user model too fast); do not quote them. Status: C17 parked, not planned.

## 10. Milestone ladder (gates are measurable; numbers are targets, [CALL] until M0 gives a baseline)

| # | Milestone | Gate |
|---|-----------|------|
| M0 | Measure baseline: stock Sunshine + moonlight-web (or Moonlight desktop) on the real path, software timestamps only (no photodiode rig) | Written table of capture, encode, network, decode and present-queue stages, direct vs DERP; total is host-to-client, not true photon |
| M1 | Protocol client in Rust: pair, RTSP, control, input; no browser | Golden-fixture tests pass; pairing vectors pass; live: 60 s stream received, 0 parse errors |
| M2 | Tap + bridge forwards frames, Chrome decodes and shows (Canvas), mouse/keyboard | Live: 10 min at 1080p60, 0 decode errors; bridge+tap adds <= 2 ms median per frame over the host encode (in-band stamps) |
| M3 | Controller: input datagrams with redundancy, rumble back, playout/present policy | Input-to-photon p50 and p99 no worse than M0 baseline minus the M2 savings; controller input loss of 5% in a netem test causes no stuck inputs |
| M4 | Pacing + loss: RFI, adaptive FEC, drop-oldest, direct/DERP indicator | Under netem 1-5% loss and 30 ms jitter: p99 frame-to-frame time within 2x of clean, no freeze > 100 ms |
| M5 | Polish UI, audio, reconnect, cert reload | Daily-driver for one week; audio/video offset < 40 ms [CALL] |
| R | Optional research tracks | Per-track experiment defined in the research log |

## 11. Open questions / my calls (not agreed; Toby may overrule)

1. [CALL] Bridge still speaks Moonlight control/input over loopback; only video is tapped. (Fewer Sunshine changes than also tapping input.)
2. [CALL] Audio is not tapped in v1; it uses the normal Moonlight audio path via the bridge.
3. [CALL] Sunshine patch lives as a small patch set on top of d0b4a88-ish upstream, kept minimal so rebasing is cheap.
4. [CALL] Target H.264 and HEVC first, AV1 later; probe `VideoDecoder.isConfigSupported` at runtime. Hardware decode on Linux Chrome is a known weak spot [U].
5. [CALL] Latency gates above are targets; they get replaced by real numbers after M0.
6. SUPERSEDED by Toby's decision 4.10: the web UI is Astro + Svelte + Bits UI, not plain TypeScript/HTML. The WebTransport/WebCodecs/WebHID code stays framework-free TypeScript modules (a Svelte component mounts them); the video path must not go through reactive state (frames go decoder -> canvas directly). [CALL]
7. Unchecked: default RSA key size in Sunshine pairing, whether nanors rs16 variants are used, the unpair endpoint, Chrome Local Network Access prompt on 100.x addresses, Chrome `outgoingMaxAge` support, whether `tailscale cert` hostname exposure in CT logs matters to Toby.

8. [CALL] Tap suppresses Sunshine's own video UDP send for the bridge session, so no CPU is wasted on packets nobody reads.
9. [CALL] Patches by milestone: M2 tap only; M4 adds runtime FEC percent (and bitrate if M0 shows it matters). Everything else stays upstream.
10. [CALL] Audio ships in M2 via the normal Moonlight path (silent video-only is not usable); the M5 gate is only the sync tolerance.
11. [CALL] QUIC stack: quinn (with web-transport-quinn or wtransport) unless the builder finds a blocker [U, not evaluated].
12. [CALL] The bridge serves the page over HTTPS (TCP) and WebTransport (UDP) with the same `tailscale cert`. LAN without a `.ts.net` name: reach it via the ts.net name anyway [U, unchecked], else `serverCertificateHashes` fallback with a short-lived ECDSA cert.
13. [CALL] netem tests run in a network namespace / veth pair or loopback only, never on bond0.
14. [CALL] Bridge pairs once as a fixed Moonlight client; keys stored in a 0600 file under the bridge user's config dir; PIN entered by Toby in Sunshine's web UI.
15. [CALL] Dev happens on bawn (the only box with Sunshine and a GPU). Root steps via `sudoq`, nothing installed without asking.
16. Status: this whole doc is a draft; none of the [CALL] items are approved until Toby says so. Research tracks are ignorable for planning.

## 12. Answers from Toby, and what they changed

1. M0 rig: NO photodiode or camera. M0 uses software timestamps only (in-band stamps, per-stage host and client clocks). Absolute input-to-photon is not measured; the display and present stage is estimated, and gates compare RELATIVE to the M0 baseline. A rig can be added later.
2. Sunshine: YES, replace bawn's running Sunshine with the patched build, installed via sudoq (Toby approves each step; keep the stock package so rollback is one command).
3. Streams: 1080p60 first, 4K60 later. [CALL] 8-bit SDR H.264/HEVC for v1, no HDR or 10-bit, no 120 Hz. (An earlier draft misread "8bit do" as a bit depth; it is the controller in item 4. The 8-bit choice is my default, not Toby's.)
4. Controller: 8BitDo Ultimate 2, one pad. [CALL] Run it in XInput mode (2.4G dongle or wired) so Chrome's Gamepad API sees a standard Xbox-style layout; rumble via the Gamepad API vibration actuator [U, test on the real pad in M3]. Extra buttons, gyro and any non-standard features of this pad are deferred and [U]: not checked whether it exposes them in XInput mode. Poll rate over the 2.4G dongle vs Bluetooth affects input latency; measure in M3 [U]. No PlayStation features in v1.
5. Product shape: left to me. [CALL] One bridge binary serves the page, lists and launches Sunshine apps, and speaks WebTransport. Access is tailnet only by default: the bridge asks Tailscale's local API who the peer is (whois) and allows only Toby's own devices. LAN access without Tailscale needs a one-time token. No user accounts in v1.

6. Patents: Toby is fine using patented techniques (hobby project). Effect: the EA/ZeniMax patent risk no longer blocks optional track C17. It is not a reason to claim novelty, and it would matter if Brisk is ever shared or sold.

Nothing open from Toby; waiting on "approved".
