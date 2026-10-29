# Environment variables

Every `BRISK_*` variable read by the code, scripts and tests. "hostd" means `crates/brisk-hostd`. Lists
(`BRISK_ALLOWED_*`) are split on commas and newlines; `#` starts a comment.

Fixed, not configurable: hostd listens on `127.0.0.1:8443` and checks the `Host` header against it
(`config.rs`, `auth.rs`); the service exposes it on the tailnet with `tailscale serve` (`docs/hosting-runbook.md`).

## Running (hostd, bridge, client)

| Variable | Meaning | Default | Read in |
|---|---|---|---|
| `BRISK_DIST` | Directory hostd serves (the built web UI). A command-line argument wins. | `web/dist` | `hostd/src/main.rs` |
| `BRISK_STATIC` | `disk` or `embed`. `embed` needs a build with `--features embed`; anything else warns and uses disk. | `disk` | `hostd/src/site.rs` |
| `BRISK_DATA_DIR` | hostd's data: `prefs.json`, `sessions.jsonl`, `owned.json`, `library.json` (emulator games; the old `emulators.json` is read once and migrated). | `~/.local/share/brisk` | `hostd/src/config.rs` |
| `BRISK_STEAM_DIR` | Steam install, for the `appmanifest_*.acf` library. | `~/.local/share/Steam` | `hostd/src/config.rs` |
| `BRISK_STEAM_KEY_FILE` | File with the Steam Web API key (0600). | `~/.config/brisk/steam-api-key` | `hostd/src/config.rs` |
| `BRISK_STEAM_API_BASE` | Steam Web API base URL (tests point it at a mock). | `https://api.steampowered.com` | `hostd/src/config.rs` |
| `BRISK_STEAM_STORE_BASE` | Steam store base URL (tests point it at a mock). | `https://store.steampowered.com` | `hostd/src/config.rs` |
| `BRISK_SGDB_BASE` | SteamGridDB API base URL (tests point it at a mock). | `https://www.steamgriddb.com/api/v2` | `hostd/src/steamgriddb.rs` |
| `BRISK_SGDB_KEY_FILE` | File with the SteamGridDB API key, one line (0600). Missing = art lookup off (read when art is needed, no restart). | `~/.config/brisk/steamgriddb-api-key` | `hostd/src/steamgriddb.rs` |
| `BRISK_GAMETDB_XML_URL` | URL of a plain `wiitdb.xml` for Wii and GameCube titles (no default; a `.zip` is refused). Optional: adds year, publisher, developer, languages and a better title to emulator games |. | unset | `hostd/src/gametdb.rs` |
| `BRISK_GAMETDB_ART_BASE` | GameTDB art base URL (tests point it at a mock). Art lookup does not use GameTDB images yet. | `https://art.gametdb.com` | `hostd/src/gametdb.rs` |
| `BRISK_BRIDGE_BIN` | `brisk-bridge` binary hostd starts per stream. | `brisk-bridge` (on PATH) | `hostd/src/config.rs` |
| `BRISK_LISTEN` | Bridge address, `IPv4:PORT`; `scripts/brisk-up.sh` rejects `0.0.0.0`. | the Tailscale IPv4 on `:4433`, asked at each stream start | `hostd/src/config.rs`, `scripts/brisk-up.sh` |
| `BRISK_TLS_DIR` | Bridge certificate directory. | `~/.config/brisk/tls` | `hostd/src/config.rs` |
| `BRISK_APP` | Sunshine app id to stream. | `870779643` (Desktop) | `hostd/src/config.rs`, `scripts/brisk-up.sh` |
| `BRISK_ALLOWED_HOSTS` | Extra accepted `Host` headers besides `localhost:8443`, `127.0.0.1:8443` and the tailnet name. | none | `hostd/src/config.rs` |
| `BRISK_ALLOWED_USERS` | Tailscale logins allowed to call `/api` without the secret (added to `~/.config/brisk/allowed-users`). | none | `hostd/src/config.rs`, `main.rs` |
| `BRISK_SECRET` | The API secret (at least 16 printable characters); meant for tests. | read from the secret file | `hostd/src/main.rs` |
| `BRISK_SECRET_FILE` | Where the persistent secret lives (created once, 0600). | `~/.config/brisk/secret` | `hostd/src/main.rs` |
| `BRISK_DOWNLOADS_DIR` | Directory for files dropped onto the Stream page (`files.rs`). | `~/Downloads/brisk` | `hostd/src/config.rs` `downloads_dir` |
| `BRISK_GAMES_DIR` | Games folder: one sub-folder per system (`wii`, `gamecube`, `nes`, ...), plus `.uploads/` for partial uploads. Scanned for games; uploads and `brisk-hostd add` put files here. | `~/Games` | `hostd/src/config.rs` `games_dir`, `ingest.rs` |
| `BRISK_SCAN_ON_START` | `1` scans the games folder once when hostd starts. | unset | `hostd/src/config.rs`, `main.rs` |
| `BRISK_DOLPHIN_DIR` | Dolphin's config directory; the controller editor reads and rewrites `WiimoteNew.ini`, `GCPadNew.ini` and `Dolphin.ini` there (`controls.rs`). | `${XDG_CONFIG_HOME:-~/.config}/dolphin-emu` | `hostd/src/config.rs` `dolphin_dir` |
| `BRISK_RA_KEY_FILE` | RetroAchievements login for the read-only progress panel: line 1 user name, line 2 Web API key, mode 600. No file = the feature is off. | `~/.config/brisk/retroachievements` | `hostd/src/config.rs` `ra_key_file`, `retroachievements.rs` |
| `BRISK_RA_BASE` | RetroAchievements API base (https, or http to loopback for tests). | `https://retroachievements.org/API` | `hostd/src/config.rs` `ra_base` |
| `BRISK_DOLPHIN_USER_DIR` | Dolphin's user directory (`StateSaves/`, `GC/`, `Wii/`). The saves list reads names, sizes and times there, never contents, and never writes. | `~/.dolphin-emu` if it exists, else `${XDG_DATA_HOME:-~/.local/share}/dolphin-emu` | `hostd/src/config.rs` `dolphin_user_dir`, `saves.rs` |
| `BRISK_PLAYTIME_POLL_S` | How often hostd checks which emulator games run, to count playtime into `playtime.json` (seconds, 5 to 15). | `15` | `hostd/src/config.rs`, `playtime.rs` |
| `BRISK_SPAWN_SCOPE` | `1` starts spawned programs (Steam launches) in their own `systemd-run --user --scope`. Set by `deploy/brisk-hostd.service`. | unset | `hostd/src/host.rs` |
| `BRISK_TAP_SOCK` | Unix socket where Sunshine's tap sends encoded frames. | `$XDG_RUNTIME_DIR/brisk/tap.sock` | `brisk-client/src/tap.rs` (used by `brisk-bridge` and `brisk-probe`) |

## Build time (hostd `embed` feature)

| Variable | Meaning | Default | Read in |
|---|---|---|---|
| `BRISK_EMBED_DIR` | Built dist to compile in (absolute, or relative to the workspace root). | `web/dist` | `hostd/build.rs` |
| `BRISK_EMBED_STAMP` | Last-Modified (Unix seconds) of embedded files. | `SOURCE_DATE_EPOCH`, else build time | `hostd/build.rs` |

(`BRISK_EMBED_ABS` is set by `build.rs` for `embed.rs`; do not set it.)

## Scripts

| Variable | Meaning | Default | Read in |
|---|---|---|---|
| `BRISK_ROOT` | Checkout holding `target/` and `web/dist`. | the script's checkout | `scripts/brisk-service.sh` |
| `BRISK_UNIT_DIR` | systemd user unit directory. | `~/.config/systemd/user` | `scripts/brisk-service.sh` |
| `BRISK_TS_NAME` | Skip the `tailscale status` lookup and use this name. | looked up | `scripts/brisk-service.sh` |
| `BRISK_UP_WAIT` | Seconds `update` waits for hostd to come up. | `45` | `scripts/brisk-service.sh` |
| `BRISK_RUSTUP_TIMEOUT` | Seconds for `rustup toolchain install` in `update`. | `900` | `scripts/brisk-service.sh` |
| `BRISK_DOWN_WAIT` | Seconds `brisk-down.sh` waits for the bridge to exit. | `60` | `scripts/brisk-down.sh` |
| `BRISK_EXIT_WAIT` | Seconds `brisk-up.sh`'s exit cleanup waits for the bridge to finish cancelling. | `90` | `scripts/brisk-common.sh` |
| `BRISK_SB` | Path of `sunshine-brisk.sh` (a stub in tests). | `scripts/sunshine-brisk.sh` | `brisk-up.sh`, `brisk-down.sh`, `brisk-service.sh` |

## Tests and web checks

| Variable | Meaning | Default | Read in |
|---|---|---|---|
| `BRISK_HOST`, `BRISK_APP` | Live test only (`cargo test -p brisk-client --test live_stream -- --ignored`): paired Sunshine host and app id (`BRISK_APP` is required). | host `127.0.0.1` | `brisk-client/tests/live_stream.rs` |
| `BRISK_CHROMIUM` | `chrome-headless-shell` for the web checks and browser tests. | The shell for the installed `playwright-core` (`web/scripts/browser.mjs`) | `web/scripts/*.mjs`, `web/vitest.config.ts` |
| `BRISK_HOSTD` | `brisk-hostd` binary for `ui-smoke.mjs`. | `target/release/brisk-hostd` | `web/scripts/ui-smoke.mjs` |
| `BRISK_SMOKE_ALLOW_SKIP` | `1` makes the smoke's "port 8443 taken" skip exit 0 instead of 3. | unset | `web/scripts/ui-smoke.mjs` |
| `BRISK_CLIP_OUT` | Directory where `replay.test.ts` writes its MP4s for `clip-check.mjs`. | unset (not written) | `web/src/modules/stream/replay.test.ts` |
| `BRISK_NOTIFY_CHILD`, `BRISK_ENV_OUT` | Internal: set by hostd's own `notify.rs` tests for their child processes. | | `hostd/src/notify.rs` |

Not environment variables, although they match the pattern: `__BRISK_COMMIT__` is a Vite define (short git hash, from
`web/astro.config.mjs`), and `BRISK_DROPIN` is a shell variable in `scripts/brisk-common.sh` (derived from `SB_DROPIN_DIR`).
