# Brisk UI notes

## Settings tabs
- Settings is split in Bits UI Tabs (arrow keys, Home/End, roving focus): General (accent, sounds, shaders), Streaming
  (default quality, replay buffer, clipboard sync), Controller (the pad test), About (commit, host name and Sunshine state
  from the `host` store, shortcuts list, doc paths as plain text; hostd does not report a version). Pure helpers and
  tests: web/src/modules/settings/tabs.ts.
- The tab lives in the URL hash (`#general|streaming|controller|about`), written with `history.replaceState` (no history
  entries) and mirrored in localStorage `brisk.settingsTab`. No hash: last used tab, then General. An unknown hash: General.
  `hashchange` and `popstate` listeners are removed on destroy (the Shell remounts islands).
- The Controller panel is only rendered while its tab is active, so its polling loop is unmounted (not just paused) on
  leaving. The commit shown comes from `git rev-parse --short HEAD` at build time (vite `define` `__BRISK_COMMIT__`).
- The server render shows General and the real tab is applied on mount, so a deep link flashes General for a frame.
- Not done: the shell sidebar has no phone layout (every page is 450 px wide at 390 px); the tab strip itself scrolls.

## Motion
- Tokens in tokens.css: `--dur-page` 200 ms, `--dur-tab` 180 ms, `--dur-enter` 420 ms, `--rise-enter` 14 px, `--stagger` 55 ms, `--rise` 8 px,
  `--rise-scroll` 14 px, easing `--ease`. All motion is opacity + the independent `translate` property (never `transform`,
  which hover owns), no `will-change`, and sits in base.css under `prefers-reduced-motion: no-preference` (reduced motion:
  no animation, `scroll-behavior: auto`).
- Page: `<main class="page-in">` (Shell only, so the sidebar, host pill and frame never replay) is an opacity-only 200 ms
  fade. Not on /stream (anti-goal, docs/ui-design.md section 2; a `translate` on `<main>` would also trap the stream's
  `position: fixed`, and `reveal.ts` does nothing there).
- Load entrance (every reload and client navigation): items above the fold rise 14 px and fade in over 420 ms, delayed
  `--i` x 55 ms (`--i` 0..8, so the 9th item and later share 440 ms). Tagging helper `shell/reveal.ts`: `enter()` finds the
  untagged `.reveal` / `.enter` elements in `<main>` that intersect the viewport, sets `--i` (DOM order) and `data-enter`;
  CSS `.reveal[data-enter], .enter[data-enter]` plays `enter-rise` on the document timeline (its shorthand resets
  `animation-timeline` to auto, so it beats the scroll reveal). It is idempotent, a tagged element never replays. Pure part
  `planEnter` is unit-tested. Callers: Shell on mount (waits for the Astro islands to hydrate, max 1.5 s) and each place
  where content arrives late: Library games landing (hero, shelf headers, first <= 8 cards per shelf), History list,
  Machines host card, Home lazy widgets. Layout's head script sets `html[data-enter-pending]` (and again on
  `astro:after-swap`; 3 s failsafe) which hides `.reveal/.enter` in `main.page-in` until `enter()` has tagged them, so the
  SSR markup does not flash before it animates.
- Classes: `.reveal` = above the fold: load entrance, below: scroll reveal (`animation-timeline: view()`, `entry 0% entry
  35%`, inside `@supports`, unsupported means plain visible) on `Card`, page headings (`.top`), Library filters, hero and shelf
  headers, Add-a-module. `.enter` = entrance only, never scroll-driven: Library cards (first 8 of a shelf at its start),
  and only tagged by `enter()` at route load / games arrival, so paging a shelf or scrolling mounts cards without any
  animation. `.fade-in` (opacity, 420 ms) is added by class when data lands: Home host card and resume card content.
- Library search/filter: the results (`.body`) cross-fade 0.3 -> 1 over 140 ms via WAAPI, at most once per 150 ms.
- Also animates: Settings tab panels and the History list/detail swap (`.swap-in` / `.live`, 180 ms, only after a user action).
- Does not animate: /stream (except the ghost bar's CSS slide), virtualised items when they mount while scrolling, tall
  blocks such as an open shelf grid, menus, the host pill.
- Springs (CSS only, zero runtime): `web/scripts/gen-spring-tokens.mjs` solves three springs with anime.js offline and writes
  `--spring-soft|snappy|settle` (CSS `linear()`) and `--spring-*-dur` (settling time, use it as the duration) into the
  generated block at the end of tokens.css; `npm run springs:check` and `src/styles/tokens.test.ts` fail if it is stale.
  soft (400/40, no overshoot, 90 % at 195 ms, capped at 450 ms): Card and game card hover lift. snappy (380/34, 0.4 %, 90 % at
  169 ms, 700 ms): link-card press, glides. settle (150/19, 2 %, 90 % at 237 ms, 900 ms): ghost bar slide-in (it leaves
  faster, 300 ms `--ease`), confirm dialog rise (`.rs`). These CSS springs are allowed on /stream; anime.js is not.
- anime.js 4.5.0 (exact pin, MIT; a runtime dependency because `ui/motion.ts` loads it) is used only where CSS cannot do it,
  all through `web/src/ui/motion.ts`, lazily by sub-path (`animejs/animation`, `/easings`, `/svg`, `/scope`), each on its first
  real use: numbers that change after load (`count` attachment / `tweenNumber`: Home host temps and ring `--p`, Machines GPU/CPU,
  Library widget installed count; first paint never animates; uptime is too coarse to roll; History has no polled numbers)
  and the History chart line draw-in (`drawIn`, `createDrawable`, 700 ms `--dur-draw`, once per open, spread onto LayerChart's
  line path through an attachment key; LayerChart's own `draw` prop redraws on every resize). The fill under it fades in
  with native WAAPI (`fadeIn`). `scope()` / `motionScope` wrap `createScope` for future multi-part effects (reverted on destroy).
- Glides (no library): `ui/motion-base.ts` `glide()` is native `Element.animate` on `translate` + `scale` of the selected
  item's `::before` highlight (server-rendered, `transform-origin: 0 0`, `z-index: -1` inside the list's stacking context so
  it passes under the neighbours). Sidebar: the previous active item's box lives in module state, so after a client
  navigation the highlight glides from it across the Shell remount (not on first load, not onto /stream). Library filter
  chips and Settings default-quality chips: `glideSwap` (measure, change, `flushSync`, FLIP). The Settings tab underline keeps
  its CSS transition.
- Not done: Library grid FLIP (`createLayout`): it works with the keyed list, but walks the whole subtree under its root and
  cost 50-61 ms per change on the 40-card "Show all" grid (over the 50 ms long-task budget while typing). Patch and numbers:
  `.superpowers/sdd/anime-pass-flip-experiment.patch`, `anime-check.mjs flip`.
- Motion rules: (1) nothing from anime.js is imported, fetched or run on /stream or by anything it loads (Shell, Sidebar,
  ghost bar, stats card, toasts, dialogs): the Shell imports only `motion-base.ts`, which has no library; `motion.ts` throws in
  dev and never resolves in prod there. (2) Pin animejs exactly; never 5.x beta. (3) anime.js ignores reduced motion: every
  helper checks it live and writes the end state; a switch mid-flight completes running tweens. (4) The Shell remounts on
  every navigation: whatever a component starts is cancelled or reverted in its cleanup (a leaked loop ticks forever).
  (5) Lazy only: dynamic `import()` on first need. (6) Compositor properties only for movement: `translate`, `scale`,
  `opacity`; never anime's `x`/`y` in waapi (they go through a custom property and fail compositing). (7) No anime `onScroll`
  (it holds a 60 Hz rAF loop alive) and no `loop: true`. (8) anime and Svelte never write the same thing on one element (the
  ring's `--p` is no longer a `style:` binding; `count` writes Svelte's own text node and ends on Svelte's value).
  (9) Nothing left behind: no `will-change`, no inline `transition`, no inline `--accent` on `<html>`. (10) Every lazy motion
  path falls back to its end state when its chunk fails to load (a number shows the new value, a chart line shows in full).
- Smooth scroll: `html { scroll-behavior: smooth }` (off while the Astro router is navigating, so scroll restore does not
  glide), `scroll-padding-top: 64px` for the sticky Settings strip, `overscroll-behavior-x: contain` on the shelf and the tab
  strip. No JS smooth-scroll library and no wheel hijacking (the shelf's vertical-wheel-to-sideways stays as it was).
- Do not write the scroll reveal as an `animation:` shorthand: the minifier would fold `view()` into it and Chrome drops that.
- Check: `node web/scripts/motion-check-2.mjs` (load entrance, library data arrival with a delayed /api/games, no re-fire, reduced
  motion, stream, seeked screenshots to `$OUT`) and `node web/scripts/motion-check.mjs` (page fade, tabs, scroll reveal), both
  against `astro preview` (ports 4422 / 4421, mocked /api). Home shows a pre-existing layout shift of about 0.02-0.06 when the
  module widgets mount, with or without motion. anime.js pass: `node web/scripts/anime-check.mjs [graph stream count revert
  reduce glide chart tour spring]` (port 4423; /stream import graph + network, tweens, revert on navigation via rAF counts,
  reduced motion, glide seeked screenshots + Chrome trace, chart draw-in, long tasks). Unit tests: `web/src/ui/motion.test.ts`.

## Run
- Normal: hostd runs as a systemd user service and the laptop opens https://bawn.tail674625.ts.net:8443
  (see Hosting below and docs/hosting-runbook.md).
- By hand (dev path, only while the service is not active; brisk-up and brisk-down refuse otherwise):
  `scripts/brisk-up.sh` (builds bridge + hostd in the capped scope, switches to the patched Sunshine, runs
  `target/release/brisk-hostd` in the foreground). It prints `page<TAB>http://localhost:8443/?s=<secret>`;
  the secret lives in `~/.config/brisk/secret`, so the link stays the same across runs.
- On the laptop: `ssh -N -L 8443:127.0.0.1:8443 bawn`, then open that URL in Chrome.
- Stop: Ctrl-C (hostd stops its bridge first), then `scripts/brisk-down.sh` (restores stock Sunshine).

## Hosting
```
 laptop Chrome ──https://bawn.tail674625.ts.net:8443──▶ tailscaled on bawn (tailscale serve, TLS,
   │                                                     adds Tailscale-User-Login/-Name)
   │                                                         │ http://127.0.0.1:8443 (loopback)
   │                                                         ▼
   │                                    brisk-hostd.service (systemd --user, toby, linger)
   │                                     page + /api, Host allowlist, secret or tailnet login
   │                                         │ spawns at each stream start (own cgroup)
   │                                         ▼
   └──WebTransport https://<tailscale IP>:4433──▶ brisk-bridge ──▶ patched Sunshine (drop-in,
                (cert hash pinned, 13-day cert)                     permanent; plain RTP w/o bridge)
```
- The root address https://bawn.tail674625.ts.net is Toby's `orch` (127.0.0.1:8481). Brisk only ever
  touches the :8443 mapping (`tailscale serve --bg --https=8443 http://127.0.0.1:8443` and
  `tailscale serve --https=8443 off`). hostd itself still binds 127.0.0.1:8443 only; tailscale serve is
  the only way in from the tailnet, so nothing outside the tailnet can reach it.
- Unit: deploy/brisk-hostd.service, installed by `scripts/brisk-service.sh install` into
  ~/.config/systemd/user (WantedBy=default.target; with linger it starts at boot without a login).
  Restart=on-failure every 3 s (gives up after 10 starts in 2 min), TimeoutStopSec=20 (the bridge stop
  can take 12 s), KillMode=mixed (SIGTERM to hostd only, so it stops the bridge first; SIGKILL to anything
  left after 20 s), MemoryMax=1G, CPUWeight=50, Nice=5, Wants/After the Sunshine
  unit. Hardening is deliberately thin, each omission is commented in the unit: NoNewPrivileges would be
  inherited by a Steam that `steam steam://` cold-starts; ProtectSystem/ProtectHome/PrivateTmp/
  PrivateUsers/ProtectProc need a user+mount namespace in a user unit, which Steam, games and `kill` on
  processes outside it would inherit or cross; address-family limits would have to allow IPv4 and
  AF_UNIX (sway, Wayland, D-Bus, tailscaled) anyway. `BRISK_SPAWN_SCOPE=1` (set by the unit) puts each
  `steam steam://…` in its own `systemd-run --user --scope`, so a cold-started Steam and its games are
  not under the 1G cap or killed by a restart.
- Self-managed at each stream start (no shell script needed): the bridge address is `tailscale ip -4`
  :4433 unless `BRISK_LISTEN` is set (0.0.0.0 refused; tailscale down = a clear 500 "tailscale gave no
  IPv4 address"), and `~/.config/brisk/tls/bridge.pem` + `bridge.key.pem` (ECDSA P-256, 13 days, 0600)
  are made with openssl when missing, older than 10 days, dated in the future, or mismatched (key newer
  than cert; a new pair gets one mtime). Both happen before an old bridge is stopped,
  so a failure leaves a running stream alone. A new pair is written to `.bridge*.new` (0600 before
  openssl writes) and renamed over the old one; a failed openssl leaves the old pair.
- Scripts: `scripts/brisk-service.sh install | update | status | logs [-f] | link | uninstall |
  serve-on | serve-off | sunshine-permanent | doctor` (never sudo; a root step is printed as the
  `sudoq run ...` line). `update` = capped release build of hostd + bridge, `npm ci` only when the lock
  changed, and only when web sources are newer than web/dist a build into a new `web/dist.<stamp>`
  (`npm run build -- --outDir`), swapped in by renaming a `web/dist` symlink over the old one (`ln -sfn`
  + `mv -T`, atomic; a first plain dist is moved to `web/dist.plain-<time>-<pid>`). A failed or empty
  build is deleted and web/dist stays. Pruning never touches the build web/dist points at and keeps the
  most recent other build with an index.html by mtime (not by name, so a clock step cannot pick the
  live one). The swap happens only together with the restart, and the restart only when hostd's API
  answers `idle`: a live stream or no answer keeps the old page and binary running, drops the new
  build and fails (FORCE=1 overrides). `install`'s restart of a changed unit follows the same rule. hostd
  canonicalizes `dist` per request and only serves files that canonicalize inside it (a symlink out of
  the build is a 404); a missing path with an extension is a 404, never index.html. `update` does not
  re-sync the unit file: after a change to deploy/brisk-hostd.service run `install` again (it restarts
  the unit when the file changed). `install` = copy the unit, daemon-reload, enable --now, and a restart
  if the unit changed while it ran (not over a live stream). `sunshine-permanent` = sunshine-brisk.sh install + enable (100 s
  spaced restarts, refuses unless Sunshine is FREE). `doctor` checks linger, the unit, the listener
  (127.0.0.1:8443 required; tailscaled's own `…%tailscale0:8443` sockets in TUN mode are fine; 0.0.0.0, [::],
  * or any other address FAIL), the serve mapping (`<name>:8443` -> `http://127.0.0.1:8443` exactly), 403
  without and 200 with the secret over https, the certificate age and the Sunshine drop-in/binary/state.
  Tests: scripts/test-brisk-service.sh (all stubs).
- Under systemd (`INVOCATION_ID` set) hostd's startup line is `page<TAB>http://localhost:8443/ (link:
  scripts/brisk-service.sh link)`, without the secret, so the journal never holds it. The bridge's
  `page` line (its wt URL carries the session token, which grants stream and input) and any other bridge
  line with `wt=` or the token are re-printed as `bridge<TAB>page <redacted>` (hostd still parses them for
  GET/POST /api/stream). `logs` also masks any `?s=`/`&s=` and `?wt=`/`&wt=` value. brisk-up and manual runs still print the full link. brisk-up/brisk-down refuse
  while the unit is active or enabled.
- A restart of the service ends hostd's `wl-copy`, so a clipboard set from the laptop is gone from bawn's
  clipboard after an update or restart.
- Rollback: `git checkout <good commit> && scripts/brisk-service.sh update`; or back to the tunnel
  workflow: `scripts/brisk-service.sh uninstall`, `serve-off`, then brisk-up/brisk-down as before.
- Root steps (Toby, via sudoq): only `tailscale serve --bg --https=8443 http://127.0.0.1:8443` (and its
  `off`), because toby is not the tailscale operator. Alternative once: `tailscale set --operator=toby`,
  after which brisk-service.sh runs serve itself. Linger is already on; setcap for patched Sunshine is
  already done (sunshine-brisk.sh install asks via sudoq if it ever is not).
- Troubleshooting: `scripts/brisk-service.sh doctor` first. Page shows "bawn doesn't know this browser":
  open the link from `scripts/brisk-service.sh link` once (the page keeps the secret in localStorage) or
  allow the login. Unit restarting: `logs` (a manual brisk-up holding 8443 = "bind ... address in use").
  Streams fail with "tailscale gave no IPv4": tailscale is down on bawn. 403 on every request through
  https but fine on localhost: the Host header is not allowed (hostd learns the ts.net name from
  `tailscale status --json` at startup and again, at most every 30 s, when an unknown *.ts.net name
  arrives; or set `BRISK_ALLOWED_HOSTS`).

### Auth model and its limits
- Host header (DNS rebinding): exactly `localhost:8443`, `127.0.0.1:8443`, `BRISK_ALLOWED_HOSTS`
  (comma list) and this machine's `Self.DNSName` from `tailscale status --json`, as `<name>:8443` only
  (no port-less, trailing-dot or IPv6 forms). Anything else is 403 before any route runs, the secret does not help.
- Every /api request needs one of: the `x-brisk-secret` header (128 random bits, `~/.config/brisk/secret`,
  0600, made once; `BRISK_SECRET` overrides for tests; `BRISK_SECRET_FILE` moves it), or a tailnet login:
  the peer is loopback, `Tailscale-User-Login` is in `BRISK_ALLOWED_USERS` (comma list) or
  `~/.config/brisk/allowed-users` (one per line, `#` comments; ignored, with a warning, unless it is a
  regular file (not a symlink) owned by hostd's uid and not group/world-writable; read at startup, so restart after editing), and the request carries `x-brisk-csrf` (the page
  always sends it). The allowlist is empty by default: feature off, secret required as before.
- Why `x-brisk-csrf`: tailscale serve attaches the laptop's identity to every request from the laptop,
  including one a hostile web page makes cross-site (a `text/plain` POST needs no preflight). A custom
  header forces a CORS preflight, which hostd never answers with CORS headers, so only same-origin pages
  can send it.
- The limit, honestly: tailscaled is not the only thing that can connect from loopback. Any local process
  on bawn can open 127.0.0.1:8443 and send `Tailscale-User-Login: <allowed login>` itself. That is the
  same trust level as being able to read `~/.config/brisk/secret` as toby (any process running as toby
  can do both), but it is weaker than the secret against other local users: on a multi-user box any
  user could spoof the header. bawn has one user, so it is acceptable there; do not enable the
  allowlist on a shared machine. [U] that tailscale serve strips client-sent `Tailscale-User-*` headers
  before adding its own and keeps the Host header (not checked on bawn yet; the doctor's 403 check and
  the runbook's spoof test cover it).
- Page: works with no secret (the tailnet login path); a captured `?s=` is kept in sessionStorage and
  localStorage (try/catch), so a bookmark of the bare URL keeps working. Any 403 sets the `denied` store
  (web/src/state/api.ts) and the Shell replaces the page with "Open this page from the link that
  includes ?s=… or ask hostd to allow your login"; the next successful call clears it.

## API (hostd, 127.0.0.1:8443 only; Host header checked; every /api request, GET included, needs the `x-brisk-secret` header or an allowed tailnet login (see Hosting), because GET /api/stream carries the bridge URL and session token; static pages and assets do not)
- GET /api/games, GET /api/host, GET /api/sessions (last 100, no series), GET /api/sessions/<started>,
  PUT /api/sessions/<started>/series (see Session history)
- GET/POST/DELETE /api/stream (state / start / stop the bridge)
- GET /api/prefs, PUT/DELETE /api/prefs/<appid> (see Per-game quality)
- POST /api/launch, POST /api/install, POST /api/sunshine/restart
- POST /api/quit `{appid, force?}` (see Quit game)
- GET/POST /api/clipboard, PUT /api/files/<name> (see Clipboard and file drop)
- anything else: the static page from `web/dist`

Guard: the Host header must be on the allowlist (`localhost:8443`, `127.0.0.1:8443`, the tailnet name; DNS rebinding).
There is no Origin check; that is safe because the secret (or the `x-brisk-csrf` header, for a tailnet login) is
required and hostd sends no CORS headers, so other origins cannot read or send them.
Disconnect (ghost bar) stops the host stream (DELETE /api/stream); leaving the Stream page only detaches the viewer and
the bridge keeps running. The game itself keeps running on bawn: GET /api/games marks it `running` (its Steam reaper,
`SteamLaunch AppId=<id>`, is in /proc), the page says Resume, and POST /api/stream with its appid skips
`steam://rungameid`. Either way, once the tap connects hostd polls up to 60 s for a sway window in the reaper's process
tree and runs `swaymsg '[con_id=N] focus, fullscreen enable'`; no window or no sway is only logged.

## Quit game
- UI: a Quit button on the Library hero and on cards of running games, per game under "Running apps" on Machines,
  and "Quit <name>" in Ctrl+K (running games only). All open one confirm (shell/QuitDialog.svelte, Bits Dialog:
  "Quit <name>? Unsaved progress may be lost.", Cancel / Quit; focus returns to the button). While hostd works
  (up to ~25 s) the buttons say "Quitting…" and are aria-disabled. Then the games store refreshes and a toast says
  "Closed <name>" or "Still running: <name> (try Force quit)" with a Force quit button (30 s), which asks again and
  retries with `force: true`; while a game is in that state Ctrl+K also offers "Force quit <name>". A 409
  "already quitting" (another tab or button got there first) is an info toast "Already closing <name>…" plus a
  refresh, not an error. User copy never says "kill". Logic and tests: web/src/state/quit.ts.
- hostd (running.rs `Quitter`): 400 unless the appid is a known game, 409 if it is not running or a quit of it
  is already under way (one per game; other games are not held up). Steps: `swaymsg '[con_id=N] kill'` on every
  sway window whose pid is a descendant of the game's reaper (sway's polite close, like the X button); poll 10 s
  for the reaper to exit; else `kill -TERM` to the reaper's descendants (not the reaper, never steam, the
  webhelper/Big Picture or another game's tree; the list is rescanned just before each signal); poll 10 s.
  `force` skips the window close: `kill -TERM`, 3 s, `kill -KILL` to the same set, 3 s. A reaper with nothing left
  under it gets the signals itself (never its parent, Steam) with 3 s waits. Answer
  `{state: "closed" | "still-running"}`; past 25 s it answers still-running and the quit carries on. The quit runs
  in its own task, so a client hanging up does not stop it halfway. A failing `kill` (a pid already gone, EPERM,
  no binary) is a stderr warning naming the appid and a pid count. Nothing touches the bridge or Sunshine, so the Desktop /
  Big Picture stream keeps going.
- sway's close on an XWayland window sends WM_DELETE_WINDOW; for an X11 window without that protocol wlroots
  disconnects the X client instead (`xcb_kill_client`), which ends that client only, still one of the game's.
- hostd must run as toby, never root: as euid 0 it logs a warning at startup and POST /api/quit answers 503.
- Tests use a fake Runner (it gets the `swaymsg` and `kill` calls) over a fake /proc shaped on bawn's real tree;
  nothing real is signalled (`cargo test -p brisk-hostd --test quit`; tokio time is paused, so no real waits).
- [U] Not tried on bawn: whether Proton/XWayland games exit on sway's close (Pepper Grinder is native XWayland; a
  Proton game may show a "really quit?" prompt and fall through to SIGTERM), that Steam's overlay
  (gameoverlayui, a child of steam) and the Running badge clear by themselves once the reaper exits, that sway
  focuses Big Picture again after the last game window closes, and the confirm's focus trap/Esc in Chrome.

## Per-game quality
- Each game remembers its own quality (preset, codec, and width/height/fps/bitrate for `custom`), with no extra
  clicks. Changing quality during a stream of a game (ghost bar chips, Advanced Apply, Ctrl+K presets) saves it for
  that appid after 1 s of quiet (last change wins; leaving the page flushes a pending one). The Desktop / Big
  Picture stream (no appid) and a failed switch save nothing. The ghost bar's Quality menu then shows "Saved for
  <game>" and "Forget for this game" (only while saved).
- Starting a game (Library click, Resume card) sends its saved quality, else Settings' default preset on h264; the
  Stream page's own Start (the Desktop) uses the default. One pure function decides: `prefsFor` in
  web/src/state/prefs.ts, used through `startGame`. Library cards and the hero show a chip of the saved quality
  ("Crisp", "HEVC · 40 Mbps", "Custom · 8 Mbps") only when saved. The store refreshes on focus (at most once a minute).
- hostd (prefs.rs): `<data_dir>/prefs.json`, `{"<appid>": {preset, codec, width?, height?, fps?, bitrateKbps?, updated}}`,
  written to a 0600 temp file in the same directory, fsynced and renamed; a missing or corrupt file (or a bad entry) loads
  as empty. `PUT /api/prefs/<appid>` validates like StreamReq (preset fast|balanced|crisp|custom, codec h264|hevc, 320..3840 px,
  24..144 fps, 1000..150000 kbps; custom numbers are kept only for `custom`), 400 otherwise and for the Desktop app id;
  `DELETE` is idempotent (204). Capped at 500 entries, oldest `updated` dropped. All need the secret.
- Not done: Ctrl+K "Launch <game>" only starts the game on the host (`/api/launch`, no stream), so it has no quality to
  apply; the saved quality is used when the game is then opened from Library or Resume. A saved HEVC on a browser
  without HEVC decode starts in H.264 for that start (toast "This browser cannot decode HEVC, starting in H.264");
  the saved entry stays.
- Saving: Forget waits for a save still in flight, then DELETEs, and that save's answer is dropped, so Forget
  cannot be undone by a late PUT. A pending change is flushed on `pagehide` too (the PUT uses fetch `keepalive`).
- [U] Not tried on hardware: saved quality across a real Library -> stream -> quality change -> Disconnect -> Library round trip.

## Session history
- `/history` (sidebar "History", chart icon, `--c-hist` orange): one card per stream (last 100), with game, date and
  time, length, quality ("Crisp · HEVC · 40 Mbps"; older records only have the preset), a delay sparkline and the
  bridge's PASS/FAIL gate (the sparkline comes with the list: `spark`, 24 mean-binned delays per row, so the list
  makes one request). A card opens `/history?session=<started>` in place (pushState; Back returns): delay, frame
  rate and bitrate over the session as LayerChart area charts with a hover tooltip (m:ss, one decimal; each chart
  has a visually hidden min/average/max and peak-time summary, the plot is aria-hidden), and the bridge report as a table
  (p50 / p99 latencies, docs/m0-baseline.md). Home's "Recent sessions" title and rows link here. Empty, unreachable
  and old-record (no series, no report) states say so.
- Recording: the Stream page passes each 500 ms stats sample with frames (delay estimate, fps > 0, Mbit/s; the same
  numbers as the stats card) to `SeriesUploader` (web/src/modules/stream/series.ts), which averages them to one point
  per 5 s and PUTs them every 30 s and on Disconnect, leaving /stream and `pagehide` (fetch keepalive). Samples stay
  pending until a PUT of them succeeds (at most 4000 samples, ~33 min, oldest dropped), so a failed PUT is simply
  re-sent; the current, partial bucket is sent again until complete (hostd keeps the later point for a t). A 404 or
  413 ends uploads for that session quietly. Uploads are never awaited.
- Time base: t is hostd's seconds since `started`, not the laptop's wall clock. GET/POST /api/stream carry
  `elapsedS` (hostd's monotonic Instant since the session's record was made; null when idle); the page adds its own
  `performance.now()` since that answer, and the uploader then counts on from its first sample. A reload of /stream
  mid-session continues from hostd's elapsed and skips the rest of the 5 s bucket it lands in, so earlier points are
  never rewritten. Only while the page shows the stream: a bridge running with no page records no series.
- hostd: the bridge's end-of-session report (stdout, see stream.rs) is parsed line by line into the record's `stats`
  `{durationS, frames, sent, dropped, resets, idrRequests, bridgeP50/P99, hostP50/P99, audioLost, audioErrors,
  inputReceived/Sent/Dropped, gate}`; parsing starts at the report's first line (`tap_frames`), and a missing or
  garbled line leaves its field null (never clearing a value already parsed). A quality change restarts the
  bridge inside one session: counts add up, p50 is frame-weighted, p99 the worst, any FAIL fails. Records also keep
  `codec` and `bitrateKbps` (the last ones of the session); old lines without them still read. The live session's id
  is `started` in GET/POST /api/stream.
- Series: `<data_dir>/sessions/<started>.series.json`, 0600, written to a temp file and renamed.
  `PUT /api/sessions/<started>/series {points: [[t, delayMs, fps, mbps], ...]}` merges by t; 400 for a point that is
  not 4 finite numbers in range (t 0..1 week, delay 0..60000, fps 0..1000, 0..10000 Mbit/s; JSON has no NaN), 413
  for more than 17280 points in the request or once merged (5 s each: 24 h; then nothing more is stored) or a body
  over 1 MiB, 404 for a session hostd has no record of. PUTs are serialized. Only the series of the newest 200
  sessions are kept; leftover `.*.tmp` files of a crashed write are swept then.
  `GET /api/sessions/<started>` = `{session, series}`. All need the secret.
- Charts: `layerchart` 2.6.1 (MIT, Svelte 5 native; its d3 deps are ISC/BSD-3/MIT), the `layerchart/svg` entry,
  no Tailwind: it styles itself from CSS variables (`--color-primary`, `--color-surface-100/200/300`,
  `--color-surface-content`), mapped to our tokens in modules/history/Charts.svelte, plus `layerchart/core.css`
  (only its cascade-layer order). No chart motion is configured, so nothing animates in. Charts.svelte is a dynamic
  import inside the detail view: Home, Library and Stream fetch none of it (headless-checked), the History list
  neither; opening a session fetches ~390 KiB of JS uncompressed (~120 KiB gzip; LayerChart + d3).
- [U] Not tried with a real stream: the series upload during a live session on the laptop, the pagehide flush
  when the tab is closed mid-stream, and the report parse against a real bridge exit (the parser is tested on a real
  report from a brisk-up log; the fake bridge in tests prints it).

## Clipboard and file drop
- Clipboard sync (on by default; ghost bar clipboard panel or Settings, key `clipboardSync`) carries text and PNG
  images both ways while the stream is live and the page is focused. Laptop -> bawn: on window focus, tab visible,
  pointer lock and stream start the page reads `navigator.clipboard` and POSTs it if its SHA-256 differs from the
  last synced one. bawn -> laptop: the page polls `GET /api/clipboard?since=<seq>` every second (204 = unchanged,
  JSON for text, the PNG body for an image; `x-brisk-seq` on every answer; no `since` = only the seq, so a fresh
  page never pulls bawn's old clipboard) and writes it with `navigator.clipboard`.
- hostd uses wl-clipboard (`/usr/bin/wl-paste`, `wl-copy`, installed by Toby; wl-clipboard-rs fails
  scripts/check-pure-rust.sh: `cc` and `wayland-sys` via wayland-backend) in the sway session (WAYLAND_DISPLAY from
  the steam process like stream.rs, else the first `wayland-*` socket in the runtime dir). A watcher thread runs
  `wl-paste --watch echo` (dies with hostd, restarted if it exits) and on each event reads `--list-types` then the
  PNG or text, each under `timeout 5`. A seq bumps when the content hash changes. A POST records the hash before
  `wl-copy --foreground --type <mime>` takes the selection, so the watcher does not report it back; the page also
  keeps the last hash (and reads an image back after writing it, as Chrome re-encodes PNGs), so nothing ping-pongs.
  The previous wl-copy is given 1 s to exit by itself, then killed and reaped.
- Limits: 16 MiB per clipboard both ways (bigger is skipped with one toast); other types (files, HTML only) are
  ignored. If Chrome refuses clipboard access the page shows one toast and the panel says how to allow it; while the
  page is unfocused nothing is read and a pending write waits for focus.
- Privacy: with sync on, every copy on the laptop is sent to bawn while the stream has focus, so a password copied
  from a password manager lands on bawn's clipboard (and stays there until something else is copied, even after
  hostd exits). Turn sync off for that. Clipboard contents are never logged.
- Files go laptop -> bawn only: drop files on the Stream page (a "Drop to send" overlay; needs a free pointer, not
  while it is locked) or "Send files…" in the panel. One upload at a time, XHR with progress, a toast with the saved
  path. `PUT /api/files/<name>` streams the body into a hidden temp file in `~/Downloads/brisk` (created on first
  use) and hard-links it to a free name: basename only, no control characters, no leading dots, at most 200 bytes,
  `name (1).ext` if taken. Max 8 GiB (413); with a Content-Length it refuses when less than that plus 256 MiB is
  free (507), and re-checks free space every 64 MiB written; the same name twice at once is a 409; a cut-off
  upload leaves no temp file (and hostd removes `.*.part` leftovers at startup). Files are 0600, the directory 0700
  when hostd creates it. Bidi and zero-width characters are stripped from names.
- [U] Chrome's clipboard permission prompt and `navigator.clipboard.read()` on the pointer-locked canvas, and a
  real image copy both ways, were not tried on hardware; the hostd side was checked live against bawn's sway
  (`cargo test -p brisk-hostd -- --ignored live_`: sets text, reads it with wl-paste, clears it).

## Screenshot and replay clip
- All in the browser, nothing on the host: files download to the laptop (`a[download]`) as
  `brisk-<game or desktop>-<YYYYMMDD-HHMMSS>.png|.mp4` (local time; the game name slugged). Nothing shows on
  screen while playing except the toast ("Saved <name> · copied", "Saved <name> · 30 s · 148.2 MB").
- Ghost bar: Screenshot, and "Save last N s" (N from Settings; disabled when Off). Hotkeys Ctrl+Alt+S
  (screenshot) and Ctrl+Alt+R (clip) are caught on `window` in the capture phase (`reserveHotkeys`,
  capture.ts), so input.ts (window, bubble phase) never sees them and they never reach the game; their keyup
  is swallowed too. The Ctrl and Alt presses before them do reach the game. Chrome has no Ctrl+Alt+S/R
  shortcut on Linux, Windows or macOS. Shift, Meta or AltGr held = not ours (GNOME's Ctrl+Alt+Shift+R
  recorder is untouched; on Windows, where AltGr reports Ctrl+Alt, an AltGr+S character such as Polish ś
  still goes to the game).
- Screenshot: `canvas.toBlob('image/png')` of the video canvas, which is the stream's native size (video.ts
  sizes it to each frame) and holds the last drawn frame (frames are drawn on arrival). Downloaded as soon as
  the PNG is ready; also written to the laptop clipboard when Chrome allows it (ClipboardItem with the PNG
  promise, inside the key/click gesture). ClipSync adopts that image (`adopt`), so clipboard sync does not
  send it to bawn.
- Replay buffer (Settings: Off / 15 / 30 / 60 s, default Off until measured at Crisp, key `replaySeconds`;
  read at each connect): web/src/modules/stream/replay.ts. video.ts hands every access unit that goes to the
  decoder (Annex B, key flag, the frame header) and audio.ts every parsed Opus packet (ts_48k) to one optional
  callback each, by reference (no copy, no await; when Off no callback is passed). Video times are capture_ns,
  or bridge_ns when the tap left it 0. The ring keeps N s plus up to one key interval: the front GOP goes only
  once the next key frame is at or before now - N s, so the latest key frame is never evicted. Byte cap 160 MiB,
  counting each entry's whole buffer (what it keeps alive) + 64 B, oldest first; with one GOP left its newest
  delta goes and recording pauses until the next key frame. One ring per connection, cleared and killed on
  disconnect, reconnect, quality change and leaving /stream.
- Sunshine sends no periodic IDR, so while replay is on (never when Off) the page sends `0x02`
  (MSG_REFRESH_KEY, docs/m2-notes.md) every 10 s without a key frame: the bridge asks for an IDR, at most once
  a second, and drops nothing (unlike `0x01`, which drops deltas until the IDR). Cost: one IDR per 10 s.
- Clip (in the worker, clip-worker.ts): from the last key frame at or before now - N s (else the earliest)
  among those whose SPS/PPS(/VPS) equal the newest key frame's (a resolution change mid-buffer clips only the
  last config), so a clip is N to N+10 s. Audio: packets overlapping that video, placed by their own ts_48k (no
  drift, u32 wrap and reordering handled), mapped to the video clock by the least-queued packet and frame (min
  arrival - send of each); gaps (lost, or a silent host that stops sending) are filled with 1-byte Opus packets
  (the TOC only = a lost frame: the decoder conceals, then silence). Opus pre-skip 120 (libopus lookahead in
  RESTRICTED_LOWDELAY, 2.5 ms). Muxed with mediabunny 1.61.4 (MPL-2.0; mp4-muxer is deprecated in its favour):
  given Annex B and no description it builds the avcC (with the High-profile chroma/bit-depth fields) / hvcC
  from the key frame's in-band parameter sets and rewrites the NAL units length-prefixed; moov at the end,
  written to Blob parts, each packet dropped once written. The main thread only snapshots the ring (0.1 ms)
  and posts it in 2 MB batches (one copy each, transferred, a yield between batches); 30 s timeout.
- Checks: `npm test` (replay.test.ts: ring eviction and retention, key frame choice, config change, audio
  gaps, and a mux of the ffmpeg fixtures in fixtures/ (gen.sh: H.264 Baseline and High, HEVC, Opus) probed
  with ffprobe and decoded by ffmpeg). `node scripts/clip-check.mjs [x.mp4 ...]`: headless Chrome screenshot
  of the latest frame, a Crisp-size ring (155 MB) clipped while a 60 fps canvas loop runs (no long task over
  50 ms; Chrome RSS +400 MB at the peak: the copy in the worker, the MP4 parts, the Blob), the hotkeys against
  input.ts with real key events, and MP4s played in a `<video>`.
- [U] Not tried with a real stream: clip A/V sync on bawn, the refresh-IDR cost at Crisp, Chrome's download
  prompt and clipboard permission while pointer-locked/fullscreen, and HEVC clip playback (headless Chromium
  has no HEVC decoder: the HEVC MP4 loads with duration 3 s but 0x0 video; ffmpeg decodes it clean).

## Env vars
BRISK_APP (default 870779643, Desktop), BRISK_LISTEN (bridge address; default the tailscale IPv4:4433, asked at each stream start),
BRISK_DIST, BRISK_DATA_DIR, BRISK_BRIDGE_BIN, BRISK_STEAM_DIR, BRISK_STEAM_KEY_FILE
(`~/.config/brisk/steam-api-key`, never printed or committed), BRISK_STEAM_API_BASE, BRISK_SECRET_FILE
(default `~/.config/brisk/secret`), BRISK_SECRET (tests), BRISK_TLS_DIR (default `~/.config/brisk/tls`),
BRISK_ALLOWED_HOSTS, BRISK_ALLOWED_USERS, BRISK_SPAWN_SCOPE (1 = `steam` in its own systemd-run scope; set by the unit).

## [U]
`[U]` in code comments marks an unverified assumption about bawn (for example the graphical
environment taken from a running steam process).

## Shaders
- Background smoke only (Home, accent colour, drifts on its own). No pointer interaction and no card shaders; cards only lift and brighten their border on hover. Off on /stream, when the shaders setting is off, and without WebGL2 (CSS gradient instead). Toby's steer, 2026-10-25.

## How to run the smoke test
`cd web && npm run build && node scripts/ui-smoke.mjs` (needs `target/release/brisk-hostd`, or set `BRISK_HOSTD`; port 8443 free).
It starts hostd on fixtures, the fake bridge, a fake `steam` and a mock Steam API (sentinel key, 139 games) in a temp dir,
drives headless Chrome through Home, Library, Machines, History, Settings (every tab), the restart dialog, Ctrl+K,
accent, the 403 guard, a launch to `live` and the ghost bar, runs axe (see Dev tooling) on each of those views, then
prints PASS/FAIL (exit 0 only if all pass) and screenshot paths. It cannot prove video: WebTransport to the fake bridge
fails, so Stream is only checked for degrading to "Lost the connection" without page errors. hostd's listen port and Host
check are fixed at 8443, so while the live service holds 8443 the smoke prints `SMOKE SKIPPED` and exits 0; use
`scripts/axe-check.mjs` for the a11y part meanwhile.

## Dev tooling
- Commands (in web/): `npm run ci` = `npm run check` + `npm test` + `npm run build` (run it before merging);
  `npm run check` = `astro check` + `svelte-check` (fails on errors; the two known svelte-check warnings in Machines and
  GhostBar are left as they are, fixing them would change reactivity); `npm test` = the node unit project only (fast, no
  DOM); `npm run test:browser` = Svelte component tests in real Chrome (separate, not in `ci`).
- Pinning: `web/.npmrc` has `save-exact=true` and every dependency in package.json is an exact version. New packages and
  bumps take the newest version that is at least 14 days old on npm (`npm view <pkg> time --json`; for new transitive
  deps `npm install --before=<date>` where the tree allows it), unless the newer one is a security fix (vitest 5.0.5 and its
  browser packages, fast-uri 3.1.9). Check `npm audit`, `npm ls` and `hasInstallScript` in the lock after a change
  (today only `fsevents`, optional, macOS).
- TypeScript is pinned to 6.0.3: TypeScript 7 (the Go port) is out, but `svelte-check` and `@astrojs/check` declare
  `typescript ^5 || ^6`. Retry when they widen. `web/tsconfig.json` extends `astro/tsconfigs/strict`; vite defines are
  declared in `src/env.d.ts`. `@ts-expect-error` only with a one-line reason; no blanket `any`.
- Accessibility: `scripts/axe.mjs` (`@axe-core/playwright`, MPL-2.0) runs the WCAG 2.0/2.1/2.2 A and AA rules and fails
  on serious or critical violations; exceptions live in its `EXCEPTIONS` list with a one-line reason (today: the command
  menu listbox, scrolled from the input through aria-activedescendant). Used by ui-smoke and by
  `node scripts/axe-check.mjs` (against `npx astro preview --host 127.0.0.1 --port 4424`, mocked /api; Home, Library,
  Machines, History list + session, every Settings tab, Stream idle and failed, command menu, Quit and restart dialogs).
- Browser tests: `*.browser.test.ts` next to the component, run by the `browser` project in `web/vitest.config.ts`
  (`@vitest/browser-playwright` + `vitest-browser-svelte`, the Playwright chrome-headless-shell, override with
  `BRISK_CHROMIUM`). Its API server is pinned to `127.0.0.1:4460` (strictPort): Vitest browser mode had critical advisories
  for an exposed API, so never 0.0.0.0. `astro:transitions/client` is aliased to `src/test/astro-transitions.ts` (a `navigate`
  spy). Covered: CommandMenu (Ctrl+K, filter, Enter navigates, Esc returns focus), the Quit confirm (focus in, Tab trap,
  Esc, focus back), Settings tabs (arrows/Home/End and the hash), GameCard (a click on a not-installed card never installs).

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

## Controller test (Settings)
- Settings has a "Controller" card below the other settings (web/src/modules/settings/ControllerTest.svelte; pure logic and
  tests in controllertest.ts / controllertest.test.ts). It shows the pad picker (index, id, mapping, button and axis counts),
  a rolling poll rate (rAF ticks per second), an SVG pad diagram (aria-hidden; pressed buttons light up in the accent,
  triggers are fill bars), per-stick circle with crosshair, display-only deadzone ring (0-25%, default 8%), x/y as -1..1 and %,
  and a 2 s trail; a range test (32 angle bins, max extent drawn as the gate polygon, Reset), a 3 s drift check (mean offset,
  peak-to-peak noise, OK <2% / Minor 2-6% / Drift >6%, suggested minimum deadzone = ceil(offset + noise); cancelled with
  "You moved it, try again" if a button, a trigger over 30% or a stick over 30% moves), a mapping table (index, label, the
  Moonlight flag from `BUTTON_FLAGS` in lib/gamepad.ts, `analog u8` for triggers, `i16` for sticks with the Y-inverted note;
  the live row is highlighted) and a Weak / Strong / Both rumble test (`dual-rumble`, 400 ms).
- Lifecycle: the rAF loop runs only while the card is in view (IntersectionObserver), the document has focus and is visible;
  unmount cancels it, removes every listener (one AbortController) and resets the actuators. It reads `navigator.getGamepads()`
  only: no fetch, no hostd, nothing reaches the bridge, and it is not on the Stream route. Reduced motion needs nothing special
  (no animations; the live view is data). The polite status line (role=status, visually hidden, mirrored on screen) speaks only
  for connect/disconnect and the drift outcome.
- Chrome shows a pad only after its first button press, hence the hint when none is listed. Readings use the raw axes (y down
  is positive on screen); the table says what Brisk sends (y flipped). Headless-checked with a scripted fake
  `navigator.getGamepads`; [U] not tried with the real 8BitDo Ultimate 2 (rumble feel, real poll rate, real stick noise).
- The diagram (PadDiagram.svelte, geometry in pad.svg.ts, 1 unit = 1 mm, viewBox -80 -60 160 120) is the 8BitDo Ultimate 2
  Wireless to scale: body 147 x 103 mm (official spec), every position and size scaled from the official manual's front
  diagram (vector line art, body box set to 147 mm; outline within about 0.6 mm of it, controls within 0.3 mm). Our own
  geometry, no 8BitDo artwork or logo. Triggers, L4/R4 and the USB-C are a top-down projection above the top edge (from the
  manual's top view). L4/R4, PL/PR and Square/Star/Profile are drawn faint with a caption: Chrome's standard mapping only
  reports the 17 standard buttons. Sources and open questions: research/8bitdo-ultimate-2-dimensions.md. pad.svg.test.ts
  checks the body aspect, one element per standard button 0..16, and unique ids.
