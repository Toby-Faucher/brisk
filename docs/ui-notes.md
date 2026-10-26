# Brisk UI notes

## Run
- On bawn: `scripts/brisk-up.sh` (builds bridge + hostd in the capped scope, switches to the patched
  Sunshine, makes a 13-day certificate if needed, runs `target/release/brisk-hostd` in the foreground).
  It prints `page<TAB>http://localhost:8443/?s=<secret>`; the secret is new every run.
- On the laptop: `ssh -N -L 8443:127.0.0.1:8443 bawn`, then open that URL in Chrome.
- Stop: Ctrl-C (hostd stops its bridge first), then `scripts/brisk-down.sh` (restores stock Sunshine).

## API (hostd, 127.0.0.1:8443 only; Host header checked; every /api request, GET included, needs the `x-brisk-secret` header, because GET /api/stream carries the bridge URL and session token; static pages and assets do not)
- GET /api/games, GET /api/host, GET /api/sessions
- GET/POST/DELETE /api/stream (state / start / stop the bridge)
- POST /api/launch, POST /api/install, POST /api/sunshine/restart
- anything else: the static page from `web/dist`

Guard: the Host header must be `localhost:8443` or `127.0.0.1:8443` (DNS rebinding). There is no Origin check; that is
safe because the secret is required and hostd sends no CORS headers, so other origins cannot read or send it.
Disconnect (ghost bar) stops the host stream (DELETE /api/stream); leaving the Stream page only detaches the viewer and
the bridge keeps running. The game itself keeps running on bawn: GET /api/games marks it `running` (its Steam reaper,
`SteamLaunch AppId=<id>`, is in /proc), the page says Resume, and POST /api/stream with its appid skips
`steam://rungameid`. Either way, once the tap connects hostd polls up to 60 s for a sway window in the reaper's process
tree and runs `swaymsg '[con_id=N] focus, fullscreen enable'`; no window or no sway is only logged.

## Env vars
BRISK_APP (default 870779643, Desktop), BRISK_LISTEN (bridge address; hostd alone defaults to 127.0.0.1:4433, brisk-up.sh sets the tailscale IP:4433),
BRISK_DIST, BRISK_DATA_DIR, BRISK_BRIDGE_BIN, BRISK_STEAM_DIR, BRISK_STEAM_KEY_FILE
(`~/.config/brisk/steam-api-key`, never printed or committed), BRISK_STEAM_API_BASE.

## [U]
`[U]` in code comments marks an unverified assumption about bawn (for example the graphical
environment taken from a running steam process, or the 127.0.0.1 default for BRISK_LISTEN).

## Shaders
- Background smoke only (Home, accent colour, drifts on its own). No pointer interaction and no card shaders; cards only lift and brighten their border on hover. Off on /stream, when the shaders setting is off, and without WebGL2 (CSS gradient instead). Toby's steer, 2026-10-25.

## How to run the smoke test
`cd web && npm run build && node scripts/ui-smoke.mjs` (needs `target/release/brisk-hostd`, or set `BRISK_HOSTD`; port 8443 free).
It starts hostd on fixtures, the fake bridge, a fake `steam` and a mock Steam API (sentinel key, 139 games) in a temp dir,
drives headless Chrome through Home, Library, Machines, Settings, Ctrl+K, accent, the 403 guard, a launch to `live` and
the ghost bar, then prints PASS/FAIL (exit 0 only if all pass) and screenshot paths. It cannot prove video: WebTransport
to the fake bridge fails, so Stream is only checked for degrading to "Lost the connection" without page errors.

## HEVC
- Select: ghost bar, Quality, Advanced..., Codec: HEVC, Apply. It reconnects like any custom quality
  (`POST /api/stream` with `codec: "hevc"`, hostd starts the bridge with `--codec hevc`, the SDP asks
  Sunshine for `bitStreamFormat` 1). H.264 stays the default for fresh launches; a preset change (chips, Ctrl+K) keeps the live codec.
- HEVC is offered only when `VideoDecoder.isConfigSupported({codec: 'hvc1.1.6.L120.B0'})` says yes, else
  the option is disabled ("not supported on this browser").
- The page does not trust the request: it reads the codec from the first key frame (H.264 SPS -> `avc1.*`,
  HEVC VPS/SPS/PPS -> `hvc1.*` from the SPS profile_tier_level, ISO 14496-15 E.3). The stats card shows it.
- The bridge refuses a tap whose codec field differs from the one it asked for (error in the bridge log), and
  with `--codec hevc` stops at once if Sunshine's serverinfo `ServerCodecModeSupport` lacks the HEVC bit 0x100
  ("Sunshine reports no HEVC support on this GPU").
- [U] Chrome decodes Annex B HEVC with no `description` (the WebCodecs HEVC registration says Annex B when it
  is absent; Sunshine sends VPS/SPS/PPS in-band on every IDR). [U] Chrome's key-frame check accepts the IDR
  that VAAPI hevc makes on an IDR request (it should be NAL type 19/20; the flag is Sunshine's key-frame flag,
  nothing in Brisk parses slice types). [U] `hvc1` vs `hev1` makes no difference to Chrome for Annex B.
  [U] the level Sunshine's VAAPI SPS carries (1080p60 needs 4.1, `L123`) is within the laptop decoder's limit.
- Toby to measure: the HEVC vs H.264 rows in docs/m0-baseline.md (same bitrate), and whether the stats card
  says `hvc1.*` with hw decode and 0 decode errors over a few quality switches.
