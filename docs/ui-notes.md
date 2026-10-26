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
- GET /api/prefs, PUT/DELETE /api/prefs/<appid> (see Per-game quality)
- POST /api/launch, POST /api/install, POST /api/sunshine/restart
- POST /api/quit `{appid, force?}` (see Quit game)
- GET/POST /api/clipboard, PUT /api/files/<name> (see Clipboard and file drop)
- anything else: the static page from `web/dist`

Guard: the Host header must be `localhost:8443` or `127.0.0.1:8443` (DNS rebinding). There is no Origin check; that is
safe because the secret is required and hostd sends no CORS headers, so other origins cannot read or send it.
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
  screen while playing except the toast ("Saved <name> · copied", "Saved <name> · 29 s · 148.2 MB").
- Ghost bar: Screenshot, and "Save last N s" (N from Settings; disabled when Off). Hotkeys Ctrl+Alt+S
  (screenshot) and Ctrl+Alt+R (clip) are caught on `window` in the capture phase
  (Stream.svelte), so input.ts (window, bubble phase) never sees them and they never reach the game; their
  keyup is swallowed too. The Ctrl and Alt presses before them do reach the game. Chrome has no Ctrl+Alt+S/R
  shortcut on Linux, Windows or macOS (Shift or Meta held = not ours: GNOME's Ctrl+Alt+Shift+R recorder is
  untouched). On Windows Ctrl+Alt is also AltGr, so on a layout where AltGr+S types a letter (Polish: ś)
  that letter cannot be typed into the game.
- Screenshot: `canvas.toBlob('image/png')` of the video canvas, which is the stream's native size (video.ts
  sizes it to each frame) and holds the last drawn frame (frames are drawn on arrival). Also written to the
  laptop clipboard as PNG when Chrome allows it (ClipboardItem with the PNG promise, inside the key/click
  gesture). With clipboard sync on, that image then also syncs to bawn like any copy.
- Replay buffer (Settings: Off / 15 / 30 / 60 s, default 30, key `replaySeconds`; read at each connect):
  web/src/modules/stream/replay.ts. video.ts hands every access unit that goes to the decoder (Annex B,
  key flag, the frame header's bridge_ns) and audio.ts every parsed Opus packet (ts_48k) to one optional
  callback each, by reference (no copy, no await; when Off no callback is passed). The ring evicts by
  arrival time (older than N s, then through to the next key frame: deltas without their IDR are useless)
  and by bytes (160 MiB for video + audio + 64 B per entry, oldest first). One ring per connection, cleared on
  disconnect, reconnect, quality change and leaving /stream. Sunshine sends no periodic IDR, so while replay
  is on the page asks the bridge for a key frame (`0x01`) whenever none came for 5 s: the clip starts at
  one, so it is N-5..N s long. Cost: one IDR every 5 s (a burst of a few hundred KB at Crisp).
- Clip: from the first key frame at or after now - N s (else the earliest) whose SPS/PPS(/VPS) equal the
  newest key frame's (a resolution change mid-buffer clips only the last config); video times from
  bridge_ns. Audio: packets overlapping that video, placed by their own ts_48k (no drift, u32 wrap and
  reordering handled), mapped to the video clock by the least-queued packet and frame (min arrival - send
  of each); gaps (lost, or a silent host that stops sending) are filled with 1-byte Opus packets (the TOC
  only = a lost frame: the decoder conceals, then silence). Opus pre-skip is 120 (libopus lookahead in
  RESTRICTED_LOWDELAY, 2.5 ms). Muxed in a Worker (clip-worker.ts, mp4.ts) with mediabunny 1.61.4
  (MPL-2.0; mp4-muxer is deprecated in its favour): given Annex B and no description it builds the avcC /
  hvcC from the key frame's in-band parameter sets and rewrites the NAL units length-prefixed. Packets go
  to the worker in 8 MB batches (one copy each, then transferred) with a yield between batches.
- Checks: `npm test` (replay.test.ts: ring eviction, key frame choice, config change, audio gaps, and a
  mux of the ffmpeg fixtures in fixtures/ (gen.sh) probed with ffprobe and decoded by ffmpeg, both codecs).
  `node scripts/clip-check.mjs [x.mp4 ...]`: headless Chrome screenshot of the latest frame, a Crisp-size
  ring (155 MB) clipped while a 60 fps canvas loop runs (no long task over 50 ms; clip() about 20 ms, the rest
  in the worker), and MP4s played in a `<video>`.
- [U] Not tried with a real stream: clip A/V sync on bawn, the IDR-every-5-s cost at Crisp, Chrome's
  download prompt and clipboard permission while pointer-locked/fullscreen, and HEVC clip playback (headless
  Chromium has no HEVC decoder: the HEVC MP4 loads with duration 3 s but 0x0 video; ffmpeg decodes it clean).

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
