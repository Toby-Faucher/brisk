# Brisk UI notes

## Run
- On bawn: `scripts/brisk-up.sh` (builds bridge + hostd in the capped scope, switches to the patched
  Sunshine, makes a 13-day certificate if needed, runs `target/release/brisk-hostd` in the foreground).
  It prints `page<TAB>http://localhost:8443/?s=<secret>`; the secret is new every run.
- On the laptop: `ssh -N -L 8443:127.0.0.1:8443 bawn`, then open that URL in Chrome.
- Stop: Ctrl-C (hostd stops its bridge first), then `scripts/brisk-down.sh` (restores stock Sunshine).

## API (hostd, 127.0.0.1:8443 only; Host header checked; non-GET needs the secret)
- GET /api/games, GET /api/host, GET /api/sessions
- GET/POST/DELETE /api/stream (state / start / stop the bridge)
- POST /api/launch, POST /api/install, POST /api/sunshine/restart
- anything else: the static page from `web/dist`

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
