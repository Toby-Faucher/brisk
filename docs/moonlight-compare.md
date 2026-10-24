# Moonlight vs Brisk: protocol match (2026-10-24)

Oracle: Sunshine 2026.914.233613, `min_log_level = verbose`, same host (bawn), same app (870779643), 1920x1080x60, H.264, SDR.
Moonlight: moonlight-qt 6.1.0-7 (pacman) on the laptop, isolated config, software decode, 14:57:30 to 14:58:21, ~44 s stream. Input: mouse, keys a/b/Enter. No controller.
Brisk: `brisk-probe stream --input-demo --take-over`, 30 s, 15:00:36.
Raw Sunshine logs contain session keys and browser cookies, so they are NOT committed (local only, `research/moonlight-compare/`).

| Area | Moonlight 6.1.0 | Brisk | Match |
|---|---|---|---|
| RTSP transport | `rtspenc://` port 48010, `X-GS-ClientVersion: 14` | same | yes |
| RTSP order | OPTIONS, DESCRIBE, SETUP audio, SETUP video, SETUP control/13/0, ANNOUNCE, PLAY | same | yes |
| SETUP Transport header | `unicast;X-GS-ClientPort=50000-50001` | same | yes |
| Launch: `mode`, `sops`, `corever`, `surroundAudioInfo`, `gcmap`, `localAudioPlayMode`, `appid` | 1920x1080x60, 0, 1, 196610, 0, 0 | identical values | yes |
| Launch: extra params | `additionalStates=1`, `uuid`, `remoteControllersBitmap=0`, `gcpersist=0` | not sent; sends `hdrMode=0` | **differs, harmless** (Sunshine accepted; see below) |
| Client `uniqueid` | fixed `0123456789ABCDEF` | its own per-identity id | differs by design |
| Bitrate | client 20000 Kbps, host adjusts to 14988 | same (client 20000, 14988) | yes |
| Control startup | connect data match [v2], then IDX_REQUEST_IDR_FRAME, IDX_START_B | same order | yes |
| Control traffic | IDX_ENCRYPTED, periodic ping, IDR requests | same types | yes |
| A/V pings | video and audio ping, "Received ping [v2]" | same (2 pings logged each) | yes |
| Mouse | relative move, buttons | relative move | yes (buttons not sent by Brisk demo) |
| Keyboard | keyAction 3, 8041 for `a` | keyAction 3 | yes |
| Gamepad | not exercised | attach (mask 0001) and detach (mask 0000) decoded, all neutral | Brisk-only; **buttons/sticks still unverified** |
| Session end | quit key, ENet disconnect ack, app left running | cancel; host free | different by design |
| Sunshine errors | none protocol-related (AV1 VAAPI unsupported is host-side) | none protocol-related | yes |

## Open points
- **Not comparable from Sunshine's log:** Sunshine does not log the ANNOUNCE SDP attributes, only the bitrate. The SDP match relies on the earlier source-level comparison, not this run.
- **Launch extras:** Brisk omits `additionalStates`, `uuid`, `remoteControllersBitmap`, `gcpersist`. Harmless for this host; `remoteControllersBitmap`/`gcpersist` may matter for multi-pad later. Decide when the controller work starts.
- **Still unverified live:** gamepad buttons/sticks, mouse buttons, scroll, rumble, HEVC, real packet loss.
- The leftover Moonlight session (app still running after quit) was cancelled by Brisk's `--take-over`.
