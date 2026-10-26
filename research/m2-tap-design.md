# M2 design note: Sunshine tap, bridge, Chrome

Date 2026-10-24. Research only: nothing was built, run or installed. All source references are to the pinned Sunshine `v2026.914.233613` (commit 63d35f7) at `~/recon-pinned/Sunshine` (paths relative to `src/` unless they say otherwise). [U] = not verified. The submodules in the pinned tree are empty, so nothing under `third-party/` was read.

## TL;DR

1. **Where to tap:** in `videoBroadcastThread` (stream.cpp:1596), after the SPS/VPS replacements and the latency stamp, before FEC and AES. One `if`: a tap session sends the frame to a unix socket and `continue`s. Other clients are unaffected.
2. **How the session is flagged:** a new SDP line `a=x-brisk.tap:1` in ANNOUNCE. Stock Sunshine ignores unknown attributes (rtsp.cpp:1107-1122), so the bridge works with either build.
3. **Transport:** `AF_UNIX SOCK_SEQPACKET`, one connection per session. The bridge listens on `$XDG_RUNTIME_DIR/brisk/tap.sock`. Sunshine connects when the video ping arrives. Messages are a 44-byte header followed by the Annex B bytes, sent with one `sendmsg`.
4. **Back pressure:** the send is non-blocking. On `EAGAIN` Sunshine drops the frame, asks itself for an IDR, and skips frames until that IDR arrives. The encoder thread never blocks.
5. **Fallback:** if the tap connect fails, Sunshine sends normal RTP. The bridge already decodes RTP (M1), so it falls back too.
6. **Keepalive:** only the first video and audio ping is required (stream.cpp:2133, 2161). Later on, only ENet traffic keeps the session alive (stream.cpp:763, 1329).
7. **RFI:** the VAAPI encoder turns every RFI into an IDR (video.cpp:526-529). Expose frame_idx anyway.
8. **Audio:** unchanged Moonlight path. The bridge forwards Opus (5 ms packets) as WebTransport datagrams. Chrome decodes with `AudioDecoder` and plays through an AudioWorklet.
9. **Install:** build to `~/.local/opt/sunshine-brisk`, point a user-unit drop-in at it, one `sudoq setcap`. The stock `sunshine-bin` stays installed. Rollback = remove the drop-in and restart.
10. **M2 plan:** 6 tasks, H.264 only. The patch is about 60 lines of C++.

## 1. Tap point

**Pipeline**

1. A per-session encode loop runs `encode_run` (video.cpp:2454-2511) or the sync variant (video.cpp:2796).
2. `encode_avcodec` (video.cpp:1826-1898) produces one `packet_raw_avcodec` per `avcodec_receive_packet`. It attaches:
   - `replacements` (video.cpp:1892)
   - `channel_data` = the `session_t*` (video.cpp:1893)
   - `frame_timestamp`
3. The encode loop `raise`s the packet into **one global queue**, `mail::video_packets` (video.cpp:1894, 2439), shared by every session.
4. That queue holds 32 entries (thread_safe.h:850). On overflow it **clears the whole queue** (thread_safe.h:432-436). Any work added to the broadcast thread must therefore be cheap.
5. One `videoBroadcastThread` (stream.cpp:1523) handles all sessions. Per frame it:
   - pops the packet (stream.cpp:1548)
   - applies SPS/VPS replacements on IDR frames (stream.cpp:1565-1573)
   - builds the 8-byte short header and stamps the latency (stream.cpp:1575-1596)
   - copies with header gaps (stream.cpp:1603)
   - splits into FEC blocks, RS-encodes, GCM-encrypts each shard, and does paced `send_batch` (stream.cpp:1598-1835)

**What is available at stream.cpp:1596**

| field | source |
|---|---|
| Annex B bytes, after replacement | `payload` string_view (stream.cpp:1558, 1571). Not a copy unless replaced. |
| frame index | `packet->frame_index()` = `av_packet->pts` = `frame_nr`, int64 (video.cpp:562, 1828). Truncated to u32 on the RTP wire (stream.cpp:1685). |
| IDR | `packet->is_idr()` = `AV_PKT_FLAG_KEY` (video.cpp:553-555) |
| after RFI | `after_ref_frame_invalidation`. Only NVENC sets it (video.cpp:1923), so it is always false on VAAPI. |
| capture time | `frame_timestamp`, `steady_clock`, optional. Missing on duplicate or late frames (video.cpp:1888-1890, 2472-2477). |
| frame_processing_latency | capture to dequeue, in 0.1 ms units (stream.cpp:1585-1596). This is the M0 "host" number. |
| codec | `session->config.monitor.videoFormat` (rtsp.cpp:1203) |

**Parameter sets**

- The codec context flags are reset to `CLOSED_GOP | LOW_DELAY` without `GLOBAL_HEADER` (video.cpp:2077-2078). So SPS/PPS (and VPS for HEVC) travel in-band on IDR frames, as Annex B with start codes. This matches what Moonlight expects [U: ffmpeg behaviour, moonlight parser not read].
- If the probe found no VUI, Sunshine rewrites the SPS (H.264) or SPS+VPS (HEVC) on IDR frames (video.cpp:1863-1885, inject mode at video.cpp:2342-2343). The rewrite sets `max_num_reorder_frames=0` and `bitstream_restriction_flag=1` (cbs.cpp:171-176).
- Tapping after line 1573 gives the bridge exactly the bytes Moonlight would decode.
- [U] Whether bawn's VAAPI SPS already has a VUI. Log the first SPS in Task 3.

**Threads and ownership**

- The broadcast thread owns the `unique_ptr` for the duration of one loop iteration. A tap that `sendmsg`s synchronously in that iteration needs no extra lifetime handling.
- `replacements` is a raw pointer into the encode session (video.cpp:534, 1892). The existing code already relies on that; the tap adds no new risk.
- Per-session tap state is written in `videoThread` before `video::capture` starts (stream.cpp:2143). It is then read by the broadcast thread after the first packet. The queue's mutex orders the two.

**Minimal patch**

1. `stream.h:28-42` `config_t`: add `bool brisk_tap = false;`.
2. `rtsp.cpp` near 1161 (inside the existing try): `config.brisk_tap = args.contains("x-brisk.tap"sv);`. `alloc` already copies `config` into the session (stream.cpp:2320).
3. `stream.cpp:505-518` `session_t::video`: add these fields:
   - `boost::asio::local::seqpacket_protocol::socket` (optional), or a raw fd with RAII
   - `bool tap_need_idr`
   - `uint32_t tap_dropped`
4. `stream.cpp:2133-2136` `videoThread`, after `recv_ping` succeeds:
   - if `config.brisk_tap`, open a non-blocking connect to the tap path and set `SO_SNDBUF` to 4 MiB (bawn's `wmem_max` is 4194304)
   - send nothing extra
   - on failure, log a warning and leave the socket unset, so the session falls back to RTP
5. `stream.cpp`, between 1596 and 1598: `if (session->video.tap) { brisk_tap_send(*session, *packet, payload, frame_header.frame_processing_latency); continue; }`. The skipped work is FEC, encryption, `lowseq` and pacing. None of it is needed when there is no RTP.
6. Close the socket when the session is destroyed. The bridge then sees EOF.

That is about 60 lines in 3 files. No encoder, capture or audio code changes.

`brisk_tap_send` does the following:

- If `tap_need_idr` is set and the frame is not an IDR, increment `tap_dropped` and return.
- Otherwise `sendmsg(MSG_DONTWAIT | MSG_NOSIGNAL)` with iov = {header, payload}.
- On `EAGAIN`: set `tap_need_idr`, `session->video.idr_events->raise(true)` (the same call the control handler uses, stream.cpp:1193-1197), increment `tap_dropped`.
- On `EPIPE` or another error: close the socket and `session::stop` (the bridge died).

**Caveat:** the broadcast thread is shared. If a normal Moonlight client streams at the same time, its pacing sleeps (stream.cpp:1773-1781) delay the tap frames that queue behind it. This is fine for one user [U impact].

## 2. Transport between Sunshine and the bridge

| | unix SEQPACKET | unix STREAM + length prefix | memfd ring + eventfd | localhost UDP |
|---|---|---|---|---|
| copies | 2 (into kernel, out of kernel) | 2 | 1 (into ring) plus mmap reads | 2, plus fragmentation over 64 KiB |
| framing | kernel keeps message boundaries | must build our own | must build our own | 64 KiB limit forces fragmenting |
| non-blocking drop | atomic: whole message or `EAGAIN` [U af_unix] | partial writes, so Sunshine needs a resend buffer | writer checks free space; drop is easy | the kernel drops silently |
| permissions | socket path in a 0700 dir, plus `SO_PEERCRED` | same | fd passing needs a socket anyway | any local user can send to the port |
| restart detection | EOF / `EPIPE` | same | needs a side channel | none |
| patch size | smallest | +resend buffer | largest (ring, futex/eventfd, layout) | +fragmenting |

**Recommendation: SEQPACKET.**

- At 7-20 Mbit/s a frame averages 15-40 KB. The extra kernel copy costs tens of µs [U], far below the 2 ms budget, so a ring buffer buys nothing measurable.
- The all-or-nothing send gives the drop policy for free.
- Limits:
  - message size ≤ `SO_SNDBUF`, about 4 MiB here [U exact formula]
  - the receive queue is capped by `net.unix.max_dgram_qlen` [U; applies to seqpacket per af_unix]
  - Both are fine: a 1080p IDR frame is roughly 100-500 KB [U].

**Who listens:** the bridge.

- It creates `$XDG_RUNTIME_DIR/brisk/` (0700) and binds `tap.sock` (`/run/user/1000` on bawn).
- Sunshine connects once per tap session, so the patch needs no accept thread.
- The bridge checks `SO_PEERCRED.uid == getuid()`. Both run as toby: Sunshine is a user service (`/usr/lib/systemd/user/app-dev.lizardbyte.app.Sunshine.service`, aliased as `sunshine.service`).
- Socket path: `getenv("BRISK_TAP_SOCK")`, else `$XDG_RUNTIME_DIR/brisk/tap.sock`. No new Sunshine config key.

**Detecting the tap**

- The bridge sends `x-brisk.tap:1`, sends the video ping, and then waits for whichever comes first:
  - a tap connection whose header `connect_data` matches its session: tap mode
  - RTP on the video socket: stock Sunshine or a failed connect, so use the M1 `StreamRx` path
- Sunshine restarts: the tap socket gets EOF and ENet times out. The bridge relaunches (needs the M2 reconnect loop, with fresh rikey per final-fix-list item 4).
- Bridge restarts: Sunshine gets `EPIPE` and stops the session.

**Framing.** Little-endian, 44 bytes, followed by the Annex B payload.

```
off size field
0   4   magic 'BTAP' (0x50415442)
4   1   version = 1
5   1   flags: b0 idr, b1 after_rfi, b2 no_capture_ts (dupe), b3 gap (frames dropped before this one)
6   2   header_len = 44 (lets the bridge skip future fields)
8   4   connect_data (session->control.connect_data, stream.cpp:543; the bridge knows it from X-SS-Connect-Data)
12  4   codec (videoFormat: 0 H.264, 1 HEVC, 2 AV1)
16  8   frame_index (int64 pts)
24  8   capture_ns  (frame_timestamp, CLOCK_MONOTONIC ns; 0 if none)
32  8   tap_ns      (steady_clock::now() at send)
40  4   dropped_total (tap_dropped)
44  ..  Annex B bytes (payload_len = message length - header_len)
```

- libstdc++ `steady_clock` is `CLOCK_MONOTONIC` [U], so the bridge (same box) can compare the timestamps directly. Rust `Instant` is not usable for this; use `clock_gettime(CLOCK_MONOTONIC)`.
- What the timestamps measure:
  - `tap_ns - capture_ns` = the same quantity as `frame_processing_latency`
  - `bridge_recv_ns - tap_ns` = tap transport
  - `wt_write_ns - capture_ns` = the M2 gate's "host encode + bridge + tap"

## 3. Session model

- **Do we need continuous pings? No.**
  - `videoThread`/`audioThread` each block in `recv_ping` for the first ping, up to `ping_timeout` (stream.cpp:2091-2124, 2133, 2161), and only then start capture.
  - After that, `pingTimeout` is refreshed only by ENet events (stream.cpp:763) and checked in the control loop (stream.cpp:1329-1332).
  - So: one video ping, one audio ping, and keep the ENet periodic ping (M1 already does all of this).
  - Continuing the 500 ms A/V pings is harmless. Keep them, since moonlight does.
- **Option (a), receive and ignore RTP:** no Sunshine change beyond the tap, but Sunshine does FEC, GCM and pacing for nothing, and the bridge has to drain about 10 Mbit/s. It also contradicts design call 11.8.
- **Option (b), recommended:** the `continue` at stream.cpp:1597 described above. The flag is the SDP attribute because:
  - it is per session, so other clients are untouched
  - stock Sunshine ignores it (rtsp.cpp:1107-1122)
  - it needs no launch-URL or config change
  - Security: the flag only stops RTP for the session that asked, and the frames go to a uid-checked socket.
- **IDR:** the bridge sends 0x0302 over the control stream as in M1 → `idr_events` (stream.cpp:1193-1197). The tap also raises it after a drop.
- **RFI:** on VAAPI it becomes an IDR (video.cpp:526-529). frame_idx is in the header, so a later RFI over NVENC only needs the bridge to send 0x0301 with indices (m1-notes RFI). It is not needed for M2.

## 4. Audio: workable

- **Bridge:** it already parses RTP audio, the RS 4+2 FEC and AES-CBC (M1 Task 10/12 and m1-notes). It sends each Opus packet as one WebTransport datagram with seq and a 48 kHz timestamp, without re-encoding.
- **Gotchas**
  1. Packets are 5 ms: `packetDuration` 5 (rtsp.cpp:1130, brisk sdp.rs:122), `frame_size` = 240 samples (audio.cpp:137). That is 200 datagrams/s, fine for QUIC [U].
  2. The encoder is CBR, RESTRICTED_LOWDELAY (audio.cpp:126-131). Stereo is a single coupled stream, so a WebCodecs `AudioDecoder` config `{codec:"opus", sampleRate:48000, numberOfChannels:2}` with no description should work [U]. 5.1 needs a multistream OpusHead description [U]; keep stereo for v1.
  3. Loss: a WebTransport datagram is not retransmitted. Opus PLC in WebCodecs is not exposed [U]. The worklet should insert silence or repeat the last 5 ms with a fade, and count the event.
  4. Playout: AudioWorklet ring of about 20-40 ms. Drift between host and client clocks needs slow resampling or occasional drop/insert. The M5 gate (A/V < 40 ms) covers sync.
  5. `AudioContext` needs a user gesture to start (Chrome autoplay policy [U]). The connect click provides it.
  6. Silence: Sunshine may stop sending audio packets while the host is silent unless `continuous_audio` is set (rtsp.cpp:1248-1251) [U behaviour]. The worklet must handle starvation without counting it as loss.

## 5. Building and installing on bawn

**Base it on** `packaging/linux/Arch/PKGBUILD`.

- depends: avahi curl gtk3 libayatana-appindicator libcap libdrm libevdev libmfx libpipewire libpulse libva libx11 libxcb libxfixes libxrandr libxtst miniupnpc numactl openssl opus qt6-base qt6-svg udev vulkan-icd-loader. They are already installed, since `sunshine-bin` has the same list.
- makedepends: appstream appstream-glib cmake desktop-file-utils gcc git make nodejs npm python-jinja shaderc. **Not installed on bawn:** cmake, shaderc, python-jinja, appstream, appstream-glib. nodejs, npm and desktop-file-utils are present.
- The appstream tools are only needed for the PKGBUILD's validate step. A plain cmake build skips them [U].
- Boost is pinned `EXACT 1.89.0` (cmake/dependencies/Boost_Sunshine.cmake:6,36). Arch has 1.92, so CMake falls back to downloading Boost (lines 37-80). That means network access at configure time and a few extra minutes.
- FFmpeg comes as prebuilt binaries from the `build-deps` submodule's release (cmake/dependencies/ffmpeg.cmake, common.cmake:99-100): another network download.
- The web UI is built with npm, which also needs network.
- CUDA: `-DSUNSHINE_ENABLE_CUDA=OFF -DCUDA_FAIL_ON_MISSING=OFF` (PKGBUILD build()). VAAPI does not need CUDA.
- Submodules: `git submodule update --init --recursive --depth 1` (PKGBUILD prepare). The pinned tree has empty submodule dirs, so build from a fresh clone of tag `v2026.914.233613` plus our patch, never from `recon-pinned`.
- gcc: bawn has gcc 16, while the PKGBUILD names 15 and builds with `BUILD_WERROR=ON`. Use `-DBUILD_WERROR=OFF` to avoid failing on new warnings [U].
- Build time on 20 cores: about 5-10 min, plus downloads [U]. Disk: about 2-4 GB of build tree [U]. The installed size is about 25 MiB (`sunshine-bin`).

**Install: side by side, not a package swap (recommended)**

1. `cmake -S src -B build -DCMAKE_INSTALL_PREFIX=$HOME/.local/opt/sunshine-brisk -DSUNSHINE_ASSETS_DIR=share/sunshine -DSUNSHINE_EXECUTABLE_PATH=$HOME/.local/opt/sunshine-brisk/bin/sunshine -DBUILD_TESTS=OFF -DBUILD_DOCS=OFF -DSUNSHINE_ENABLE_CUDA=OFF -DCUDA_FAIL_ON_MISSING=OFF -DBUILD_WERROR=OFF`, then `cmake --build build`, then `cmake --install build`. No root.
2. **sudoq (the only root step):** `sudoq run --why "KMS capture caps for patched sunshine" -- setcap cap_sys_admin,cap_sys_nice+p ~/.local/opt/sunshine-brisk/bin/sunshine`.
   - This mirrors sunshine.install `do_setcap`.
   - /home is btrfs without `nosuid`, so file caps apply.
   - Every rebuild rewrites the binary and drops the caps, so it is one sudoq approval per reinstall.
3. Add a user drop-in `~/.config/systemd/user/app-dev.lizardbyte.app.Sunshine.service.d/brisk.conf` with `[Service] ExecStart=` and `ExecStart=%h/.local/opt/sunshine-brisk/bin/sunshine`. The existing `override.conf` stays. Then `systemctl --user daemon-reload && systemctl --user restart sunshine`. No root.
4. **Rollback:** `rm brisk.conf && systemctl --user daemon-reload && systemctl --user restart sunshine`.
5. The patched build uses the same `~/.config/sunshine` (pairing, apps), so the bridge stays paired.
6. udev rules for uinput/uhid already come from the stock package.

The alternative is a `sunshine-brisk` PKGBUILD with `provides=(sunshine) conflicts=(sunshine sunshine-bin)`. That needs sudoq for `pacman -U`, and rollback needs the stock package file. **None is cached** (not in /var/cache/pacman/pkg, ~/.cache/yay or ~/.cache/paru), so rollback would mean re-fetching from the AUR. The side-by-side install avoids this.

Advice either way: before the first swap, download the v2026.914.233613 release `.pkg.tar.zst` into `~/pkgs/` [U whether sunshine-bin wraps that exact file].

## 6. Bridge to browser (sketch)

Spec facts below are from the W3C TR drafts, fetched today. Chrome shipping behaviour is [U].

- **Video:** one unidirectional stream per frame.
  - Stream header: frame_idx u32, flags, capture_ns, bridge_ns, then the Annex B bytes, then FIN.
  - `sendOrder` = frame index, so newer frames go first. The spec says lower sendOrder data in the same sendGroup starves until higher sendOrder data is sent (WebTransport §4.3). In our design the bridge sets it on the server side in quinn: stream priority [U API].
  - Datagrams for small P-frames add a second path and a reassembly step. Skip them in M2.
- **Back pressure on the browser leg:** if frames are more than about 3 behind (frames still unacked), reset the old streams, drop until the next IDR, and request an IDR over control. This is the same rule as the tap.
- `outgoingMaxAge` exists on datagram streams (WebTransport §5.3, ms, null by default) [U Chrome]. Not needed with streams.
- **WebCodecs**
  - `VideoDecoderConfig{codec, optimizeForLatency:true, hardwareAcceleration:"no-preference"}` with no `description`. Without a description the AVC bitstream is Annex B, and each key chunk must carry its SPS/PPS (AVC registration). Sunshine's IDRs do.
  - `optimizeForLatency` = minimize chunks decoded before output (WebCodecs §7.6).
  - The codec string is `avc1.` + 6 hex chars taken from SPS profile_idc, constraint byte and level_idc (RFC 6381 [U byte order, not fetched]). Parse it from the first IDR, then `isConfigSupported`.
  - After `configure()` (and `flush()`) the first chunk must be a key chunk, otherwise `decode` throws `DataError` (WebCodecs, key chunk required). So the client waits for an IDR at start and after every reset.
  - Try `prefer-hardware` and fall back if unsupported. Linux Chrome hardware decode is uncertain [U].
- **Present policy:** the decoder output callback keeps only the newest `VideoFrame` and `close()`s the older ones. `requestAnimationFrame` draws the newest frame to the canvas. Watch `decodeQueueSize`: if it is above 2, wait for the next IDR.

## 7. M2 breakdown, risks, decisions

**Task 1: Sunshine tap patch.**
- Files: `patches/sunshine/0001-brisk-tap.patch` (src/stream.h, stream.cpp, rtsp.cpp) and `patches/sunshine/README.md` (base tag, apply steps).
- Interface: the header from section 2, `a=x-brisk.tap:1`, `BRISK_TAP_SOCK`.
- Verification:
  - the patch applies cleanly to tag v2026.914.233613
  - the diff is under 100 lines
  - a gtest for header packing in Sunshine's `tests/unit/` (it builds only with `BUILD_TESTS=ON` [U])

**Task 2: Build, install, rollback script.**
- Files: `scripts/sunshine-brisk.sh {build,install,enable,rollback,status}`.
- Root: only `setcap`, via sudoq.
- Verification:
  - enable → `brisk-probe stream --secs 60` (no tap flag) still passes the M1 gate on the patched build (regression check)
  - rollback → the stock binary is running (`systemctl --user show -p ExecStart`)
  - the round trip works twice

**Task 3: Bridge tap receiver.**
- Files: `crates/brisk-proto/src/tap.rs` (sans-io header parse), `crates/brisk-client/src/tap.rs` (listener, `SO_PEERCRED`, RTP fallback), `fixtures/tap/`.
- This task also takes the M2 blockers: the ControlSession command channel (for IDR from the tap/browser), typed errors and the hot-path allocations.
- Interface: `TapFrame{idx:u64, idr:bool, capture_ns, tap_ns, dropped:u32, annexb:Bytes}`.
- Verification, live 60 s tap session:
  - frames ≥ 3420
  - idx contiguous except for the counted drops
  - IDR within 2 s of each request
  - 0 RTP datagrams received in tap mode
  - `recv - capture` p50/p99 printed next to `frame_processing_latency`

**Task 4: WebTransport bridge and page (video only).**
- Files: `crates/brisk-bridge/` (quinn plus wtransport or web-transport-quinn [U], tailscale cert), `web/` (Astro/Svelte shell, framework-free `video.ts`).
- Verification:
  - Chrome on Toby's client shows 1080p60
  - the page's stats show 0 decode errors over 10 min
  - `bridge_write - capture` p50 ≤ `frame_processing_latency` p50 + 2 ms (the M2 gate)

**Task 5: Audio forward.**
- Files: bridge datagram path, `web/audio.ts` (AudioDecoder and AudioWorklet).
- Verification:
  - 10 min of audio
  - underrun and loss counters shown
  - no worklet glitches on a loopback path (ears plus counters)

**Task 6: Mouse and keyboard, plus the M2 gate run.**
- Files: `web/input.ts`, bridge input → `ControlSession`. Update `docs/m0-baseline.md`.
- Verification:
  - key and mouse events reach the host (xev or a game)
  - the full M2 gate table is recorded

**Risks**

1. Chrome's H.264 decode path on Toby's client may add frames of delay despite `optimizeForLatency`, or run in software [U]. Measure it in Task 4 before blaming the tap.
2. SEQPACKET size and queue limits are [U] until Task 3 measures them. The fallback is STREAM framing with a resend buffer.
3. The build needs network (Boost, FFmpeg, npm) and may hit gcc 16 warnings.
4. Sharing the broadcast thread with a concurrent Moonlight client adds jitter.
5. With an IDR after every drop, sustained back pressure on the browser leg shows up as IDR storms. Rate-limit IDR requests to about 1 per 100 ms [CALL].

**Decisions for Toby**

1. Install method: side-by-side `~/.local` plus a unit drop-in (recommended, rollback is one command) vs replacing the package.
2. When the tap is requested but the bridge socket is missing, Sunshine falls back to RTP (recommended) vs fails the session.
3. M2 is H.264 only, with HEVC in M3 or later (recommended: less SPS parsing and a simpler codec string).
4. Sunshine-side drop policy: on a full socket, drop and self-request an IDR (recommended) vs block briefly (up to 1 ms).
5. The build downloads Boost, FFmpeg and npm packages at configure time. OK to allow those downloads?
