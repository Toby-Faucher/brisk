# Hosting runbook: Brisk as a service on bawn

Goal: after this, the laptop opens **https://bawn.tail674625.ts.net:8443** (tailnet only) with no ssh
tunnel and, once allowed, no `?s=` link. hostd runs as `brisk-hostd.service` (systemd user unit, starts
at boot thanks to linger), patched Sunshine stays on for good. Architecture and the auth model:
docs/ui-notes.md, section Hosting.

Who does what: **[agent]** = the controller/agent in a shell as toby (no root). **[Toby]** = needs Toby:
a sudoq approval, the laptop, or a decision. Nothing here touches the root address
(https://bawn.tail674625.ts.net = orch on 127.0.0.1:8481).

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
   https://bawn.tail674625.ts.net:8443 bookmark works.
9. **[Toby decides]** Optional, no link at all: allow your tailnet login (see the limits in ui-notes.md:
   fine on a one-user box, not on a shared one).
   - Spoof test first. **[agent]** `printf 'nobody@example.invalid\n' > ~/.config/brisk/allowed-users &&
     chmod 600 ~/.config/brisk/allowed-users && systemctl --user restart brisk-hostd.service`.
     **[Toby]** on the laptop:
     `curl -s -o /dev/null -w '%{http_code}\n' -H 'Tailscale-User-Login: nobody@example.invalid' -H 'x-brisk-csrf: 1' https://bawn.tail674625.ts.net:8443/api/host`
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
