# Emulator games as first-class games (roadmap)

Status: Stages 1 to 6 are merged to main through `stack-all` (PR #13); `2026-10-28-stack-all.md` lists what each brought.

Toby tested the first emulator version (Dolphin, hand-written `emulators.json`, Play and Controls in the
detail panel). His verdict: it "feels very patched in", and he wants the emulator process "as clean as a
normal game". He wants art, a description and achievements for these titles, a way to get game files onto the
machines, and launch options for emulators and Steam (for example run any game with MangoHud).

Related docs:

- Controller editor: `docs/plans/2026-10-27-emulator-controls.md` (v1, built) and
  `docs/plans/2026-10-28-emulator-controls-v2.md` (v2 design, written separately). This roadmap only says where
  the editor lives (Settings > Controller > "Emulator mappings") and how it finds games.
- Metadata and achievements research: `docs/research/emulator-metadata-and-achievements.md`. Stage 3 and 5
  depend on it. This plan does not repeat it.
- Hub: `docs/plans/machine-hub.md`. Design: `docs/design.md`.

## 1. Intent and non-goals

Intent: a game is a game. Steam titles and emulator titles have the same shape in the API, the same card and
detail panel in the web app, the same running badge, Play and Quit, and differ only where the source forces it
(Steam installs and updates, emulators have files and systems).

Non-goals:

- No second emulator in the first pass. The registry makes the next one a table row, but Dolphin ships first.
- No ROM downloading, no ripping, no legality checks. Brisk handles files the user already has.
- No archive unpacking (zip, 7z) on upload. Use RVZ, ISO and friends as they are.
- No cloud sync of saves, no save editing. Stage 6 only lists them.
- No hot-plug folder watcher with inotify. A rescan on demand and on a timer is enough.
- No cross-machine game identity beyond the id scheme below (a Wii game has the same id on every machine).
- No attempt to keep old `/api/games` shapes working. The web app and hub change together.

## 2. Target architecture

### 2.1 The Game model

Today `steam::Game` is Steam-shaped (`appid: u32`) and `emu.rs` fakes ids in the range 0xE0000000 and up from a
hash of the name. Emulator games then dodge Steam code with `is_emu_id` checks in `http.rs` (games, steam_cmd,
check_appid), in `running.rs` (quit scope) and in `stream.rs`. That branching is the "patched in" feel.

New model, one struct for every source:

```
Game {
  id: String,            // "steam:620", "emu:dolphin:RSBE01"
  source: "steam" | "emu",
  system: Option<String>,// "wii", "gamecube"; None for Steam
  name, installed, sizeBytes,
  lastPlayed: Option<u64>, playtimeMin, playtime2wkMin,
  cover, hero, logo,     // art URLs, empty if none
  running: bool,
  achUnlocked, achTotal: Option<u32>,
  updated, update, controller, coop,   // Steam facts stay, null for emulators
  caps: { install, controls, achievements, launchOptions, saves, screenshots },
  legacyId: Option<String>,            // one release only, see 2.3
}
```

`caps` replaces every `is_emu_id` branch: the web app shows Install only if `caps.install`, Controls only if
`caps.controls`, and so on. Hostd asks the source, not the id range.

Id scheme:

- Steam: `steam:<appid>`. The numeric appid is the source id.
- Emulator: `emu:<emulator>:<stable id>`. The stable id comes from the file, not the name or path, so renaming
  or moving a file keeps notes, tags, playtime and art. The registry gives each system an `ident(file)`:
  Dolphin reads the 6-character game ID (`RSBE01`) from the disc header, plus the disc number for multi-disc
  games when it is not 0 (`RSBE01-d1`). Fallback when there is no header: first 10 hex chars of SHA-1 of
  `system + lowercase file stem`. Two files with the same id (a duplicate dump): the second gets `~2`.
- Ids are strings everywhere: Rust `GameId` newtype (`Source`, `String`), TypeScript `type GameId = string`.
  They go in URLs percent-encoded (`/api/games/steam%3A620/info`; axum decodes path segments, so handlers
  receive `steam:620`).

What happens to numeric ids: they are dropped from the API. The server still accepts a bare number in a path
or body for one release and reads it as `steam:<n>` (old bookmarks, old cached clients). Emulator numeric ids
are not accepted; they were never stable enough to keep.

### 2.2 The source interface

One trait in `crates/brisk-hostd/src/source/mod.rs`, two implementations. Steam code moves behind it without
rewriting (`steam.rs`, `steam_info/*` stay; a thin `source/steam.rs` adapts them).

```
trait GameSource {
  fn id_prefix(&self) -> &'static str;                       // "steam" | "emu"
  fn list(&self, refresh: bool) -> Vec<Game>;                // fast, from caches
  fn owns(&self, id: &GameId) -> bool;
  fn launch(&self, id, profile: &LaunchProfile) -> bool;     // spawn, detached
  fn install(&self, id) -> Option<bool>;                     // None: not supported
  fn scope(&self, ps: &[Proc], id) -> Option<Scope>;         // running? which pids, which window class
  fn focus_class(&self, id) -> Option<&str>;                 // window to focus after launch
  fn info(&self, id) -> Info;                                // description, genres, year, screenshots
  fn achievements(&self, id) -> Achievements;
  fn art(&self, id, kind: ArtKind) -> Option<ArtFile>;
  fn local(&self, id) -> Local;                              // screenshots, saves, states
}
```

`Scope { reapers, kids, class }` is what `running::scope` already computes inside `Quitter`. The Quitter stops
knowing about appids and `emulators.json`; it asks `source.scope(...)`. That is also what makes Quit work the
same for both (tonight's reliability fix lands in the shared quit code, and this stage keeps it there).

`running_appids` becomes `running_ids(sources) -> HashSet<GameId>`, computed once per `/api/games` call.

### 2.3 Where state lives, and the migration

Today (all in `data_dir`, hub or machine):

| File | Where | Key today | Key after |
| --- | --- | --- | --- |
| `notes.json` (tags, per-game note and tags) | hub (user level) | `u32` appid | game id string |
| `prefs.json` (per-game stream quality) | hub | `u32` appid | game id string |
| `sessions` (history rows have `appid`) | machine | `appid: Option<u32>` | `gameId: Option<String>`, old rows keep `appid` and read as `steam:<n>` |
| Dolphin mapping (`WiimoteNew.ini`) | machine | per machine | unchanged |
| `emulators.json` | machine | name hash | replaced by `library.json` |

New file on each machine: `data_dir/library.json`, the emulator game list. One record per game:
`{id, emulator, system, file (relative to the games folder when inside it), name, addedAt, source: "scan"|"upload"|"manual", meta: {...}}`.
Metadata match state lives next to it (Stage 3). Removing a game deletes the record (and optionally the file).

Migration, one-shot at hostd start, idempotent, with backups:

1. `emulators.json` -> `library.json`: for each entry, read the header, compute the new id, write the record.
   Rename the old file to `emulators.json.migrated`. Remember `legacyId` (the old hash id, as decimal text)
   for each record and publish it in `/api/games` for one release.
2. `notes.json` and `prefs.json` on the hub: copy to `*.pre-ids` first. Numeric keys below 0xE0000000 become
   `steam:<n>`. Keys at or above 0xE0000000 (emulator games) cannot be mapped by the hub alone: when the hub
   first gets a machine's `/api/games`, it maps `legacyId` to `id` and rewrites those keys. Unmapped keys are
   kept as they are and listed in the log, never dropped.
3. Sessions: no rewrite. The reader maps `appid` to `steam:<n>` and old emulator rows to the name they stored.
4. The migration is a pure function (old JSON in, new JSON out) with fixture tests, so a real run is only the
   file swap.

Notes and prefs stay on the hub, keyed by id. A Wii game's id is the same on every machine, so a note follows
the game. That is deliberate.

### 2.4 The emulator registry

A static table in `crates/brisk-hostd/src/emu/registry.rs` (Rust, no config file in the first pass; a machine
override for the command path comes with Stage 4's settings). One row per emulator:

```
Emulator {
  id: "dolphin",
  name: "Dolphin",
  cmd: "dolphin-emu", args: ["-b", "-e"],      // then the game file, as its own argv entry
  window_class: "dolphin-emu",
  config_dir: from Config.dolphin_dir,
  controller: ["wii-classic"],                 // editor types, see controls v2
  systems: [
    System { id: "gamecube", folder: "GameCube", ident: dolphin_header },
    System { id: "wii",      folder: "Wii",      ident: dolphin_header },
  ],
  extensions: ["iso","rvz","wbfs","gcm","ciso","gcz","wia","wad","tgc"],
  saves: [ ... ],      // Stage 6
  achievements: Some(RetroAchievements { .. }),  // Stage 5, from the research doc
}
```

`shortcut:` from today's `emu.rs` (one window class, no pid match) stays a known limit; the registry adds
`match_by: Args | Pid` per emulator when a second emulator collides.

Adding PCSX2, RPCS3, Cemu, Ryujinx later is: a row, an `ident` function per system, a controller type if it
has an editor.

### 2.5 API changes

All under `/api`, all machine-scoped paths go through `/api/m/{id}/` on a hub exactly as today.

| Today | After |
| --- | --- |
| `GET /games` -> `{games:[{appid,...}]}` | `GET /games` -> `{games:[Game], ownedAvailable}`. Includes emulator games from every source. |
| `/games/{appid}/info` | `/games/{id}/info`. Emulators answer from the metadata cache; shape gains `description`, `genres`, `year`, `matchState`. |
| `/games/{appid}/achievements`, `/news`, `/local`, `/screenshots/{name}` | same paths with `{id}`. `news` is empty for emulators. `local` gains `states` and `saves` lists (Stage 6). |
| `/games/{appid}/controls[...]` | `/games/{id}/controls[...]`; gate is `caps.controls`. Becomes the Emulator mappings editor in Settings (controls v2). |
| `POST /launch {appid}` | `POST /launch {id}`; `/install {id}` answers 400 when `caps.install` is false. |
| `POST /quit {appid,force}` | `POST /quit {id,force}`. Same 25 s answer, same 409s. |
| `/notes/games/{appid}`, `/prefs/{appid}` (hub) | `{id}` strings. |
| `emulators.json` (hand edit) | `POST /library/games`, `DELETE /library/games/{id}` and the ingest routes below. |

Running: `Game.running` stays the single source in `/api/games` (the web already polls it). There is no
separate emulator running route.

New routes by stage: `/library/*` (Stage 2), `/games/{id}/meta` and `/library/match` (Stage 3),
`/launch-options` (Stage 4), `/games/{id}/screenshots` POST (Stage 6).

### 2.6 Hub proxy and web client

Hub: `/api/m/{id}/{*path}` forwards unchanged, so the id change needs no hub code. Two real changes:

- `files/*` is refused today (501) and the body cap is 16 MiB each way, with a 40 s timeout. Library upload
  needs a streaming pass-through route for `library/uploads/*` (no cap, no total timeout, idle timeout only,
  headers `Upload-Offset` and `Content-Range` forwarded). This is the main hub work in Stage 2.
- The migration in 2.3 step 2 (hub rewrites note and pref keys using `legacyId`).

Web (`web/src/state/api.ts` and `web/src/modules/library/*`):

- `Game.appid: number` -> `Game.id: string`; every `appid` parameter in `api.ts` becomes `id: string`
  (`encodeURIComponent` once, in a `gameUrl(id, tail)` helper). `SCOPED` in `api.ts` needs `library` added.
- `library.ts`, `GameCard`, `GameDetail`, `Shelf`, `Hero`, notes and prefs keys: type change plus the `caps`
  checks instead of any "is this an emulator" test.
- Settings gets two sections: "Emulator mappings" under Controller and, in Stage 4, "Launch options".
- Library gets an "Add game" button (Stage 2) and a "Remove" action in the detail menu.
- The web browser tests and `library.test.ts` fixtures change shape in the same commit as the type change.

## 3. Stages

Sizes are rough working days for one agent plus review: S about 1, M about 2 to 3, L about 4 to 5.

### Stage 1: unified model and sources (refactor)

- Goal: the architecture above with no new features. Everything that works tonight works the same, through one
  path.
- Size: L. Touches `steam.rs`, `emu.rs`, `http.rs`, `running.rs`, `stream.rs`, `notes.rs`, `prefs.rs`,
  `controls.rs`, `sessions.rs`, `api.ts`, the library module.
- Depends on: tonight's Quit reliability fix and UI fixes merged (so the refactor starts from a green base),
  and the controls v2 work not being mid-flight in `controls.rs` (merge order: v2 first, or v2 rebases on this).
- Replaces: `is_emu_id`, the 0xE0000000 hash ids, `emulators.json`, `emu::games`/`launch`/`focus` as
  free functions, the emulator branch in `steam_cmd`, the emulator special case in `Quitter::scope`, numeric
  game ids in the web app.
- Steps (each leaves the tree green):
  1. `GameId` and `Game` types, `GameSource` trait, Steam adapter. API still returns the old shape through a
     mapper. Pure code move.
  2. Emulator source and registry behind the trait; `library.json` with the migration from `emulators.json`.
  3. Routes take `{id}`; launch, install, quit, running go through sources.
  4. Notes, prefs and sessions keys, with migration and `*.pre-ids` backups.
  5. Web client and hub together: `id: string`, `caps`, tests.
- Tests: migration fixtures (old notes, prefs, emulators.json in, expected out, run twice, second run is a
  no-op); id derivation from header fixtures (tiny fake GameCube and Wii headers made in the test); `caps`
  per source; route tests for `steam:620`, a bare `620`, a bad id (400), an unknown id (404); the existing
  quit tests untouched except for the scope call; web unit and browser tests updated; one manual pass on bawn
  (Steam game, Dolphin game: Play, running badge, Quit, notes still there).
- Needs Toby: an OK to ship a breaking API change (decided), and a moment to check his notes and tags after
  the deploy.

### Stage 2: library ingest (files in)

- Goal: no hand-edited JSON. Game files get onto a machine three ways and show up as games.
- Size: L (the resumable upload through the hub is most of it).
- Depends on: Stage 1.
- Replaces: hand-written `emulators.json`; the 8 GiB `MAX_UPLOAD` for games (the plain file drop keeps its cap).
- Pieces:
  - One games folder per machine, default `~/Games`, setting `BRISK_GAMES_DIR` (and a field in Settings).
    One subfolder per system from the registry: `GameCube/`, `Wii/`. Hostd creates them on first use.
  - Folder scan: walks the games folder (depth 2), takes files with a registry extension, detects the system by
    header, and adds or updates `library.json` records (`source: "scan"`). Files are matched to records by
    stable id. A file in the wrong system folder is still found; brisk offers to move it, never moves it
    silently. Missing files keep their record and show "not installed" (as today). Runs at start, on
    `GET /games?refresh=1`, via `POST /library/scan`, and every 5 minutes. Drop files any way you like
    (scp, a USB stick, Syncthing) and brisk finds them.
  - Header detection (registry `ident`): GameCube magic at 0x1C and Wii magic at 0x18 for plain ISO/GCM/GCZ-after-
    read; `WBFS` header at 0x200; RVZ and WIA carry a copy of the disc header in their own header. Unknown
    extension or no known magic: not a game, listed under "unrecognised files" in the scan answer, not added.
  - Resumable upload, in `files.rs` style (stream to disk, free-space reserve, sanitised names, temp file
    guard) but with offsets:
    - `POST /library/uploads {name, size}` -> `{uploadId, offset: 0}` (checks free space against `size`).
    - `PATCH /library/uploads/{uploadId}` with `Upload-Offset: n` and a body chunk (the web sends 32 MiB
      chunks) -> `{offset}`. Wrong offset: 409 with the real one.
    - `HEAD`-style `GET /library/uploads/{uploadId}` -> `{offset}` so a page reload resumes.
    - On the last byte: detect system, move into `Games/<System>/`, add the record, answer `{game}`.
    - Partial files live in `<games>/.incoming/`; stale ones (7 days) are cleaned at start, as `clean_partials`
      does today.
  - Size limit: default 64 GiB per file (a Wii U or Switch-class dump is far above 8 GiB), `BRISK_MAX_GAME_BYTES`
    to change it, plus the existing free-space reserve. A tus-compatible shape was considered; the three routes
    above are the same idea without the dependency.
  - Web: Library "Add game" dialog. Pick the machine (defaults to the selected one), pick files or drop them,
    per-file progress bar with speed and a Cancel, resume after reload, errors per file. When done the game
    appears, and (Stage 3) goes to match review.
  - Command and API for scripts: `POST /library/uploads` family is the contract. `scripts/brisk-add.sh <machine>
    <file>...` is a thin curl wrapper that reads the machine's address and secret from the user's own
    config and resumes with the offset route. On the machine itself, `brisk-hostd library add <path>` copies
    or hard-links into the right folder and rescans (no network).
  - Remove: `DELETE /library/games/{id}` drops the record; `?deleteFile=1` also deletes the file, behind a
    confirm in the UI that names the file and size.
- Tests: scan with a temp folder (fake headers, wrong folder, unknown file, duplicate dump, missing file);
  upload state machine (happy path, wrong offset, resume, disk full, size over limit, cancel cleans up, name
  traversal); hub pass-through test with a slow upstream (no 40 s cut, no 16 MiB cap, offset header
  forwarded); web unit tests for the upload queue; browser test for the dialog; a real 4 GB resume on bawn once.
- Needs Toby: confirm the default games folder path; one real game file to try.

### Stage 3: metadata and art

- Goal: cover, hero, logo, description, genres and release year for emulator games, fetched once per game and
  kept on the machine.
- Size: L.
- Depends on: Stage 1 (Stage 2 to add games without hand editing); the research doc picks the sources. Pending
  that, the working assumption is: an exact lookup by Dolphin game ID where a database offers one (likely no
  match question at all for Wii and GameCube), and a name search with a confidence score otherwise.
- Replaces: the empty `cover/hero/logo` strings in `emu::games`.
- Pieces:
  - Cache in `data_dir/meta/<game id>/`: `meta.json` (name, description, genres, year, source, sourceId,
    matchState, matchedAt) and the art files. Art is served by the same `/games/{id}/art/{kind}` route Steam
    uses, so `Cover.svelte` and `Hero.svelte` need no emulator branch.
  - `matchState`: `matched` (exact id or high score), `weak` (needs a confirm), `none`, `manual` (set by
    Toby). Looked up once; a refresh button on the game re-runs it.
  - Review-match UI: a "Needs review" strip in Library (count badge) and a dialog per game: the top 5 candidates
    with cover, year and system, "None of these" and "Search again" with typed text. Confirm writes `manual`
    and fetches the art. Same dialog is reachable from the detail panel.
  - Keys: a Settings section "Metadata sources" for the free API keys, stored in a 0600 file in the machine's
    `data_dir` like `steam_key_file`, never in the repo, never echoed back to the browser (the UI shows "set").
  - Hub side: metadata is fetched by the machine that owns the file, once. If the same game is on two machines
    each fetches its own copy (small, simple).
  - Offline: no key or no network means `matchState: none` and a text-only card with the system name as a
    placeholder art. Never an error toast per game.
- Tests: a fake metadata server in the test (same pattern as `steam_api_base`); match scoring unit tests on
  names (region tags, "(USA)", "Disc 1"); cache hit makes zero requests; rate limit and a 429 backoff; the
  review dialog browser test; art route test for a missing file.
- Needs Toby: create the free accounts and keys the research doc names; test the review flow with a few odd
  titles.

### Stage 4: launch options (Steam and emulators)

- Goal: define launch options once in Settings and use them on any game, for example MangoHud.
- Size: M for emulators, M for Steam (the Steam half has a spike first).
- Depends on: Stage 1 (profiles apply through `GameSource::launch`). Independent of Stages 2 and 3, so it can
  move earlier if Toby wants it sooner.
- Replaces: nothing today (there is no launch customisation); `emu::launch` gets the profile argument.
- Model (`data_dir/launch.json`, on the machine, since commands differ per machine):
  - `presets`: `[{id, name, wrapper?, env: {K: V}, args: [String]}]`. Example: `mangohud` =
    `{wrapper: "mangohud"}`; `perf-hud` = `{env: {DXVK_HUD: "fps"}}`.
  - `games`: `{ "<game id>": {presets: [id...], env: {...}, args: [...]} }`, and `defaults` per source.
  - Resolution order: source default, then the game's presets in order, then the game's own env and args.
- Safe execution (the hard rule: no shell strings anywhere):
  - Everything is argv. `wrapper` is not free text: it is a name from a fixed allowlist in code
    (`mangohud`, `gamemoderun`, `gamescope`, `env`-less) with its own typed options (gamescope: width, height,
    refresh, fullscreen, each validated as numbers or booleans). Brisk builds `[wrapper argv] [cmd] [args] [file]`
    and hands it to `Runner::spawn`, which already takes separate args.
  - `env` keys must match `^[A-Z_][A-Z0-9_]*$` and be on an allowlist of names or prefixes (`MANGOHUD*`,
    `DXVK_*`, `VKD3D_*`, `PROTON_*`, `__GL_*`, `MESA_*`, `RADV_*`, `SDL_*`, `WINE*`, `LIBGL_*`). Never allowed:
    `LD_PRELOAD`, `LD_LIBRARY_PATH`, `PATH`, `HOME`, `BASH_ENV`, `ENV`, anything starting `BRISK_`. Values are
    limited to printable ASCII without newline or NUL, max 512 bytes.
  - `args` (emulators only) are separate argv entries, max 32, each max 256 bytes, no NUL. Dolphin's own
    flags that run code or change the user dir (`-u`, `--user`) are refused by a per-emulator denylist.
  - The settings endpoint validates on write and again on launch; a failure launches with no profile and tells
    the UI why, instead of refusing to start the game.
  - Needs auth like every other write (the machine secret and the csrf header); hostd already runs unprivileged.
- Emulators: easy. Hostd builds the argv itself. Quit must signal the wrapper's child, not only the wrapper:
  `scope` already matches by process name and file argument, and it needs a test with a wrapper in the chain
  (`gamescope` is the parent of the emulator, `mangohud` just sets `LD_PRELOAD` and execs).
- Steam: Steam, not hostd, spawns the game, and an env var set on the `steam steam://rungameid/N` call only
  reaches a Steam that is not running yet. Three ways, in order of preference:
  1. A small pure-Rust helper `brisk-wrap`, put once into each Steam game's own Launch Options as
     `brisk-wrap %command%`. At launch it reads `launch.json` by `SteamAppId` and `exec`s the real command
     with the resolved wrapper and env. After that one-time step every option is edited live in Brisk, with no
     Steam restart. Same validation as above, no shell.
  2. Writing `LaunchOptions` into Steam's `localconfig.vdf` for the game, which only works with Steam closed
     and gets overwritten otherwise. Spike first; use it only to set the one-time `brisk-wrap %command%`.
  3. Fallback: show the exact line to paste into Steam's own Launch Options.
  The stage starts with a half-day spike to choose between 1 plus 2 and 1 plus 3.
- Web: Settings > "Launch options" (list presets, add and edit with typed fields and a live preview of the
  resulting command line, test button that is just a dry-run showing the argv), and a "Launch options" block in
  the game detail panel to pick presets for that game. Works for Steam and emulator games the same way.
- Tests: argv builder unit tests (no field can add an argv entry through spaces, quotes or `;`), env allowlist
  and denylist tables, validation errors, resolution order, a launch test with a fake runner for each source,
  quit with a wrapper in the process tree, `brisk-wrap` unit test (profile in, argv out), web form tests.
- Needs Toby: the list of wrappers and env names he really wants in the first allowlist; whether he accepts
  the one-time `brisk-wrap %command%` step per Steam game (or wants the vdf edit).

### Stage 5: achievements

- Goal: achievements for emulator games where a source exists. RetroAchievements is wanted even if a part is
  manual.
- Size: M to L, depends on the research.
- Depends on: Stage 1 (the `achievements` source method and shape already exist: id, name, description, icon,
  unlockedAt) and `docs/research/emulator-metadata-and-achievements.md`, which decides what Dolphin and
  RetroAchievements support for GameCube and Wii, how a game is hashed to find its set, and what the API
  needs.
- Plan, assuming the research confirms an account-plus-key web API:
  - Settings > "RetroAchievements": user name and Web API key, stored like the metadata keys.
  - Game to set: the hostd computes the hash the research names, once, caches it with the metadata.
  - Achievement list and the user's progress for the set come from RA and fill `Achievements`; icons are
    cached on the machine like art. Counts feed `achUnlocked` and `achTotal` on the card.
  - Manual part, if the emulator cannot report unlocks to RA: the list is shown locked, and each item can be
    ticked by hand; ticks are stored with the game and counted on the card. Clearly labelled "marked by you".
    If the emulator does report (RA login inside the emulator), the site's progress wins.
- Tests: fake RA server; hash fixtures from the research; empty set; wrong key (clear message, no retry loop);
  manual tick persistence.
- Needs Toby: an RA account and Web API key; a decision on hardcore versus softcore if the emulator asks.

### Stage 6: playtime, screenshots, saves

Three independent pieces, in this order.

6a. Playtime and last played (size S, do it right after Stage 1):

- A watcher task in hostd samples `running_ids` every 5 s and records start and stop per game, so time counts
  while the process runs even when nobody has the web app open and survives a hostd restart (on start it
  re-reads the running set and carries on the open session).
- Store `data_dir/playtime.json`: per game id, session list (start, seconds), capped (last 500 sessions per
  game, older ones folded into a total). `playtimeMin`, `playtime2wkMin` and `lastPlayed` derive from it.
- Steam games keep Steam's own numbers; only emulator games use brisk's. (Brisk could also log Steam sessions
  for a cross-check; skip.)
- Tests: fake clock and fake proc dir: start, stop, restart in the middle, two games, a crash (process gone),
  cap folding.

6b. Screenshots taken from the stream (size M):

- A camera button in the stream view grabs the current video frame in the browser (canvas to PNG or JPEG) and
  posts it to `POST /games/{id}/screenshots`. No host capture tool is needed, and it works the same on a VM.
- Stored in `data_dir/shots/<game id>/`, listed and served through the existing `local` and
  `screenshots/{name}` routes, which move behind `GameSource::local`. The existing lightbox needs no change.
- Caps: 8 MiB per image, 200 per game, names generated by hostd.
- Tests: upload validation (type sniff, size), listing order, delete, the lightbox with a mixed list.

6c. Saves and save states list (size M):

- Registry rows say where to look. Dolphin: save states are `StateSaves/<GameID>.s01` to `.s10`; Wii saves are
  per title under `Wii/title/00010000/<hex of the 4-char game ID>/data`; GameCube saves live on memory card
  images (`GC/<region>/Card A.raw`), which are per region, not per game, so they are listed as a card with a
  note, not per game.
- Read-only list in the detail panel: kind, file, size, modified time. No download in this pass.
- Tests: fixture Dolphin tree, each kind, a game without saves, a missing config dir.
- Needs Toby: whether a "back up saves" or "download" button belongs in a later pass.

## 4. Risks

- Stage 1 is a wide breaking change with data migrations. Mitigation: backups (`*.pre-ids`), pure migration
  functions with fixtures, idempotent runs, notes never dropped, and web plus hub plus hostd shipped together.
- Controls v2 and Stage 1 both touch `controls.rs` and the game-detail panel. Merge order must be agreed
  (see Stage 1 dependencies).
- Upload through the hub is new streaming code in the least tested part of `machines.rs`. A slow or dropped
  link must not leave half files or stuck locks (the `Guard` pattern from `files.rs` applies).
- Metadata matching is where wrong data appears. The `weak` state and the review UI are the protection;
  nothing weak is shown as fact.
- Metadata source terms and rate limits are unknown until the research lands. A source that forbids caching
  art would change Stage 3.
- Steam launch options: Steam owns the spawn, so the wrapper path depends on `brisk-wrap` and a one-time step
  per game. Steam updates can rewrite `localconfig.vdf`.
- Launch options are the first place Brisk lets the browser influence a spawned command. The allowlist and the
  no-shell rule are the security boundary; they need review before the stage ships.
- Dolphin specifics (RVZ header offset, GameTDB-style ids) are assumptions to check against real files.
- Public repo: keys, game file names and paths of Toby's library must never end up in fixtures, docs or logs.
  Test headers are generated in the tests.

## 5. Cheapest order

1. Stage 1 (everything else is cheaper after it).
2. Stage 6a playtime: small, and it makes emulator cards stop looking empty.
3. Stage 2 ingest: removes hand-edited JSON, the loudest remaining rough edge.
4. Stage 4 launch options: Toby asked for it by name; it is independent and can swap with Stage 2.
5. Stage 3 metadata and art (needs the research and keys).
6. Stage 6b screenshots, then 6c saves.
7. Stage 5 achievements last: blocked on research and an account, and the biggest unknown.

Controller editor v2 (Settings) is separate and goes in whenever its design lands; after Stage 1 it just
switches to `id: string`.

## 6. Decisions I made for you

- Ids are `steam:<appid>` and `emu:<emulator>:<id from the file header>`, not a hash of the name. Bare numbers
  mean Steam for one release, and old emulator ids are not kept.
- `library.json` replaces `emulators.json`; old file is migrated and renamed, not deleted.
- Notes and prefs stay on the hub, keyed by the new ids, so a Wii game's note follows it across machines.
- One games folder per machine (`~/Games`), one subfolder per system, system detected from the file header,
  wrong folder is flagged, never silently moved.
- Rescan on demand and every 5 minutes, no inotify.
- Upload is a small resumable protocol of our own (three routes), 32 MiB chunks, 64 GiB default cap.
- Metadata is fetched on the machine that owns the file, once, and cached there. Keys live on the machine in a
  0600 file and are write-only from the browser.
- Launch options are presets plus per-game picks, argv only, a fixed wrapper allowlist and an env name
  allowlist. Steam uses a `brisk-wrap %command%` shim so options stay editable in Brisk.
- Playtime is counted by a background watcher from the process running, not from focus or stream time.
- Screenshots are frame grabs from the browser's stream view, not a host capture.
- Save listing is read-only for now.
- Stage 5 is last, and a hand-ticked fallback is accepted for RetroAchievements.

## 7. Open questions for Toby

- Default games folder: is `~/Games` right on bawn and the Proxmox games VM, or do you want a bigger disk path?
- Do you want a per-file 64 GiB cap, or none beyond free space?
- For Steam launch options, one-time `brisk-wrap %command%` per game (my pick), or should brisk edit Steam's
  config for you?
- Which wrappers and environment variables must be in the first allowlist (MangoHud, GameMode, gamescope,
  which others)?
- Are you willing to create the free metadata and RetroAchievements accounts once the research doc names them?
- Should deleting a game from the Library ever offer to delete the file, or only forget it?
