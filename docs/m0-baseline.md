# M0 baseline: stage latency table

Software timestamps only. There is no photodiode or camera, so true input-to-photon is NOT measured. "Total" is host-to-client, not photon. The display/present stage is estimated.

Cells: `PENDING (Toby)` = needs Toby's client machine; `PENDING (Task 12)` = host-side measurement; `[U]` = unverified.

| stage | stock Sunshine+Moonlight | source | direct | DERP |
|---|---|---|---|---|
| network RTT (real client path) | LAN ping (100 samples): 1.75 / 1.99 / 2.44 ms (min / median / p99), 0% loss. Tailscale ping (20 samples, wired laptop): 2 / 2 / 3 ms, whole-ms resolution. Measured 2026-10-24 | `scripts/m0-netpath.sh` run on the laptop | direct: Tailscale used LAN endpoint `10.0.0.x:41641`, same path as LAN [U: remote/off-LAN direct path not measured] | PENDING (DERP not exercised; needs an off-LAN client) |
| host capture+encode | 3.2 / 5.1 / 7.2 ms (min / median / p99), measured 2026-10-24, static Steam Big Picture screen [U: content-dependent; also needs a moving-scene run] | Sunshine `frame_processing_latency` via `brisk-probe stream` | same (host side, path-independent) | same |
| client decode | PENDING (Toby) | Moonlight Qt stats overlay | PENDING | PENDING |
| client present queue | PENDING (Toby) | Moonlight Qt stats overlay | PENDING | PENDING |
| total host-to-client | PENDING | sum of the above | PENDING | PENDING |

Resolved: moonlight-qt 6.1.0 is on Toby's laptop (docs/moonlight-compare.md); the overlay rows need his run.

## How to fill the Task 12 rows

`brisk-probe stream --host 127.0.0.1 --app <id> --secs 60` ends with a fixed-label block
(`=== brisk stream summary ===` ... `=== end summary ===`). Copy from it:

| row / note | summary line |
|---|---|
| host capture+encode | `host_latency_ms min / median / p99` (Sunshine `frame_processing_latency`, 0.1 ms units, frames with 0 = no measurement are left out) |
| arrival jitter (add as a note under the table) | `arrival_jitter_ms p50 / p99`: abs(inter-frame arrival delta - 16.67 ms) at the client socket |
| loss (add as a note) | `video_loss` and `audio_loss`: missing RTP sequence numbers, before FEC; `fec_recovered`, `audio_recovered`, `audio_lost` after it |

Run on bawn itself these are loopback numbers (host side only); the network part stays the row above.

## How to fill the network row

From the client machine, in this repo: `scripts/m0-netpath.sh bawn` (Tailscale; prints direct/DERP verdict, per-reply path, min/median/p99) and `scripts/m0-netpath.sh <bawn LAN IP>` (plain `ping -c 100 -i 0.2`). Parser check without a live peer: `scripts/test-m0-netpath.sh`.

`tailscale ping` reports whole milliseconds, so sub-ms direct-LAN RTT is coarse there. p99 over 20 samples is effectively the max.

## Measured on bawn itself, not the real client path

These only show the script works. Do not use them as the baseline.

| target | method | verdict | min / median / p99 |
|---|---|---|---|
| 127.0.0.1 | `ping -c 100 -i 0.2` | loopback | 0.035 / 0.060 / 0.122 ms |
| a tailnet peer on the same LAN (from bawn, 20 pongs) | `tailscale ping` | direct, `<peer LAN IP>:41641` (LAN) | 2 / 2.5 / 109 ms |

bawn cannot ping its own tailnet name (`tailscale ping bawn` reports "local Tailscale IP"); the script exits 2 with a message. The peer's p99 is one outlier of 20; not re-run, so unexplained.

## Measured live on bawn (brisk-probe stream, 60 s, 2026-10-24, loopback, static Steam Big Picture)

M1 gate: PASS. Sunshine v2026.914.233613, 1080p60 H.264 [U: codec/res from the probe's default StreamConfig], control+audio encryption 0x5.

| metric | value |
|---|---|
| frames assembled | 3490 in 60 s (about 58.2 fps; gate >= 3420; a static screen may legitimately send fewer than 60) |
| IDR frames / drops / FEC recoveries / parse errors | 3 / 0 / 0 / 0 |
| host capture+encode (frame_processing_latency) | min 3.20, median 5.10, p99 7.20 ms |
| arrival jitter (abs delta vs 16.67 ms) | p50 0.23, p99 4.71 ms (loopback) |
| IDR request to next IDR frame | 13.0 ms and 18.8 ms |
| video bitrate | 7.17 Mbit/s effective, 9.57 on the wire (includes FEC) |
| loss | 0 missing of 50963 video, 0 of 11995 audio (loopback, so FEC recovery is untested live) |
| control | 594 pings sent, 594 acked, 0 bad host messages |

Not measured: any network path (this is bawn talking to itself), client decode/present, a moving scene, input-to-photon.

## bawn -> bawn over real interfaces (2026-10-24, 60 s each, static Steam Big Picture)

Same `brisk-probe stream --secs 60` but addressed to bawn's own LAN IP (bond0) and Tailscale IP (tailscale0) instead of 127.0.0.1. Traffic to a local address still loops through the kernel, so this exercises the address/port handling and socket buffers but NOT a real wire or Tailscale's tunnel; do not read it as a network measurement.

| metric | 127.0.0.1 | LAN IP | Tailscale IP |
|---|---|---|---|
| gate | PASS | PASS | PASS |
| frames assembled (gate >= 3420) | 3490 | 3476 | 3484 |
| drops / FEC recovered / parse errors | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 |
| host latency min / median / p99 (ms) | 3.20 / 5.10 / 7.20 | 3.20 / 5.90 / 7.00 | 3.20 / 5.90 / 7.20 |
| arrival jitter p50 / p99 (ms) | 0.23 / 4.71 | 0.24 / 4.83 | 0.21 / 4.77 |
| IDR response (ms) | 13.0, 18.8 | 9.5, 16.4 | 19.5, 17.3 |
| video effective / wire (Mbit/s) | 7.17 / 9.57 | 6.83 / 9.15 | 7.22 / 9.64 |
| video loss | 0 of 50963 | 0 of 48733 | 0 of 51320 |
| effective SO_RCVBUF video / audio | n/a (before 12b) | 4 MiB / 512 KiB | 4 MiB / 512 KiB |

The arrival-jitter p99 of about 4.8 ms is identical across all three, so it comes from the host's frame pacing and not the network. [U] likely the timer-paced KMS capture noted in the design doc; not yet tested (capture=kms, encoder=vaapi on this box).

## M2: Brisk bridge to the browser (2026-10-24, Task 4)

Client decode and present measured by the page (`web/src/lib/video.ts`): decode = `VideoDecoder`
output callback minus the `decode()` call, present = canvas paint on `requestAnimationFrame` minus
the output callback (the newest frame wins; older ones are closed unpainted).

| stage | headless Chromium on bawn (software decode, no GPU; NOT the real client) | laptop Chrome |
|---|---|---|
| client decode | p50 4.1, p99 11.4 ms (10 min, 34619 frames, 0 errors) | p50 1.0, p99 2.0 ms (147 s, 8796 frames, 0 errors, hardware decode, Chrome 152, 2026-10-25) |
| client present queue | p50 9.9, p99 19.4 ms (headless rAF) | p50 5.1, p99 7.9 ms; 595 of 8796 frames skipped (6.8%), 55.9 presented/s (same run) |
| `bridge_write - capture` (bawn clock) | p50 6.0, p99 8.0 ms vs `frame_processing_latency` p50 5.9, p99 7.9 ms: M2 gate PASS | same host side |

### M2 gate run with video + audio + input (2026-10-24, Task 6)

Measured on bawn with headless Chromium (software decode, no GPU, the bridge on bawn's Tailscale
IP, page on bawn): NOT the laptop and NOT the real network path. Details: docs/m2-notes.md
"M2 gate run: video + audio + input".

| quantity | bawn, headless Chromium (measured) | laptop Chrome over Tailscale |
|---|---|---|
| duration, resolution | 10 min, 1920x1080 at 60 fps, H.264 | PENDING (Toby, docs/m2-notes.md "Toby to run") |
| decode errors | 0 (34610 frames decoded) | PENDING (Toby) |
| `bridge_write - capture` (bridge report) | p50 5.9, p99 8.1 ms | same host side (Toby's run reprints it) |
| `frame_processing_latency` (tap - capture) | p50 5.9, p99 8.0 ms | same host side |
| M2 gate: bridge p50 <= fpl p50 + 2 ms | PASS (5.9 <= 7.9) | PENDING (Toby's run) |
| `bridge_write - tap` | p50 0.1, p99 0.2 ms | same host side |
| client decode | p50 1.1, p99 8.7 ms (software) | PENDING (Toby) |
| client present queue | p50 13.0, p99 20.3 ms (headless rAF) | PENDING (Toby) |
| network path | none (same box) | PENDING (Toby) |
| audio | 120044 decoded, 0 decode errors, page lost 0, underruns 9, bridge lost 3 | PENDING (Toby: listening check) |
| input | 200 messages (120 keys, 80 moves) page = bridge = Sunshine decoded, 0 bad, 0 dropped | PENDING (Toby: real mouse, buttons, wheel, typing) |
| Moonlight overlay, same path (head-to-head) | n/a | 2026-10-25, Mewgenics, 1080p60 H.264, different scenes: Moonlight 60.05 fps in/decoded/rendered, host processing min/avg/max 4.2/6.6/10.8 ms, 0% network and jitter drops, net latency 3 ms, decode 0.45 ms, queue 0.06 ms, render 1.56 ms. Brisk: bridge_write-capture p50 4.7 / p99 6.9 ms, decode p50 1.0, present p50 5.1, 6.8% skipped. Toby: Moonlight felt a bit smoother; both played well. Host leg is a tie; client leg about 4 ms behind (decode +0.5, draw +3.5) and Brisk drops ~4 frames/s. No freezes after the bawn fix (docs/bawn-display-stalls.md). |

