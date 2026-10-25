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
