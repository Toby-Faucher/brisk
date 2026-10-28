# RetroAchievements client notes

Module: `crates/brisk-hostd/src/retroachievements.rs`. Read-only. Date of checks: 2026-10-28. Nothing was ever sent to the real API; all tests run against a local mock.

## Confirmed from the official API docs

- Auth: the Web API key goes in the `y` query parameter. Base `https://retroachievements.org/API/`, one `API_*.php` file per endpoint. https://api-docs.retroachievements.org/getting-started.md
- Game list: `API_GetGameList.php?y=KEY&i=<console id>&h=1&f=1`. `i` console id (required), `h=1` adds hashes, `f=1` only games with achievements. Returns a JSON array with `Title`, `ID`, `ConsoleID`, `ConsoleName`, `NumAchievements`, `Points`, `Hashes` (string array) and more. The docs strongly advise caching it. https://api-docs.retroachievements.org/v1/get-game-list.md
- Progress: `API_GetGameInfoAndUserProgress.php?y=KEY&u=<user>&g=<game id>`. Returns the game (`ID`, `Title`, `ConsoleID`, `ConsoleName`, `NumAchievements`, `NumAwardedToUser`, `NumAwardedToUserHardcore`) and `Achievements`, an object keyed by achievement id with `ID`, `Title`, `Description`, `Points`, `BadgeName`, `DisplayOrder`, and `DateEarned` / `DateEarnedHardcore` only when earned. https://api-docs.retroachievements.org/v1/get-game-info-and-user-progress.md
- Rate limiting exists, bursts are allowed, callers are asked to be reasonable and cache. No numbers are given. https://api-docs.retroachievements.org/

## Assumed (not in the official docs)

- Console ids: GameCube 16, Wii 19 (from memory; the docs page lists none). Verify with `API_GetConsoleIDs.php` once a key exists. Constants are `CONSOLE_GAMECUBE` / `CONSOLE_WII`.
- Status codes: nothing documents 401 or 429. The client treats 401 and 403 as "key refused" and 429 as "rate limited". An invalid key may in reality return 200 with an error body; that would show as "unexpected response".
- The hash format Dolphin reports for a disc matches the entries in `Hashes` (compared case-insensitively). Not checked.
- A Wii game list with hashes fits under the 8 MiB body cap of the shared `https_get`. Not measured.
- The `f=1` filter and `h=1` combine as documented. Not tried live.
- `Hashes` may be absent for a game; handled as empty.

## Behaviour

- Credentials: `~/.config/brisk/retroachievements`, line 1 username, line 2 Web API key. Plain characters only. Missing file means off (`RaError::Off`).
- Caches under `<data_dir>/ra/`: `hashes-<console>.json` (7 days), `game-<id>.json` (5 minutes). Written 0600 through a temp file and rename; a failed write is ignored and the data still returned.
- One request at a time spaced at least 1 s apart per process, no retries.
- The key is put into the URL only inside `get_json`; errors carry only a failure kind. A hash missing from a fresh list is not re-fetched until the list expires.
- `BRISK_RA_BASE` changes the host (https, or http to loopback). It needs adding to docs/env.md when this is wired in.
- Change outside the new files: `https_get` and `base_ok` in steam.rs became `pub(crate)`, plus one `pub mod` line in lib.rs.

## What Toby must do to turn it on

1. Create a RetroAchievements account. Copy the Web API key from the Keys section of the account control panel.
2. Write the file (mode 600): username on line 1, key on line 2, at `~/.config/brisk/retroachievements` on the host.
3. In Dolphin on the games VM: use a build that supports achievements (GameCube since 2407-68, Wii since 2603a per the earlier research), turn Dual Core off, and log in to RetroAchievements inside Dolphin. Dolphin reports the unlocks; brisk only reads them back.

## Stage 5 wiring

Add a `ra_config` to hostd's state from `RaConfig::from_env(&cfg.data_dir)`. For a running or selected Wii/GameCube game, compute the disc hash, call `progress_for(&ra, CONSOLE_WII, &hash)` from a handler, and map `RaError` to a short status line (hide the panel on `Off`). Poll on view, not on a timer.

## Title fallback

When a file's hash is not in the console's list (trimmed WBFS dumps, whose first 32 MB hash region is zeros), `progress_for_names` matches the library name against the same cached game list. Titles are lowercased, "(...)" groups and a trailing "disc N" dropped, punctuation removed. RA's "[Subset - X]" titles keep their brackets so a subset never equals the base game. Exactly one game id must match, otherwise the state stays `not-found`. The answer carries `matched: "title"` and the UI says the hash was not verified. A hash hit always wins. The list is cached (`ra/titles-<console>.json`, 7 days).
