# Hosting runbook: Brisk as a service on bawn

Goal: after this, the laptop opens **https://bawn.tail0.ts.net:8443** (tailnet only) with no ssh
tunnel and, once allowed, no `?s=` link. hostd runs as `brisk-hostd.service` (systemd user unit, starts
at boot thanks to linger), patched Sunshine stays on for good. Architecture and the auth model:
docs/ui-notes.md, section Hosting.

Who does what: **[agent]** = the controller/agent in a shell as toby (no root). **[Toby]** = needs Toby:
a sudoq approval, the laptop, or a decision. Nothing here touches the root address
(https://bawn.tail0.ts.net = orch on 127.0.0.1:8481).

## First-time checklist

1. **[agent]** Merge branch `hosting` into main in `~/Projects/brisk`.
2. **[agent]** Build the release binaries and the page (capped; does not restart anything yet):
   `scripts/brisk-service.sh update`
3. **[Toby]** Stop the manual instance: Ctrl-C in the `brisk-up.sh` terminal and wait for it to exit
   (up to ~15 s while the bridge cancels its Sunshine session). Do **not** run brisk-down.sh: it would
   put stock Sunshine back.
4. **[agent]** Install and start the service: `scripts/brisk-service.sh install`
   (refuses while a brisk-up hostd still runs; idempotent).
5. **[agent]** Patched Sunshine for good: `scripts/brisk-service.sh sunshine-permanent`
   (a no-op when brisk-up already left it patched; otherwise refuses unless Sunshine is FREE and keeps
   restarts 100 s apart).
6. **[Toby]** Map :8443 on the tailnet (toby is not the tailscale operator, so this needs root):
   ```
   sudoq run --why "Brisk page on the tailnet at :8443 (root mapping untouched)" -- tailscale serve --bg --https=8443 http://127.0.0.1:8443
   ```
   `scripts/brisk-service.sh serve-on` prints exactly this line. The mapping persists across reboots.
   Optional instead, once: `sudoq run --why "let toby manage tailscale serve" -- tailscale set --operator=toby`
   (then `serve-on` / `serve-off` run without root; this lets toby run tailscale up/down/serve too).
7. **[agent]** `scripts/brisk-service.sh doctor` (all `ok`; Sunshine FREE may be `info` during a stream).
   Also confirm the root mapping is unchanged: `tailscale serve status` shows `/ -> http://127.0.0.1:8481`
   and `:8443 / -> http://127.0.0.1:8443`.
8. **[Toby]** On the laptop: run `scripts/brisk-service.sh link` on bawn and open the `https://…:8443/?s=…`
   line once in Chrome. The page keeps the secret (localStorage), so from then on the bare
   https://bawn.tail0.ts.net:8443 bookmark works.
9. **[Toby decides]** Optional, no link at all: allow your tailnet login (see the limits in ui-notes.md:
   fine on a one-user box, not on a shared one).
   - Spoof test first. **[agent]** `printf 'nobody@example.invalid\n' > ~/.config/brisk/allowed-users &&
     chmod 600 ~/.config/brisk/allowed-users && systemctl --user restart brisk-hostd.service`.
     **[Toby]** on the laptop:
     `curl -s -o /dev/null -w '%{http_code}\n' -H 'Tailscale-User-Login: nobody@example.invalid' -H 'x-brisk-csrf: 1' https://bawn.tail0.ts.net:8443/api/host`
     must print **403** (tailscale serve replaced the forged header with your real login). If it prints
     200, stop: remove the file, restart, and keep using the secret.
   - **[agent]** Put the real login in the file instead (find it with `tailscale whois <laptop tailscale IP>`,
     the `LoginName`), restart the service, and **[Toby]** check the page loads in a private window
     without the link.
10. **[Toby]** Reboot test (the box is reached over the network; pick a moment you can wait for it):
    `sudoq run --why "Brisk reboot test" -- systemctl reboot`. Then **[agent]** `scripts/brisk-service.sh doctor`,
    and **[Toby]** open the page and start a stream (Desktop) once. A stream started right after boot may
    say "tailscale gave no IPv4 address" until tailscale is up; retry.

## Everyday
- Update after new commits: `scripts/brisk-service.sh update` (refuses to restart over a live stream;
  `FORCE=1` ends it). The page is swapped in atomically (web/dist is a symlink to the newest
  web/dist.<stamp>; the previous build is kept: to go back, `ln -sfn dist.<older> web/dist.link &&
  mv -T web/dist.link web/dist`).
- `update` does not re-sync the unit file. After changing deploy/brisk-hostd.service, run
  `scripts/brisk-service.sh install` again (daemon-reload, and a restart because the file changed).
  `install` keeps the previous unit as `~/.config/systemd/user/brisk-hostd.service.bak` and, after
  (re)starting a changed unit, waits up to 45 s for it to be active with 127.0.0.1:8443 answering. If it
  is not, it puts the .bak back, daemon-reloads, restarts the old unit and exits non-zero ("new unit
  failed; previous unit restored"); then read `scripts/brisk-service.sh logs`. While a stream is live
  (or the API does not answer) `install` writes nothing, does not daemon-reload and exits non-zero:
  run it again when idle (or FORCE=1). A failed first install (no .bak) disables the unit.
- The unit is `Type=notify` with `WatchdogSec=60`: systemd counts hostd as started only after hostd
  answers HTTP on 127.0.0.1:8443 itself (READY, 30 s start timeout), and restarts it when the watchdog
  pings (every 15 s, so three may be missed) stop for 60 s (a wedged runtime or a stuck state lock). `systemctl --user status brisk-hostd`
  shows hostd's status line (`serving 127.0.0.1:8443, stream idle|starting|live`); `doctor` checks
  Type=notify, that status and the watchdog.
- First deploy of the Type=notify unit (from the old Type=simple one) **[agent]**: when no stream is
  live, `scripts/brisk-service.sh update` (new binary first, it still runs under the old unit) then
  `scripts/brisk-service.sh install` (unit changed: restart, wait, restore on failure), then `doctor`.
  Order matters: an old binary under the new unit never sends READY and would be rolled back.
- Any restart (update, install with a changed unit) ends hostd's `wl-copy`: a clipboard sent from the
  laptop is no longer on bawn's clipboard afterwards.
- The journal never holds the secret (hostd prints the page line without it under systemd); use
  `scripts/brisk-service.sh link` for the link.
- Emulator playtime: hostd checks the running emulators every 15 s and writes `playtime.json` in the data
  dir (at most once a minute while a game runs). Restarting hostd loses at most that minute; time while
  hostd is down is not counted. Delete the file to reset all emulator playtime.
- Status / logs: `scripts/brisk-service.sh status`, `scripts/brisk-service.sh logs -f`.
- A new link (another browser): `scripts/brisk-service.sh link`. To rotate the secret: remove
  `~/.config/brisk/secret`, `systemctl --user restart brisk-hostd.service`, open the new link.

## Single binary (optional, not what the service runs)
`cargo build --release -p brisk-hostd --features embed` after `cd web && npm run build` gives a hostd with the
page inside (`BRISK_STATIC=embed`, or automatically when web/dist is missing). The unit and `update` keep
serving web/dist from disk, because the atomic dist swap is the update and rollback path. Details:
docs/ui-notes.md, Hosting, Static files.

## Rollback
- Bad build: `git checkout <last good commit> && scripts/brisk-service.sh update`, later back to main.
- Bad unit file: `install` restores the previous one by itself (see Everyday). By hand:
  `cp ~/.config/systemd/user/brisk-hostd.service.bak ~/.config/systemd/user/brisk-hostd.service &&
  systemctl --user daemon-reload && systemctl --user restart brisk-hostd.service`.
- Back to the tunnel workflow:
  1. **[agent]** `scripts/brisk-service.sh uninstall` (stops and removes the unit; keeps the secret,
     certificate and allowed-users).
  2. **[Toby]** `sudoq run --why "remove the Brisk :8443 tailnet mapping (root mapping untouched)" -- tailscale serve --https=8443 off`
     (printed by `scripts/brisk-service.sh serve-off`).
  3. As before: `scripts/brisk-up.sh` on bawn, `ssh -N -L 8443:127.0.0.1:8443 bawn` on the laptop, open the
     printed `http://localhost:8443/?s=…` link (same secret as the service used). Stock Sunshine again:
     `scripts/brisk-down.sh` after Ctrl-C.

## Machine hub
One URL for several streaming machines (a bare-metal box and a Proxmox gaming VM, say). Use it when you
have more than one machine, or a VM that should start when you want to play and shut itself down when you
stop. With one machine, skip it: the plain service above is simpler. Contract: docs/plans/machine-hub.md.

```
browser --https--> hub (Dockge VM: tailscale serve :443 -> brisk-hostd 127.0.0.1:8443)
                     |-- /api/m/<id>/...  --tailnet--> machine hostd :8443 (one per machine)
                     '-- power on/off     --https---> Proxmox API (VM.PowerMgmt on one VM)
browser <==== video (WebTransport) ==== the machine's own bridge  (never through the hub)
```

The hub is the same `brisk-hostd` as a container with a `machines.json`. Prefs and notes live on the hub.
Each machine keeps running its own hostd as above.

### Build the image
On the build box (capped; Docker is needed only for the last step). `web/dist` may be the symlink: build.rs
resolves it.
```
scripts/brisk-service.sh update            # builds the page into web/dist (or: cd web && npm run build)
. scripts/capped.sh
BRISK_EMBED_DIR=web/dist SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) \
  capped cargo build --release -p brisk-hostd --features embed
cp target/release/brisk-hostd deploy/hub/brisk-hostd
docker build -t brisk-hub:local deploy/hub      # or the box that runs Docker, after copying deploy/hub/
rm deploy/hub/brisk-hostd
```
`BRISK_EMBED_DIR` is relative to the repo root (or absolute). The binary is glibc: if the image base is
older than the build box, hostd may fail with a `GLIBC_x.y not found` error; pass `--build-arg BASE=...`
with a newer base. If Docker runs on another host, `docker save brisk-hub:local | ssh <vm> docker load`.

### Deploy
1. On the VM: `mkdir -p /srv/brisk-hub/config`, then copy `deploy/hub/machines.example.json` there as
   `machines.json` and edit it. The container runs as uid 10001: `chown -R 10001 /srv/brisk-hub/config`
   and `chmod 600` the secret and token files.
2. In Dockge create a stack from `deploy/hub/compose.yaml`; set `BRISK_ALLOWED_HOSTS` to the VM's tailnet
   name, with and without `:8443`.
3. On the VM: `tailscale serve --bg --https=443 http://127.0.0.1:8443` (needs root or the tailscale
   operator). The page is then `https://<hub>.<tailnet>.ts.net`.
4. First visit needs the hub's own secret: `docker compose exec` has no shell tools, so read it from the
   volume: `docker run --rm -v <stack>_data:/data debian:trixie-slim cat /data/secret` and open
   `https://<hub>.<tailnet>.ts.net/?s=<secret>` once. Or set `BRISK_ALLOWED_USERS` (see the limits in
   docs/ui-notes.md, Auth model).

### machines.json
`deploy/hub/machines.example.json` is the template; field rules are in docs/plans/machine-hub.md. In the
container the paths are under `/config`. A bad file stops hostd at start (check the stack logs).

Each machine's `secret_file` is that machine's own hostd secret: on the machine,
`cat ~/.config/brisk/secret` (created by hostd on first start; `scripts/brisk-service.sh link` prints it
inside a link). Copy it to `/srv/brisk-hub/config/<id>.secret` (no newline needed, 0600, owner uid 10001).
Rotating a machine's secret means copying it again; the hub reads the file on each use, no restart.

The machine's hostd must also accept the hub: it is reached by its tailnet name over `tailscale serve`
(`https://<machine>.<tailnet>.ts.net:8443`), and the hub authenticates with the secret, so the machine
needs no `allowed-users` entry for the hub.

### Proxmox token (power control)
Run on the Proxmox node as root. The hub gets only "see and start/stop" on one VM (here vmid 110):
```
pveum role add BriskPower --privs "VM.Audit VM.PowerMgmt"
pveum user add brisk@pve --comment "brisk hub"
pveum user token add brisk@pve hub --privsep 1          # prints the secret once; save it
pveum acl modify /vms/110 --roles BriskPower --users 'brisk@pve'
pveum acl modify /vms/110 --roles BriskPower --tokens 'brisk@pve!hub'
```
Put the printed secret value (only the UUID) in `/srv/brisk-hub/config/pve.token`, and set `token_id` to
`brisk@pve!hub`. With privilege separation a token gets at most what both the token and the user have, so
the ACL is granted to both, on `/vms/110` only (no `propagate` needed for a single VM, no `/` ACL).
Why these two: `VM.PowerMgmt` covers start/shutdown/stop; reading `status/current` needs `VM.Audit`.
Not confirmed on a live node: check with `pveum user token permissions brisk@pve hub --path /vms/110`.
If the status call still answers 403, read the hub log before adding any privilege.

`ca_file`: the PEM that signed the Proxmox web certificate. For the default self-signed setup that is
`/etc/pve/pve-root-ca.pem` on the node (copy it to `/srv/brisk-hub/config/pve-ca.pem`). If the node uses a
public or ACME certificate, the system roots in the image already work: leave `ca_file` out.

### Add a machine
1. Machine runs the hosting setup above (hostd as a service, `tailscale serve` on :8443).
2. Copy its secret to the hub config dir; add an entry to `machines.json` (a unique `id`). For a Proxmox VM
   add a `power` block and create the token/ACL for its vmid (a second VM: repeat the two `acl modify`
   lines with its path).
3. Restart the stack in Dockge (the file is read at start). Reload the page: the machine is in the list.

### Idle shutdown
With `idle_shutdown_min` set, the hub checks every 30 s. A machine that is `ready` with no live stream, no
running game and no Steam update in progress starts a clock; after N minutes the hub sends an ACPI shutdown
(never a hard stop). Any activity, or the hub being unable to see the machine, resets the clock. The clock
starts when the machine becomes ready, so a machine you start and never use also shuts down. Leave the key
out for never. Change it by editing `machines.json` and restarting the stack.

### Gotchas
- Host header: `tailscale serve` on 443 sends `Host: <hub>.<tailnet>.ts.net` (no port). Both forms must be
  in `BRISK_ALLOWED_HOSTS` or every request is 403.
- `BRISK_ALLOWED_USERS` works only because hostd sees `tailscale serve` as a loopback peer, which is why
  the stack uses host networking. In a bridged container the peer is not loopback and logins are never
  trusted; use the secret.
- File drop is not available through the hub (501): drop files on the machine's own page.
- Stream video connects from the browser straight to the machine's bridge address; that must be reachable
  from the browser (tailnet), not just from the hub.

### Update the hub
Rebuild the binary and image as above (same tag `brisk-hub:local`), then in Dockge press Update/Redeploy
(or `docker compose up -d --force-recreate`). Prefs, notes and the hub secret stay in the `data` volume.
Before rebuilding, tag the running image for rollback: `docker tag brisk-hub:local brisk-hub:prev`.

### Roll back
`docker tag brisk-hub:prev brisk-hub:local`, then redeploy the stack. To leave the hub altogether:
`tailscale serve --https=443 off` on the VM and keep using each machine's own page at :8443.

### Troubleshooting
- Every request 403: Host not allowed (see Gotchas), or no secret/login. Check `BRISK_ALLOWED_HOSTS` against
  the exact name in the browser address bar, and the stack logs.
- A machine's calls give 403 through the hub: the secret file is stale or has the wrong owner/mode.
- 502 `machine did not answer`: the hub cannot reach `url` (tailnet down, hostd stopped, wrong name or
  port, or 40 s timeout). From the VM: `curl -sk -o /dev/null -w '%{http_code}\n' https://<machine>.<tailnet>.ts.net:8443/`.
- 503 `machine is off`: Proxmox says the VM is stopped; press Start (or Play, which starts it).
- Proxmox calls fail (502 on power): token or ACL wrong, or the CA does not match. The message names the
  cause but never the token. Check `ca_file` and `pveum user token permissions`.
- VM starts but is never `ready`: `ready` needs the machine's hostd to answer and Sunshine to be up. Check
  the guest booted to the desktop session (autologin), `systemctl --user status brisk-hostd` in the guest,
  and `tailscale status` there. The page gives up waiting after 5 min.
- Machine shuts down while you are away from the keyboard but connected: the hub only counts a live
  stream as activity; raise `idle_shutdown_min` or remove it.

## Adding games

Games live in `~/Games/<system>/` on each machine (`BRISK_GAMES_DIR` changes it). The system comes from the
file header, never the name. Three ways in; all end in a library card:

- Library page, "Add game": resumable upload to the chosen machine. Only free space limits the size; 5 GB stay free.
- Copy files into the folder any way you like (scp, a share), then press "Scan games folder" or
  `POST /api/library/scan`. `BRISK_SCAN_ON_START=1` also scans when hostd starts.
- On the machine: `brisk-hostd add <file> [--move|--copy|--link]` (no server needed).

Only Wii and GameCube have an emulator today. Other systems are stored in their folder but get no card yet.
"Remove" in a game's menu forgets the card and keeps the file; a scan finds it again.
The games VM's `~/roms` keeps working as it is. To fold it in later: `ln -s ~/roms ~/Games/wii`.

Upload from a script (`$HOST` is the machine's hostd address, `$SECRET` its `x-brisk-secret`; placeholders,
fill them in your shell, never commit them). Chunks are at most 32 MiB and must go in order:

```sh
f=game.iso; size=$(stat -c %s "$f")
id=$(curl -s -H "x-brisk-secret: $SECRET" -H 'content-type: application/json' \
  -d "{\"name\":\"$f\",\"size\":$size}" "$HOST/api/uploads" | jq -r .id)
off=0
while [ "$off" -lt "$size" ]; do
  dd if="$f" bs=1M skip=$((off/1048576)) count=32 2>/dev/null | \
    curl -sf -X PUT -H "x-brisk-secret: $SECRET" -H "x-offset: $off" --data-binary @- "$HOST/api/uploads/$id" >/dev/null \
    || { off=$(curl -s -H "x-brisk-secret: $SECRET" "$HOST/api/uploads/$id" | jq -r .offset); continue; }
  off=$(curl -s -H "x-brisk-secret: $SECRET" "$HOST/api/uploads/$id" | jq -r .offset)
done
curl -s -X POST -H "x-brisk-secret: $SECRET" "$HOST/api/uploads/$id/finish"
```

If a PUT fails, the loop asks `GET /api/uploads/$id` for the offset and carries on from there. Details and
error codes: `docs/plans/2026-10-28-stage2-upload.md`. Through the hub this needs the hub change in that file.

## Launch options (MangoHud, GameMode, gamescope, env vars, your own command)

Defaults live in Settings and each game can override single fields (a field left alone inherits). The data is
`launch.json` in the data directory on the machine that runs the game. Allowed: the MangoHud, GameMode and
gamescope wrappers (gamescope takes only `-W -H -w -h -r -f -b --fsr`), environment variables from a fixed
list, extra arguments, and your own command. Your own command stays off until Settings has Allow custom
commands switched on. Nothing goes through a shell; every entry is one argument.

**Emulator games** need nothing: brisk wraps the emulator command itself on Play.

**Steam games** are set once per game. In Steam, open the game's Properties, General, Launch Options and paste
the line the game's panel shows (`GET /api/games/steam:<appid>/launch`, field `steamLine`):

```
<path-to>/brisk-hostd wrap steam:<appid> -- %command%
```

After that, everything is changed in brisk and takes effect on the next start. If hostd runs with a
`BRISK_DATA_DIR`, put the same `BRISK_DATA_DIR=<dir>` in front of the line; Steam's own environment is what the
shim sees. Brisk never edits Steam's config files.

What `wrap` does: reads `launch.json` directly (no network, no secret), works out the options for that game,
puts the wrappers around the command Steam passes and `exec`s it. One line goes to stderr, for example
`brisk-wrap: steam:620: starting through gamemoderun`.

**A bad setting never stops a game.** If `launch.json` is missing, unreadable or invalid (for example custom
commands were switched off after a custom command was saved), the game starts with no options, and the log says
why (`launch options for steam:620 ignored: ...` on the shim's stderr, `warn: launch options ...` in hostd's
log for emulators). If a wrapper program is not installed, the plain command runs instead.

Check it works: run `<path-to>/brisk-hostd wrap steam:<appid> -- /bin/echo hello` in a terminal (it should print
`hello`, or with options set, run `echo` under the wrappers), then start the game from Steam and look at
`ps -ef` for the wrapper above the game. Steam's launch log: `STEAM_LINUX_RUNTIME_LOG=1` in the same option
line writes to `~/.local/share/Steam/logs`.

## Dolphin on the games VM
Brisk can list Wii and GameCube games next to the Steam ones. Play starts `dolphin-emu -b -e <file>` on the host.

1. Install Dolphin: `pacman -S dolphin-emu` (needs root).
2. Install the gamepad profile as the games user: `scripts/setup-dolphin.sh`. It writes
   `~/.config/dolphin-emu/WiimoteNew.ini` and adds `BackgroundInput = True` and `ConfirmStop = False` to `Dolphin.ini`. A file you
   have changed is left alone. `--force` replaces it and keeps the old one as `<name>.before-brisk`.
   `BackgroundInput` was added while debugging the pad and may not be needed. `ConfirmStop = False` is
   needed for Quit: by default Dolphin answers a window close with a "stop the current emulation?" dialog
   that nobody sees on a stream, and it then ignores SIGTERM until it is answered. Existing values are
   never rewritten.
3. List the games in `library.json` in the hostd data dir (`~/.local/share/brisk` by default, see
   `BRISK_DATA_DIR`):
   `{"v": 1, "games": [{"id": "emu:dolphin:mario-kart-wii", "emulator": "dolphin", "name": "Mario Kart Wii", "file": "/path/to/game.iso"}]}`.
   The id is yours to pick, and it stays the game's id for good: notes, tags and quality settings hang on it.
   On the first start after an upgrade, an old `emulators.json` is turned into `library.json` (ids from a
   slug of the name) and is kept as it is. Only `dolphin` works today. A game whose file is missing shows as
   not installed. A broken file is ignored with a warning in the hostd log. The data files that were keyed by
   game number are rewritten to the new ids once, with a copy kept next to each as `<name>.pre-ids`
   (see `docs/plans/2026-10-28-stage1-api.md`).
4. Put the ROMs in any directory the games user can read. A separate disk mounted somewhere is fine.

The profile maps the streamed Sunshine pad to a Wii Classic Controller. It names the pad
`evdev/0/Sunshine (libvirtualhid) X-Box Series Controller`. The `0` is the index among pads with that
name, so it assumes one such pad. With two, the index changes and the mapping misses.
Quit on a Dolphin game closes its window and waits about 5 s, sends SIGTERM and waits about 10 s more, then
reports still running so the UI can offer Force quit. Nothing is killed without that tap.
Dolphin reads its config only at start. After a change, quit Dolphin and start the game again.

## RetroAchievements
Brisk shows your RetroAchievements progress for Wii and GameCube games, read-only. Dolphin earns the
achievements; brisk only reads them back from the public web API when you open the game (and on a Refresh,
and every so often while the game runs). Nothing hooks into Dolphin.

1. On retroachievements.org: make an account, then copy the Web API key from the Keys section of the
   account settings.
2. On the host, as the games user, write `~/.config/brisk/retroachievements`: line 1 your user name, line 2
   the key, then `chmod 600` it. Letters, digits, `_`, `-` and `.` only. No file means the panel stays off.
   (`BRISK_RA_KEY_FILE` moves it, see `docs/env.md`.) Never commit it.
3. Log in to RetroAchievements inside Dolphin on the VM (Settings, Achievements). This needs a Dolphin
   build with achievements support (GameCube since 2407-68, Wii since 2603a per
   `docs/research/emulator-metadata-and-achievements.md`) and Dual Core off. UNVERIFIED: whether the Arch
   `dolphin-emu` package is built with it.
4. Only plain `.iso` (GameCube, Wii) and `.wbfs` (Wii) files can be hashed. An RVZ or WIA file shows
   "unsupported". Hashing reads the whole game once and is cached in `data_dir/ra/hash-cache.json`.

Confirm the hash by hand: `GET /api/games/<id>/ra-hash` answers `{console, hash, container, computedAt}`.
On retroachievements.org open the game page, then its Supported Game Files list, and compare the hash.
The GameCube routine and the Wii routine are checked against synthetic images only. Neither has been
compared with a real disc yet, so a "not found" for a game that has a set may mean a wrong hash: check this
first.

States of `GET /api/games/<id>/achievements` for an emulator game: `off`, `unsupported`, `not-found`,
`rate-limited`, `bad-credentials`, `error` and `ok`. `?refresh=1` skips the 5 minute cache, at most once
every 30 s per game.

## What needs root, and why
| Step | Command | Why root |
|---|---|---|
| :8443 mapping on | `tailscale serve --bg --https=8443 http://127.0.0.1:8443` | tailscaled only takes serve config from root or its operator |
| :8443 mapping off | `tailscale serve --https=8443 off` | same |
| (optional) operator | `tailscale set --operator=toby` | same; then the two above need no root |
| (already done) linger | `loginctl enable-linger toby` | `Linger=yes` already; doctor checks it |
| (already done) caps | `setcap cap_sys_admin,cap_sys_nice+p ~/.local/opt/sunshine-brisk/bin/sunshine` | KMS capture; sunshine-brisk.sh asks via sudoq only if missing |
| reboot test | `systemctl reboot` | |
Everything else (the unit, its enable/start, builds, Sunshine restarts) is toby's own systemd user manager.

## Emulator saves list (Stage 6c)

`GET /api/games/{id}/saves` lists a Dolphin game's save states, Wii save folder and GameCube memory cards. It is
read-only: no download, no delete, no file contents. The folder is `BRISK_DOLPHIN_USER_DIR` (see `docs/env.md`).
Symlinks are skipped. The list is capped (100 items; a Wii save folder is counted, 2000 entries at most).
A game only has saves when its id ends in the 6-character disc id (like `RMCE01`); other ids list nothing.

Checked against Dolphin's source: the user dir rules (`~/.dolphin-emu` if present, else XDG data home),
`StateSaves/<GameID>.sNN`, the memory card names `GC/MemoryCard{A,B}.<region>[.<blocks>].raw`, the GCI folder
`GC/<region>/Card A`, and the Wii save path `Wii/title/<hi>/<lo>/data` with hi `00010000` or `00010004`.

Assumed, not seen in the source: 10 save slots, the region folder names (USA, EUR, JPN or JAP), and that a
disc game's low title id is the first four characters of the disc id in hex. Nothing was run against a real
Dolphin user dir.
