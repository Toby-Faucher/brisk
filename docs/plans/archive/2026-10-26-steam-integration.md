# Steam integration (items 1-7) Implementation Plan

Done and merged. Archived; kept for history.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show more of Steam in Brisk: a "this fortnight" shelf, hero/logo art, a game detail panel (store info), achievements, an "updated since you played" badge, friends online, and screenshots and save info read from bawn.

**Architecture:** hostd gets one new module `steam_info.rs` for everything beyond `GetOwnedGames`. It reuses the existing key handling and `https_get` in `steam.rs` (pulled into one `steam_get`) and a small disk cache `data_dir/steam/<kind>-<appid>.json` with a TTL. Per-game endpoints (`/api/games/{appid}/…`) are fetched when the panel opens. Card badges (achievement bar, "Updated") are read from that cache only, and a background warm task fills the cache for installed games slowly, so `/api/games` never waits on Steam. The page adds one panel component and small additions to the existing Library and Home cards.

**Tech Stack:** Rust (axum, ureq + rustls-graviola, serde_json, tokio), Astro + Svelte 5 + Bits UI, Vitest, cargo nextest.

**Spec:** the numbered list agreed in chat on 2026-10-26 (items 1-7), on top of `docs/ui-design.md` (Steam key rules, line 107) and `docs/ui-notes.md` ("Owned games" paragraph).

## Global Constraints

- Pure Rust, no C/C++ (`scripts/check-pure-rust.sh`). HTTP goes through `https_get` only: https, 15 s total, 8 MiB cap, no redirects, no proxy.
- The API key is read per request from `cfg.steam_key_file`, goes only into the query string, and **never** appears in logs, errors, API responses, fixtures or docs. Errors reduce to `safe_err` kinds.
- Public repo: fixtures use fake names and ids (test key `TESTKEY-0123456789abcdef`, steam id `76561198000000000`). No real friends, no real ids, no home paths.
- npm: exact versions, no new packages planned. If one is needed, the newest version at least 14 days old.
- Every new `/api` route sits behind the existing `guard`. `<img src>` cannot send `x-brisk-secret`/`x-brisk-csrf`, so images served by hostd are fetched with `fetch` and shown from a blob URL.
- Unknown appids are refused with 404 via `AppState::is_known_appid` before any upstream call (keeps Steam quota and paths safe).
- Run builds as `. scripts/capped.sh; capped cargo ...`, with `~/.cargo/bin` on PATH. Root steps: none needed.

## Review Focus

1. Upstream failure (500, timeout, HTTP 403 from a private profile): the endpoint answers with empty data, the response body and logs contain no key, and the UI shows "unavailable", not a crash. Pinned in Task 1 and in every endpoint's own test.
2. A game with no achievements or no store page (`success:false`, error 400): `total: 0` / empty info, not an error. Pinned in Tasks 4 and 5.
3. A private friends list or private game details: `available: false`, no toast loop. Pinned in Task 7.
4. Screenshot name from the URL (`..%2f..`, absolute path, symlink): only names that appear in the directory listing and end in `.jpg`/`.png` are served. Pinned in Task 8.
5. Steam throttling the Store API (HTTP 429): the failure is cached for 10 minutes so reopening the panel does not hammer it. Pinned in Task 4.
6. Notes: a corrupt `notes.json`, a note pasted at 1 MB, or markup in a note must neither lose other notes, crash hostd, nor run as HTML. Pinned in Task 9.
7. Deleting a tag that games use: it disappears from every game and card, after a confirmation that says how many. Pinned in Task 9.

---

## File Structure

- Modify `crates/brisk-hostd/src/steam.rs`: `steam_get`, `Owned.playtime_2weeks`, new `Game` fields, cache reads in `load_games_with`.
- Create `crates/brisk-hostd/src/steam_info.rs`: `cached`, store info, achievements, news, friends, screenshots, warm.
- Modify `crates/brisk-hostd/src/{lib.rs,config.rs,http.rs}`: module, `steam_store_base`, routes, `AppState` fields (`friends`, `warming`).
- Create `crates/brisk-hostd/src/notes.rs` (notes and tags store, Task 9) and `web/src/state/notes.ts`.
- Create `crates/brisk-hostd/tests/common/mod.rs` (mock Steam, `steam()` temp setup, moved out of `tests/api.rs`); create `crates/brisk-hostd/tests/steam_info.rs`; fixtures in `tests/fixtures/steam/`.
- Modify `web/src/state/api.ts` (types, calls), create `web/src/modules/library/GameDetail.svelte`, modify `GameCard.svelte`, `Hero.svelte`, `Library.svelte`, `library.ts`, create `web/src/modules/home/FriendsCard.svelte`, modify `Home.svelte`.
- Docs: `docs/env.md`, `docs/ui-notes.md` (Owned games paragraph becomes "Steam data"), `docs/ui-design.md` only if its Steam section contradicts.

---

### Task 1: Shared Steam plumbing and test harness

**Files:**
- Modify: `crates/brisk-hostd/src/steam.rs` (`fetch_owned` at ~293), `config.rs`, `lib.rs`, `docs/env.md`
- Create: `crates/brisk-hostd/src/steam_info.rs`, `crates/brisk-hostd/tests/common/mod.rs`, `crates/brisk-hostd/tests/steam_info.rs`
- Modify: `crates/brisk-hostd/tests/api.rs` (use `mod common;`, delete the moved helpers)

**Interfaces:**
- Produces in `steam.rs`: `pub(crate) async fn steam_get(cfg: &Config, path_query: &str) -> Result<serde_json::Value, String>`. `path_query` is e.g. `"ISteamUser/GetPlayerSummaries/v2/?steamids=1,2"`; it appends `key=` and `format=json`, validates the base and the key exactly as `fetch_owned` does today, and returns the parsed JSON. `fetch_owned` becomes a caller of it.
- Produces in `steam.rs`: `pub(crate) async fn store_get(cfg: &Config, path_query: &str) -> Result<serde_json::Value, String>` (no key; base is `cfg.steam_store_base`).
- Produces in `steam.rs`: `pub(crate) fn steam_id(steam_dir) -> Option<String>` (already exists; make `pub(crate)`) and `pub(crate) fn account_id(steam_id64: &str) -> Option<u32>` (`id - 76561197960265728`).
- Produces in `steam_info.rs`: `pub(crate) async fn cached<T: Serialize + DeserializeOwned + Clone>(cfg: &Config, name: &str, ttl_s: u64, refresh: bool, fetch: impl Future<Output = Result<T, String>>) -> Option<T>`. Reads `data_dir/steam/<name>.json` (`{fetched, value}`); fresh and not `refresh` returns it; else runs `fetch`, stores, returns; on fetch error returns the stale value if any, else `None`. `name` must be `[a-z0-9-]+` (assert).
- Produces in config: `Config.steam_store_base: String`, env `BRISK_STEAM_STORE_BASE`, default `https://store.steampowered.com`.
- Produces in `tests/common/mod.rs`: `mock_steam(routes: Vec<(&'static str, u16, String)>) -> String` (first route whose path substring is in the request line wins; status and body from the tuple; `key=TESTKEY…` required for routes under `/I…/` or 403), `steam(base: &str, store_base: &str, key: Option<&str>) -> Steam`, `games_json`, `get_json(cfg, uri) -> (StatusCode, serde_json::Value)`.

- [ ] **Step 1: Move the test helpers.** Move `copy_dir`, `mock_steam`, `Steam`, `steam`, `games_json` from `tests/api.rs` into `tests/common/mod.rs` with the new signatures above; `api.rs` keeps its tests and gets `mod common; use common::*;` with its mock call rewritten as one `owned.json` route. Run `capped cargo nextest run -p brisk-hostd --test api`. Expected: all pass, same count as before.
- [ ] **Step 2: Failing tests** in `tests/steam_info.rs` (use `/api/games` as the vehicle until Task 2): `owned_still_works_through_steam_get` (the existing games assertion on `find(&v, 620)["name"] == "Portal 2"`), and in `src/steam_info.rs` unit tests `cached_returns_fresh_without_fetching`, `cached_refetches_when_stale_or_refresh`, `cached_returns_stale_on_fetch_error`, `cached_none_on_error_without_stale`, `cached_rejects_bad_name` (panics). Run: the unit tests fail to compile.
- [ ] **Step 3: Implement** `steam_get`, `store_get`, `account_id`, `cached`, the config field and env row in `docs/env.md`. `store_get` reuses `base_ok`. Move the key-file reading and validation out of `fetch_owned` unchanged.
- [ ] **Step 4: Failing test then pass** `key_never_in_errors`: mock returns 500 for a `steam_get` call; assert the `Err(String)` is `"HTTP 500"` and does not contain `TESTKEY`. Run `capped cargo nextest run -p brisk-hostd`. Expected: PASS.
- [ ] **Step 5: Commit** `refactor(hostd): one steam_get for Web API calls, cached() for per-game data`.

---

### Task 2: "Played this fortnight" shelf (item 1)

`GetOwnedGames` already returns `playtime_2weeks` (minutes, only when above 0) next to `playtime_forever`, so no extra call is needed. **Step 1 verifies that on the live response before building on it**; if the field is missing, switch `fetch_owned` to also call `IPlayerService/GetRecentlyPlayedGames/v1/?steamid=…` in the same refresh and merge by appid.

**Files:**
- Modify: `steam.rs` (`Owned`, `Game`, `load_games_with`), `tests/fixtures/steam/owned.json`, `web/src/state/api.ts` (`Game`), `library.ts`, `Library.svelte`, `GameCard.svelte`, `library.test.ts`, `tests/api.rs` or `steam_info.rs`

**Interfaces:**
- Consumes: Task 1 `steam_get` (through `fetch_owned`).
- Produces: `Owned.playtime_2weeks: u64` (`#[serde(default)]`); `Game.playtime2wk_min: u64` (JSON `playtime2wkMin`); TS `Game.playtime2wkMin: number`; `shelves()` returns an extra `fortnight: Game[]` (games with `playtime2wkMin > 0`, most first); `fmtFortnight(min: number): string` in `library.ts` (`"3.2 h this fortnight"`, `"25 min this fortnight"`).

- [ ] **Step 1: Verify the field live.** One-off, not committed: with the real key file, `curl` the GetOwnedGames URL the way `fetch_owned` builds it, but print only `jq '[.response.games[]|select(.playtime_2weeks)]|length'` (never print the URL or key). Expected: a number above 0 if anything was played in the last two weeks. If 0 and you know you played, use the fallback described above.
- [ ] **Step 2: Failing tests.** Fixture: add `"playtime_2weeks":192` to Portal 2 and `30` to Counter-Strike 2. Rust: `games_have_fortnight_minutes` asserts `find(&v,620)["playtime2wkMin"] == 192` and `find(&v,220)["playtime2wkMin"] == 0`. Vitest in `library.test.ts`: `shelves_fortnight_orders_most_first` (620 before 730) and `fmtFortnight` cases `192 -> "3.2 h this fortnight"`, `25 -> "25 min this fortnight"`. Run both; expect FAIL.
- [ ] **Step 3: Implement** the fields and `shelves().fortnight`; in `Library.svelte` render `<Shelf title="Played this fortnight" …>` above the installed shelf when `fortnight.length > 0` and no search and filter is `all`; `GameCard` sub line shows `fmtFortnight` instead of "Played … ago" when `playtime2wkMin > 0` and the card sits in that shelf (pass a `showFortnight` prop).
- [ ] **Step 4: Run** `capped cargo nextest run -p brisk-hostd` and `npm --prefix web run test` (use the script names in `web/package.json`). Expected: PASS.
- [ ] **Step 5: Commit** `feat: played-this-fortnight shelf`.

---

### Task 3: Hero and logo art (item 6)

**Files:**
- Modify: `steam.rs` (`Game`), `api.ts`, `Hero.svelte`, `tests/api.rs` (or `steam_info.rs`)

**Interfaces:**
- Produces: `Game.hero: String` = `https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/library_hero.jpg`, `Game.logo: String` = `…/logo.png`; both built next to `cover` in `load_games_with` (one `art(id, file)` closure replaces the `cover` closure). TS `Game.hero`, `Game.logo`. Hero art can 404 for newer games, so every use has a fallback.

- [ ] **Step 1: Failing test** `games_have_art_urls`: for appid 620 `hero` and `logo` equal the two URLs above. Expect FAIL.
- [ ] **Step 2: Implement** the fields. In `Hero.svelte` use `game.hero` as a background image layer under the existing hue wash, with an `<img>` probe (`onerror` drops back to the wash only). Logo shown over the hero when it loads, replacing the plain `h3` text visually only (keep the `h3` for screen readers and the split animation).
- [ ] **Step 3: Component test** in `GameCard.browser.test.ts` style or a Vitest unit if there is no Hero test: `hero falls back to the hue wash when the art fails` (dispatch `error` on the probe image, assert the background image is removed). Run the browser test script named in `web/scripts/README.md`.
- [ ] **Step 4: Commit** `feat: hero and logo art with fallback`.

---

### Task 4: Click a game to see its stats: detail panel (item 2)

Clicking any card in the Library opens the panel, installed or not. Play (or Install) moves into the panel, and the Hero card keeps its own Play button. This replaces today's one-click play and the not-installed "select, then Install" overlay.

**Files:**
- Modify: `steam_info.rs`, `http.rs` (route, handler), `api.ts`, `library.ts` (`cardClick`), `library.test.ts` (lines ~80-83), `GameCard.svelte`, `GameCard.browser.test.ts`, `Hero.svelte`, `Library.svelte`, `ui/Icon.svelte` (only if a needed icon is absent)
- Create: `web/src/modules/library/GameDetail.svelte`, `GameDetail.browser.test.ts`
- Test: `tests/steam_info.rs`, fixture `tests/fixtures/steam/appdetails-620.json` (trimmed real shape, fake text)

**Interfaces:**
- Consumes: Task 1 `store_get`, `cached`.
- Produces: `pub struct Info { available: bool, description: String, genres: Vec<String>, controller: Option<String> /* "full"|"partial" */, singleplayer: bool, coop: bool, remote_play_together: bool, release: Option<String>, metacritic: Option<u32>, screenshots: Vec<String> /* thumbnail URLs, max 6 */ }` (camelCase JSON). `pub async fn info(cfg: &Config, appid: u32, refresh: bool) -> Info` calls `appdetails?appids={id}&l=english`, TTL 7 days; `success:false` or a fetch error gives `Info { available: false, .. }` cached 10 minutes. Route `GET /api/games/{appid}/info` (404 unknown appid, `?refresh=1`). TS `GameInfo` and `api.gameInfo(appid)`.
- Changes `cardClick(g, selected)` in `library.ts` to `cardClick(g: Game): { action: 'open' }`; the `selected` state and the `.over` Install overlay in `GameCard.svelte` and `Library.svelte` are deleted, and the Install call moves into the panel.
- Produces in UI: `GameDetail.svelte` props `{ game: Game; busy: boolean; onclose: () => void; onplay: (g: Game) => void; oninstall: (g: Game) => void }`, a Bits UI dialog showing hero, logo, a primary button (Play / Resume / Install with size, reusing `playLabel` and `installLabel`) plus Quit when `game.running`, then description, genre chips, controller line ("Full controller support"), co-op / Remote Play Together chips, store screenshots, and later (Tasks 5, 6, 8) achievements, patch notes, local screenshots and saves. `Library.svelte` holds `open: Game | null`, so the panel stays on the same game while `games` refreshes underneath.
- `GameCard` keeps its hover pill but it now reads "Details" (the pill is not a separate button, so it cannot also play).

- [ ] **Step 1: Failing Rust tests** in `tests/steam_info.rs`: `info_distils_store_details` (genres `["Action","Puzzle"]`, `controller == "full"`, `coop == true`, `screenshots.len() <= 6`, no HTML in `description`: decode `&amp;`, `&quot;`, `&#39;` and strip tags), `info_unknown_appid_404`, `info_store_failure_is_unavailable_not_error` (mock 500: 200 with `available:false`, body has no `TESTKEY`), `info_failure_is_cached_ten_minutes` (second request does not reach the mock: count hits), `info_success_false_is_unavailable`. Run; expect FAIL.
- [ ] **Step 2: Implement** `Info`, `info()`, the handler (`is_known_appid`, then `info`), the route.
- [ ] **Step 3: Failing web tests.** `library.test.ts`: replace the four `cardClick` assertions with `cardClick(installed)` and `cardClick(notInstalled)` both `{ action: 'open' }`. `GameDetail.browser.test.ts`: `shows genres and controller line`, `shows "No store details" when available is false`, `Escape closes and returns focus to the card that opened it`, `Play calls onplay for an installed game`, `Install calls oninstall for a not-installed game`, `Quit shows only when running`. `GameCard.browser.test.ts`: `clicking an installed card calls onpick without starting the stream`. Mock `api.gameInfo`. Expect FAIL.
- [ ] **Step 4: Implement** the component and wiring. Info is fetched when the dialog opens, not for the grid. `pick` in `Library.svelte` sets `open`; `startGame`, `navigate('/stream')` and `api.install` run from the panel's callbacks with the existing `run()` busy handling. Run the web test scripts. Expected: PASS.
- [ ] **Step 5: Commit** `feat: click a game to open its detail panel`.

---

### Task 5: Achievements, background warm, card bar (item 3)

**Files:**
- Modify: `steam_info.rs`, `steam.rs` (`Game.ach_unlocked: Option<u32>`, `Game.ach_total: Option<u32>`, filled from the cache), `http.rs` (route, `AppState.warming: AtomicBool`, call `steam_info::warm` after `games`), `api.ts`, `GameDetail.svelte`, `GameCard.svelte`
- Test: `tests/steam_info.rs`, fixtures `playerachievements-620.json`, `schema-620.json`

**Interfaces:**
- Consumes: Task 1 `steam_get`, `cached`, `steam_id`; Task 4 panel.
- Produces: `pub struct Achievements { available: bool, unlocked: u32, total: u32, list: Vec<Ach> }`, `Ach { id: String, name: String, description: String, icon: String, unlocked_at: Option<u64> }` (hidden achievements keep their description empty). `pub async fn achievements(cfg, appid, refresh) -> Achievements` merges `ISteamUserStats/GetPlayerAchievements/v1/?steamid=…&appid=…&l=english` (unlocked flags and times) with `ISteamUserStats/GetSchemaForGame/v2/?appid=…&l=english` (names, descriptions, icons); TTL 1 h; a game without achievements or a private profile gives `available:false, total:0`. Route `GET /api/games/{appid}/achievements`. `pub fn ach_counts(cfg, appid) -> Option<(u32,u32)>` reads the cache file only, no network. `pub fn warm(state: Arc<AppState>, appids: Vec<u32>)` spawns one task unless `warming` is already set; for each appid (installed games with `playtimeMin > 0`, at most 60) it calls `achievements` then Task 6's `news`, with a 300 ms sleep between games and a 6 h TTL, and clears the flag at the end. TS `Game.achUnlocked/achTotal: number | null`, `api.gameAchievements(appid)`.

- [ ] **Step 1: Failing tests:** `achievements_merge_schema_and_player` (3 of 5 unlocked fixture: `unlocked == 3`, `total == 5`, first item has an `unlockedAt`), `achievements_none_is_unavailable` (mock answers 400 `{"playerstats":{"success":false,"error":"Requested app has no stats"}}`: 200, `total == 0`), `achievements_private_profile_is_unavailable` (mock 403), `warm_fills_cache_and_games_show_counts` (call `warm_once(cfg, vec![620]).await` — a non-spawning inner function `warm_games` that `warm` wraps — then `/api/games` has `achUnlocked == 3` for 620 and `null` for 730), `warm_runs_once_at_a_time` (set the flag, `warm` returns without spawning). Expect FAIL.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: UI tests:** `GameDetail` `shows 3 of 5 unlocked with a progress bar and the list`, `hides the section when total is 0`; `GameCard` `shows a thin progress bar when achTotal > 0` with `aria-label="3 of 5 achievements"` and none when null. Expect FAIL, then implement, then PASS.
- [ ] **Step 4: Commit** `feat: achievements in the detail panel, progress bar on cards, background cache warm`.

---

### Task 6: "Updated since you played" badge (item 4)

**Files:**
- Modify: `steam_info.rs`, `steam.rs` (`Game.updated: bool`), `api.ts`, `GameCard.svelte`, `GameDetail.svelte`
- Test: `tests/steam_info.rs`, fixture `news-620.json`

**Interfaces:**
- Consumes: Task 5 `warm` (already calls `news`).
- Produces: `pub struct News { items: Vec<NewsItem> }`, `NewsItem { title: String, url: String, date: u64 /* unix s */, patch: bool }`. `pub async fn news(cfg, appid, refresh) -> News` calls `ISteamNews/GetNewsForApp/v2/?appid=…&count=5&maxlength=0` (no key needed but the call goes through `steam_get` for the shared base; fine), TTL 6 h; `patch` is true for feed `steam_community_announcements` items. Route `GET /api/games/{appid}/news` for the panel. `Game.updated` = the newest `patch` item's `date` is later than `lastPlayed` and newer than 30 days, read from the cache only. Only installed games get the flag. TS `Game.updated: boolean`, `api.gameNews(appid)`. Card shows an "Updated" chip next to the saved-quality chip, panel lists the 5 headlines as links (`rel="noopener noreferrer"`, `target=_blank`; only `https:` URLs rendered as links).

- [ ] **Step 1: Failing tests:** `news_marks_updated_when_patch_after_last_played` (fixture patch at `1700001000`, Portal 2 `rtime_last_played` is `1700000500`: `updated == true`), `news_not_updated_when_played_after`, `news_old_patch_not_updated` (older than 30 days at the injected clock: pass `now` into the pure function `is_updated(news: &News, last_played: u64, now: u64) -> bool` and test it directly), `news_non_https_url_is_dropped`. Expect FAIL.
- [ ] **Step 2: Implement; UI test** `card shows Updated chip`, `panel links only https`. Run all checks for hostd and web. Expect PASS.
- [ ] **Step 3: Commit** `feat: updated-since-played badge and patch notes list`.

---

### Task 7: Friends online (item 5)

**Files:**
- Modify: `steam_info.rs`, `http.rs` (`AppState.friends: Mutex<Option<(Instant, Friends)>>`, route `GET /api/friends`), `api.ts`, `Home.svelte` (add to `FIXED`)
- Create: `web/src/modules/home/FriendsCard.svelte`, `FriendsCard.test.ts` (or browser test)
- Test: `tests/steam_info.rs`, fixtures `friends.json`, `summaries.json` (invented people)

**Interfaces:**
- Consumes: Task 1 `steam_get`, `steam_id`.
- Produces: `pub struct Friends { available: bool, online: Vec<Friend> }`, `Friend { name: String, avatar: String, state: String /* "online"|"away"|"busy"|"snooze" */, playing: Option<String> /* game name */, appid: Option<u32> }`. `pub async fn friends(cfg) -> Friends`: `ISteamUser/GetFriendList/v1/?steamid=…&relationship=friend`, then `GetPlayerSummaries/v2/?steamids=` in batches of 100; keep `personastate > 0` or in a game; sort playing first, then name; at most 30. Held **in memory only** for 120 s in `AppState.friends` (names are other people's data: never written to `data_dir`). A private list or any failure gives `available:false`. Avatar URLs are rendered only if `https://` and on `steamcdn-a.akamaihd.net`/`avatars.*.steamstatic.com`/`avatars.cloudflare.steamstatic.com`; else a letter placeholder.

- [ ] **Step 1: Failing tests:** `friends_online_sorted_playing_first`, `friends_offline_are_dropped`, `friends_private_list_is_unavailable` (mock 401), `friends_cached_in_memory_not_on_disk` (second call no mock hit; `data_dir` has no file containing a friend's name), `friends_batches_over_100` (150 invented ids → two summaries calls). Expect FAIL.
- [ ] **Step 2: Implement; UI tests:** card lists names with "playing X", shows "Friends list is private" when `available` is false, and nothing breaks when the list is empty ("No one online"). Polls via the existing `pollHost`-style interval (60 s, only while the page is visible). Expect PASS.
- [ ] **Step 3: Commit** `feat: friends online card`.

---

### Task 8: Screenshots and cloud saves from bawn (item 7)

**Files:**
- Modify: `steam_info.rs`, `http.rs` (two routes), `api.ts` (blob fetch helper), `GameDetail.svelte`
- Test: `tests/steam_info.rs` (temp `userdata` tree created in the test, no committed images)

**Interfaces:**
- Consumes: Task 1 `steam_id`, `account_id`; Task 4 panel.
- Produces: `pub struct Local { screenshots: Vec<String> /* file names, newest first, max 60 */, saves: Option<SaveInfo> }`, `SaveInfo { files: u32, bytes: u64, modified: u64 }`. `pub fn local(cfg, appid) -> Local` reads `steam_dir/userdata/<account_id>/760/remote/<appid>/screenshots/` (`.jpg`/`.png`, skipping `thumbnails/`) and summarises `steam_dir/userdata/<account_id>/<appid>/remote/` (file count, total bytes, newest mtime; no contents, no names). Routes: `GET /api/games/{appid}/local` and `GET /api/games/{appid}/screenshots/{name}`. The second serves a file only when `name` equals an entry returned by the listing (compare against the directory listing, never join the raw string) and ends in `.jpg`/`.png`, with `content-type` from that extension, `cache-control: private, max-age=3600`, `x-content-type-options: nosniff` (guard already adds). TS `api.gameLocal(appid)`, `api.screenshot(appid, name): Promise<string>` returning an object URL; the panel revokes URLs on close and loads thumbnails lazily (IntersectionObserver), at most 12 at a time.
- Saves are shown as a line only ("Cloud saves: 14 files, 2.1 MB, synced 3 days ago"). No download or restore: out of scope.

- [ ] **Step 1: Failing tests:** `local_lists_screenshots_newest_first_and_saves_summary`, `screenshot_serves_listed_file`, `screenshot_rejects_traversal` (`..%2F..%2Fconfig%2Floginusers.vdf`, `/etc/passwd`, `%00x.jpg`: all 404), `screenshot_rejects_symlink_out_of_dir` (symlink named `a.jpg` pointing at `loginusers.vdf`: 404 — check `symlink_metadata` is a regular file), `screenshot_wrong_extension_404`, `local_empty_when_no_userdata` (200, empty). Expect FAIL.
- [ ] **Step 2: Implement; UI test:** `screenshots load through fetch with the secret header, not img src` (assert `api.screenshot` called, `<img src>` starts with `blob:`) and `object URLs are revoked on close`. Expect PASS.
- [ ] **Step 3: Commit** `feat: screenshots and cloud-save summary from userdata`.

---

### Task 9: Notes and colored symbol tags (added after the first plan)

Two kinds of text, both yours: a **note on each game**, and **tags** you create (name, color, symbol, and their own note, e.g. a tag "Co-op night" with "Needs 4 controllers"). A game can carry any number of tags. Tags show on cards, filter the Library, and are edited in the detail panel. Stored on bawn only.

**Files:**
- Create: `crates/brisk-hostd/src/notes.rs`, `crates/brisk-hostd/tests/notes.rs`, `web/src/state/notes.ts`, `web/src/state/notes.test.ts`, `web/src/modules/library/TagChip.svelte`, `web/src/modules/library/TagEditor.svelte`, `web/src/modules/library/GameNotes.svelte`, `web/src/modules/library/GameNotes.browser.test.ts`
- Modify: `crates/brisk-hostd/src/{lib.rs,http.rs}` (`AppState.notes`, routes), `web/src/state/api.ts`, `GameDetail.svelte`, `GameCard.svelte`, `Library.svelte`, `library.ts`, `library.test.ts`

**Interfaces:**
- Produces (`notes.rs`, modelled on `prefs.rs`: `data_dir/notes.json`, whole file written to a 0600 temp file then renamed, tolerant load, one `Mutex`): `pub struct Tag { name: String, color: String, symbol: String, note: String }`, `pub struct GameNote { note: String, tags: Vec<String> }`, `pub struct Notes { tags: BTreeMap<String, Tag>, games: BTreeMap<u32, GameNote> }` (camelCase JSON). `Notes::load(path)`, `all() -> Notes`, `put_tag(id: &str, t: Tag) -> Result<Tag, NotesError>`, `delete_tag(id)` (also removes it from every game), `put_game(appid: u32, g: GameNote) -> Result<GameNote, NotesError>` (empty note and no tags deletes the entry). Limits (consts): `MAX_TAGS = 64`, `MAX_GAMES = 2000`, tag id `[a-z0-9-]{1,16}`, tag name 1..=24 chars after trim, tag note 500 chars, game note 4000 chars, 16 tags per game, `color` one of `gray red orange yellow green teal blue purple pink`, `symbol` `[a-z-]{1,16}` (the page owns the list of glyphs; an unknown name renders as a plain dot). Unknown tag ids in a game's list are dropped. `NotesError` maps to 400, a full store to 409.
- Routes (all behind `guard`): `GET /api/notes`, `PUT /api/notes/tags/{id}`, `DELETE /api/notes/tags/{id}`, `PUT /api/notes/games/{appid}` (body limit 64 KiB, `deny_unknown_fields`). Whole-document PUT is deliberately not offered, so two browsers editing different games do not overwrite each other.
- Produces (web): `notes` store (`{ tags, games }`), `refreshNotes()`, `watchNotes()` (same once-a-minute focus refresh as `watchPrefs`), `api.notes()`, `api.putTag(id, tag)`, `api.deleteTag(id)`, `api.putGameNote(appid, body)`, `TAG_COLORS: Record<TagColor, string>` (CSS color per name, readable on dark and light), `TAG_SYMBOLS: string[]` (about 16 names: `star heart flag check clock fire bookmark trophy skull gamepad users bolt moon gift question`) with an inline SVG per name in `TagChip.svelte`, `newTagId(): string`, `gameTags(map, appid): Tag[]`, `filterByTag(games: Game[], notes, tagId: string | null): Game[]` in `library.ts`.
- UI: `GameNotes.svelte` (in the detail panel) has a textarea that saves 800 ms after you stop typing and on blur or close, a "Saved" / "Not saved, retrying" status line, and the game's tag chips with a "+ Tag" picker (existing tags, or "New tag…" opening `TagEditor`). `TagEditor` edits name, color swatches, symbol grid and the tag's note, with Delete (asks first, says how many games use it). `GameCard` shows up to 3 tag symbols in their colors (`+n` for more) and a small note mark when a note exists; `title`/`Tip` shows the tag names. `Library.svelte` gets a tag filter row (chips of tags in use, one active at a time, combined with search and the existing filters). Notes are always rendered as text, never as HTML.

- [ ] **Step 1: Failing Rust tests** in `tests/notes.rs` (HTTP, temp `data_dir`) and unit tests in `notes.rs`: `put_tag_roundtrip_and_persists_across_load`, `tag_rejects_bad_color`, `tag_rejects_bad_id_and_empty_name`, `tag_note_over_500_chars_is_400`, `game_note_over_4000_chars_is_400`, `put_game_drops_unknown_tag_ids`, `empty_game_note_without_tags_deletes_entry`, `delete_tag_removes_it_from_games`, `store_full_is_409` (set limits through a small test constructor), `corrupt_file_loads_empty_and_next_save_works`, `file_is_0600`, `game_note_unicode_counts_chars_not_bytes` (4000 emoji accepted, 4001 refused), `notes_require_guard` (no secret: 403). Run; expect FAIL.
- [ ] **Step 2: Implement** `notes.rs`, `AppState.notes`, the four routes.
- [ ] **Step 3: Failing web tests:** `notes.test.ts` (`gameTags` skips ids not in `tags`; `filterByTag` null returns all; `newTagId` matches `/^[a-z0-9-]{1,16}$/`; save is debounced: three edits inside 800 ms send one PUT; a failed PUT keeps the draft and retries on the next edit or focus), `library.test.ts` (`filterByTag` composes with `searchGames`), `GameNotes.browser.test.ts` (typing saves once and shows "Saved"; failure shows "Not saved"; adding a tag chip calls `putGameNote` with its id; creating a tag calls `putTag` then `putGameNote`; deleting a used tag asks first and names the count; a note containing `<img src=x onerror=alert(1)>` renders as literal text). Expect FAIL.
- [ ] **Step 4: Implement** the store, components and wiring; run the web test scripts and `capped cargo nextest run -p brisk-hostd`. Expected: PASS.
- [ ] **Step 5: Commit** `feat: per-game notes and colored symbol tags`.

---

### Task 10: Smart tags, backlog, controller-friendly and almost-done (ideas 10, 1, 2, 3)

One mechanism covers four ideas: **smart tags** are read-only tags the page computes from data it already has. They sit in the tag filter row next to your own tags, with a dashed outline so they look different. The Backlog shelf (1), the controller filter (2) and the almost-done shelf (3) are three of them.

| Smart tag | Rule | Color, symbol |
|---|---|---|
| Backlog | owned, `playtimeMin < 30` | gray, `bookmark` |
| Controller | `controller == "full"` | blue, `gamepad` |
| Co-op | `coop` (hostd sets it from the store flags co-op or Remote Play Together) | green, `users` |
| Almost done | `achTotal > 0` and `achUnlocked / achTotal >= 0.8` and `< 1` | yellow, `trophy` |
| Updated | `updated` (Task 6) | orange, `bolt` |

**Files:**
- Modify: `steam.rs` (`Game.controller: Option<String>`, `Game.coop: bool`, both read from the info cache only), `steam_info.rs` (extend `warm_games`), `api.ts`, `library.ts`, `library.test.ts`, `Library.svelte`, `TagChip.svelte`, `GameCard.svelte`, `GameDetail.svelte`
- Test: `tests/steam_info.rs`

**Interfaces:**
- Consumes: Task 4 `info()` and its cache, Task 5 `warm_games`, `ach_counts`, Task 6 `updated`, Task 9 tag filter row and `TagChip`.
- Produces (Rust): `Game.controller` (`"full"|"partial"|null`) and `Game.coop` filled in `load_games_with` from `data_dir/steam/info-<appid>.json` without any network call. `warm_games` now also fetches `info` for **every** owned game (up to 200, 500 ms apart, TTL 7 days), after the achievements and news pass, so Controller and Co-op fill in over the first few minutes. Games not warmed yet simply do not match those tags (`null`).
- Produces (TS, `library.ts`): `SMART_TAGS: { id: 'auto-backlog' | 'auto-controller' | 'auto-coop' | 'auto-almost' | 'auto-updated'; name: string; color: TagColor; symbol: string; test: (g: Game) => boolean }[]`; `smartTags(g: Game): SmartTag[]`; `filterByTag` (Task 9) accepts an `auto-…` id and applies that rule; `pickRandom<T>(items: T[], rnd: () => number = Math.random): T | null`.
- UI: the tag filter row lists smart tags first (only the ones that match at least one game, with counts), then yours. Library shelves: "Backlog" (up to 40 games, least-played first) with a **Surprise me** button that picks `pickRandom` from the games the current filter shows and opens its panel; "Almost done" shelf (closest to 100% first). The card shows smart tag symbols after yours, still capped at 3 in total. The panel shows all of them.

- [ ] **Step 1: Failing Rust tests:** `games_carry_controller_and_coop_from_info_cache` (seed `info-620.json` with `controller: "full", coop: true`; `/api/games` shows them for 620 and `null`/`false` for 730), `games_without_info_cache_have_no_controller`, `warm_fetches_info_for_unplayed_games` (appid 220, playtime 0, gets an info cache file), `warm_caps_info_at_200`. Expect FAIL.
- [ ] **Step 2: Failing web tests** in `library.test.ts`: `backlog_is_under_30_minutes` (29 yes, 30 no, 0 yes), `controller_needs_full_support` (partial no), `almost_done_is_80_to_99_percent` (79 no, 80 yes, 100 no, total 0 no), `smartTags_orders_backlog_first`, `filterByTag_applies_auto_rule`, `pickRandom_empty_is_null`, `pickRandom_uses_rng` (`rnd = () => 0.99` picks the last). Browser test: `Surprise me opens the panel of a game that matches the active filter`. Expect FAIL.
- [ ] **Step 3: Implement** both sides. Run `capped cargo nextest run -p brisk-hostd` and the web scripts. Expected: PASS.
- [ ] **Step 4: Commit** `feat: smart tags, backlog shelf, controller and almost-done filters`.

---

### Task 11: Stream history and recommended quality per game (ideas 4, 5)

No new Steam or hostd code: `GET /api/sessions` already returns the last 100 sessions with `appid`, `preset`, `started`, `ended` and `stats` (`dropped`, `sent`, `bridgeP99`, `gate`).

**Files:**
- Create: `web/src/modules/library/streamHistory.ts`, `streamHistory.test.ts`, `StreamHistory.svelte`
- Modify: `GameDetail.svelte`, `GameDetail.browser.test.ts`, `web/src/state/api.ts` only if the sessions call needs a cache (it does not: fetched once when the panel opens)

**Interfaces:**
- Consumes: `api.sessions()`, `api.putPrefs`, `prefs` store, Task 4 panel, `fmtDuration`/`fmtAgo` from `home.ts`.
- Produces: `gameHistory(sessions: SessionRec[], appid: number): { count: number; totalMs: number; last: number | null; usualPreset: Preset | null }` (closed sessions only, `usualPreset` = most frequent, ties to the latest); `recommendPreset(sessions: SessionRec[], appid: number): Preset | null`: looks at the 5 newest closed sessions of the game; returns the preset one step cheaper (`crisp -> balanced -> fast`, never from `fast`, never `custom`) when at least 3 of them have `dropped / sent > 0.02` or `gate === 'fail'`, and it is the same preset as the newest session; otherwise `null`. It only ever suggests down, never up.
- UI: `StreamHistory.svelte` in the panel: "Streamed 12 times · 14 h · last 3 d ago · usually Crisp" (hidden when `count == 0`), and, when `recommendPreset` returns a preset, a line "Frames were dropped in 3 of the last 5 sessions at Crisp. Try Balanced?" with a **Use Balanced** button that calls `api.putPrefs(appid, { preset, codec })` keeping the saved codec (default `h264`), then updates the `prefs` store and shows a toast. It never changes anything without that click.

- [ ] **Step 1: Failing tests** in `streamHistory.test.ts`: `gameHistory counts only closed sessions of that game`, `gameHistory total and last`, `usualPreset ties go to the latest`, `recommend: 3 of 5 sessions with 3% drops at crisp -> balanced`, `recommend: 2 of 5 -> null`, `recommend: gate fail counts like drops`, `recommend: fast -> null`, `recommend: custom -> null`, `recommend: sent 0 or null stats never divide by zero`, `recommend: older sessions beyond the newest 5 are ignored`, `recommend: newest session on a different preset than the bad ones -> null`. Browser test: `shows the line and Use Balanced calls putPrefs with the saved codec`; `hides both when the game was never streamed`. Expect FAIL.
- [ ] **Step 2: Implement; run** the web scripts. Expected: PASS.
- [ ] **Step 3: Commit** `feat: per-game stream history and a quality suggestion`.

---

### Task 12: Update state on cards and in the panel (idea 7)

**Files:**
- Modify: `steam.rs` (`Installed`, `parse_acf`, `Game`), `tests/fixtures/steam/lib1/steamapps/` (one more manifest), `tests/api.rs` or `steam_info.rs`, `api.ts`, `GameCard.svelte`, `GameDetail.svelte`, `library.ts`, `library.test.ts`

**Interfaces:**
- Produces: `Game.update: Option<String>` (`"needed"` or `"running"`, null otherwise; JSON `update`), computed from the manifest `StateFlags` of installed games: **running** when any of `256` (UpdateRunning), `1024` (UpdateStarted) or `1048576` (Downloading) is set, else **needed** when `2` (UpdateRequired) is set. These bit values are from memory: **Step 1 checks them against real manifests on bawn** (`grep -h StateFlags ~/.local/share/Steam/steamapps/appmanifest_*.acf`, and once while a game is updating) and the constants get a comment with what was seen. `updateLabel(g: Game): string | null` in `library.ts`: `"Update needed"`, `"Updating…"`.
- UI: an orange chip on the card (`updateLabel`); in the panel a notice above the Play button: "Steam still has to update this game. Starting it now may wait for the download." Play stays enabled (Steam handles the wait). Distinct from Task 6's "Updated" (patch notes after you last played).

- [ ] **Step 1: Verify the bits live** as above and fix the constants if they differ.
- [ ] **Step 2: Failing tests:** `parse_acf_update_flags` (`4 -> None`, `4|2 -> needed`, `4|256 -> running`, `4|2|256 -> running`, `1048576|4 -> running`), `games_show_update_state` (new manifest fixture with `StateFlags 6` for an owned appid: `"needed"`), `not_installed_game_has_no_update_state`; web `updateLabel` cases. Expect FAIL.
- [ ] **Step 3: Implement; run** hostd and web tests. Expected: PASS.
- [ ] **Step 4: Commit** `feat: show Steam update state on cards and in the panel`.

---

### Task 13: Pinned tag cards on Home (idea 9)

**Files:**
- Modify: `notes.rs` (`Tag.pinned: bool`, `#[serde(default)]`), `tests/notes.rs`, `api.ts` (`Tag`), `TagEditor.svelte` (a "Show on Home" switch), `Home.svelte`, `Library.svelte` (read `?tag=` on load), `home.ts`, `home.test.ts`
- Create: `web/src/modules/home/TagCard.svelte`, `TagCard.test.ts`

**Interfaces:**
- Consumes: Task 9 `notes` store and tags, Task 10 smart tag ids, Home's `FIXED` registry and `reconcileOrder`.
- Produces: `Tag.pinned` (bool; a pinned tag becomes a Home card with id `tag-<tagId>`, 20 characters at most, which `clean_home_order` accepts). `tagCardIds(tags: Record<string, Tag>): string[]` in `home.ts` (pinned only, sorted by name) — `Home.svelte` appends these to the ids it passes to `reconcileOrder`, so they are draggable and their position is saved like any card; a card for a deleted or unpinned tag drops out because `reconcileOrder` drops unknown ids. `TagCard.svelte` props `{ tagId: string }`: the tag's symbol and name, its note (shortened to 3 lines), up to 8 cover thumbnails of its games (installed first), a count, and click goes to `/library?tag=<id>`. Pinned smart tags are not offered in this task (they are not stored).

- [ ] **Step 1: Failing tests:** Rust `tag_pinned_roundtrips_and_defaults_false` (an old `notes.json` without the field loads); web `tagCardIds only pinned and sorted`, `tagCardIds ids pass clean_home_order rules (a-z0-9- , max 32)`, `TagCard shows count, note and at most 8 covers`, `TagCard empty tag says No games yet`, `Library opens with the tag filter from ?tag=`, `Library ignores an unknown ?tag=`. Expect FAIL.
- [ ] **Step 2: Implement; run** the Rust and web tests. Expected: PASS.
- [ ] **Step 3: Commit** `feat: pin a tag as a Home card`.

---

### Task 14: Docs and full check

**Files:** `docs/ui-notes.md` (also a line each for smart tags, the quality suggestion rule and the update-state bits; the "Owned games" paragraph becomes "Steam data": endpoints, TTLs, the warm task, the in-memory-only friends rule, the blob-URL rule; plus a "Notes and tags" paragraph: `notes.json` format, limits, routes), `docs/env.md` (already has `BRISK_STEAM_STORE_BASE` from Task 1), `docs/ui-design.md` only if it says the Library has no detail view.

- [ ] **Step 1: Update docs.** List the new routes with their TTLs in one table.
- [ ] **Step 2: Run** `. scripts/capped.sh; capped scripts/check.sh`. Expected: every stage passes (fmt, clippy `-D warnings`, nextest, doc tests, `cargo deny`, pure-Rust, embed runs, `scripts/test-*.sh`, `npm --prefix web run ci`).
- [ ] **Step 3: Live smoke on bawn (Toby, user-level, no root):** `scripts/brisk-service.sh update`, open the Library: fortnight shelf, a detail panel on an installed game, the achievements bar appearing a minute after load, the friends card, and the journal (`scripts/brisk-service.sh logs`) shows no `key=`.
- [ ] **Step 4: Commit** `docs: steam data endpoints`.

---

## Self-review notes

- Spec coverage: item 1 → Task 2, 2 → 4, 3 → 5, 4 → 6, 5 → 7, 6 → 3, 7 → 8; Task 1 and 14 are plumbing and docs; Task 9 is the notes and tags request; Tasks 10 to 13 are the later ideas 10+1+2+3, 4+5, 7 and 9.
- Names used across tasks: `steam_get`, `store_get`, `cached`, `steam_id`, `account_id`, `warm`, `Game.achUnlocked/achTotal/updated/playtime2wkMin/hero/logo` are defined once in Tasks 1 to 6 and only consumed after.
- Skipped on purpose: global achievement rarity, wishlist, `localconfig.vdf` collections (item 8), installing through the API.
