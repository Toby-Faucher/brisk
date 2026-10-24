# M0 baseline: stage latency table

Software timestamps only. There is no photodiode or camera, so true input-to-photon is NOT measured. "Total" is host-to-client, not photon. The display/present stage is estimated.

Cells: `PENDING (Toby)` = needs Toby's client machine; `PENDING (Task 12)` = host-side measurement; `[U]` = unverified.

| stage | stock Sunshine+Moonlight | source | direct | DERP |
|---|---|---|---|---|
| network RTT (real client path) | PENDING (Toby) | `scripts/m0-netpath.sh bawn` run on the client | PENDING (Toby) | PENDING (Toby) |
| host capture+encode | PENDING (Task 12) | Sunshine `frame_processing_latency` | PENDING | PENDING |
| client decode | PENDING (Toby) | Moonlight Qt stats overlay | PENDING | PENDING |
| client present queue | PENDING (Toby) | Moonlight Qt stats overlay | PENDING | PENDING |
| total host-to-client | PENDING | sum of the above | PENDING | PENDING |

[U] Whether Moonlight Qt is installed on Toby's client machine (the overlay rows need it).

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
| tailnet peer `framework` (from bawn, 20 pongs) | `tailscale ping` | direct, `10.0.0.151:41641` (LAN) | 2 / 2.5 / 109 ms |

bawn cannot ping its own tailnet name (`tailscale ping bawn` reports "local Tailscale IP"); the script exits 2 with a message. The `framework` p99 is one outlier of 20; not re-run, so unexplained.
