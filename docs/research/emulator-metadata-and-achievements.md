# Emulator games: metadata, art and achievements

Date of findings: 2026-10-28. My sources may be dated. Anything I could not confirm from a primary page is marked **unverified**. Several sites (GameTDB, IGDB docs, the RetroAchievements game pages, the Dolphin blog) returned HTTP 403 to my fetch tool, so those claims lean on search snippets or third-party pages and are marked.

## Summary

**Recommendation**

1. Art and text: look a game up once, by disc ID (GameCube/Wii) or ROM hash (retro), cache the result on the host, never refetch.
2. Wii/GameCube covers and titles come from GameTDB by disc ID. Hero, logo and grid art comes from SteamGridDB by name search. IGDB (or ScreenScraper for retro) fills descriptions.
3. A weak match (name search, or no ID hit) goes into a "confirm this game" queue for Toby. Exact ID or hash matches are accepted without asking.
4. Achievements, first version: read-only. Poll the RetroAchievements Web API for the user's progress per game. Show unlocked and locked achievements and a percent bar.
5. Do not try to hook live unlocks. Dolphin already reports unlocks to RetroAchievements itself. Brisk only needs to poll afterwards.

**The 5 facts that matter most**

1. Dolphin (standalone) has official RetroAchievements support: GameCube since 2407-68, Wii since 2603a / 2603-81. "Enable Dual Core" must be off. Source: https://docs.retroachievements.org/general/emulator-support-and-issues.html
2. A Mario Kart Wii set exists on RetroAchievements (game ID 38107, search snippets say 202 achievements and 1,401 points, plus Multiplayer and Bonus subsets). I could not open the page, so this is **unverified**. Source: https://retroachievements.org/game/38107
3. The RetroAchievements Web API is key-based (`y` query parameter, the user's own key), has rate limiting with burst headroom, and exposes profile, recent unlocks, and per-game progress with `DateEarned`. Source: https://api-docs.retroachievements.org/
4. The Web API cannot look up a game by hash directly. You fetch the hash list per system (`API_GetGameList.php?i=<system>&h=1`) and match locally. Source: https://api-docs.retroachievements.org/v1/get-game-list.md
5. Disc ID of a GameCube/Wii image is the first 6 bytes of the file (plain ISO). RVZ stores the first 0x80 bytes of the disc inside its own header. Sources in section C.

**Needs Toby**

- A RetroAchievements account, and its username plus Web API key (from the account settings page). Hardcore mode is required for Wii sets per a news report (unverified against official docs).
- Confirm Dolphin version on the games VM is at least 2603a and that Dual Core is off, then log in to RetroAchievements inside Dolphin (Tools, Achievements window per a news report, unverified).
- A free SteamGridDB account and API key.
- A Twitch account with two-factor auth to get IGDB client credentials (only if he wants descriptions for non-Steam games from IGDB).
- Decision: is IGDB OK, or should descriptions for Wii/GameCube come only from GameTDB (which has no long description, see B)?
- Decision: for retro systems later, create a ScreenScraper account? Its API needs developer credentials from the ScreenScraper forum.
- Decision: is the confirm queue a card in the hub UI or a notification?

## A. RetroAchievements

### Which systems and emulators

- Dolphin standalone is listed as a supported emulator for GameCube and Wii. Notes: GameCube needs "version 2407-68 or newer", Wii needs "version 2603a (stable)/2603-81 (nightly) or newer", and "Enable Dual Core (speedup)" must be unchecked. Source: https://docs.retroachievements.org/general/emulator-support-and-issues.html
- The libretro Dolphin and Ishiiruka cores are marked "Deprecated or confirmed to be problematic. Unusable for achievements." for GameCube, and Dolphin libretro the same for Wii. So RetroArch is not the route for GameCube/Wii. Source: https://docs.retroachievements.org/developer-docs/unsupported-emulators-and-cores.html
- Same page lists other standalone emulators: DuckStation (PlayStation), PCSX2 (PS2), PPSSPP, Flycast, RAVBA (GB/GBA), RASnes9x (SNES), RANes (NES), RAProject64 (N64), and more. RetroArch with supported cores covers many retro systems; the FAQ says console support is "dictated by the emulator/core developer". Source: https://docs.retroachievements.org/general/faq.html
- Wii launched on RetroAchievements around March 2026 with an event running March 19 to July 19, 2026, per a news report. Needs Dolphin 2603 or newer. Source: https://retrohandhelds.gg/retroachievements-finally-adds-wii-support-with-four-months-of-waggle-grinding/ (third party). The same report says Hardcore Mode is required for Wii. Unverified against official docs.
- GameCube support launched July 2024 with Dolphin, reported by third parties (https://www.timeextension.com/news/2024/07/gamecube-gets-achievements-thanks-to-the-dolphin-emulator-and-retroachievements). Not checked against the Dolphin blog (403).
- Mario Kart Wii: search results show a game page at https://retroachievements.org/game/38107 with a base set, a Multiplayer subset and a Bonus subset, and user progress pages showing 202 achievements. One snapshot said "no achievements yet". **Unverified.** Brisk can check at runtime with `API_GetGameExtended` or `API_GetGameInfoAndUserProgress` (`NumAchievements`).

### How unlocks happen

Achievements are evaluated inside the emulator by RetroAchievements' rcheevos library, which reads emulated memory while the game runs. The emulator then reports each unlock to the RetroAchievements server. The FAQ says you must be online at session start, and "your achievements will sync" if you lose connection and reconnect. Source: https://docs.retroachievements.org/general/faq.html

Brisk cannot evaluate achievements itself without reading emulator memory. It does not need to, because Dolphin does it (see above) and the server holds the result.

### Web API

- Base: `https://retroachievements.org/API/API_<Name>.php`. Docs index: https://api-docs.retroachievements.org/ (an `llms.txt` lists every endpoint).
- Auth: "Provide your API key to the `y` query param." The key is found on the account's control panel. Source: https://api-docs.retroachievements.org/
- Rate limit: enabled, "a fair amount of burst requests". The docs ask you to be reasonable and to cache or preload static game data. A higher limit can be requested in the `#coders` channel of their Discord. No numbers are published. Same source.
- Terms: the docs page has no formal terms section. It says you are responsible for your own API use. Whether a third-party app may share a key between users is not stated, so brisk should use Toby's own key for Toby's own account only. This is **unverified**; check the RetroAchievements site terms before sharing the app.
- Endpoints useful to brisk (all from https://api-docs.retroachievements.org/llms.txt):
  - User Profile, User Summary, User Recently Played Games
  - User Unlocks (most recent): `API_GetUserRecentAchievements.php?u=<user>&m=<minutes>`; default window is the last 60 minutes. Fields include `Date`, `HardcoreMode`, `AchievementID`, `Title`, `Description`, `BadgeName`, `Points`, `GameTitle`, `GameID`. Source: https://api-docs.retroachievements.org/v1/get-user-recent-achievements.md
  - Game Info and User Progress: `API_GetGameInfoAndUserProgress.php?u=<user>&g=<gameId>`. Returns game fields (`Title`, `ConsoleName`, `Genre`, `Developer`, `Publisher`, `Released`, `ImageIcon`, `ImageTitle`, `ImageIngame`, `ImageBoxArt`), every achievement with `Title`, `Description`, `Points`, `BadgeName`, and `DateEarned` / `DateEarnedHardcore` only when the user has it, plus `NumAwardedToUser`, `UserCompletion`, `UserTotalPlaytime`. Source: https://api-docs.retroachievements.org/v1/get-game-info-and-user-progress.md
  - Game list with hashes: `API_GetGameList.php?i=<systemId>&h=1&f=1` (system ID, include hashes, only games with achievements). Source: https://api-docs.retroachievements.org/v1/get-game-list.md
  - Game hashes: `API_GetGameHashes.php?i=<gameId>`; takes a game ID only, no lookup by hash. Source: https://api-docs.retroachievements.org/v1/get-game-hashes.md
  - Get All Systems (console IDs).
- Achievement and game images are relative paths (for example `/Images/026368.png`). The host to prefix is not stated on that page; I believe it is `https://media.retroachievements.org`, **unverified**.

### Hashing (game identification)

From https://docs.retroachievements.org/developer-docs/game-identification.html:

- NES: custom. If the file starts with `NES\x1a`, skip the first 16 bytes and hash the rest. Otherwise hash the whole file.
- SNES: MD5. If the size is 512 bytes more than a multiple of 8 KB, skip the first 512 bytes.
- Genesis / Mega Drive: MD5 of the file.
- Game Boy Advance: MD5 of the file.
- Nintendo 64: MD5 of the big-endian `.z64` form. `.v64` and `.n64` are converted first.
- PlayStation: custom. Parse `SYSTEM.CNF`, take the `BOOT=` executable name and path, then hash name plus executable contents.
- GameCube: custom. Load the Apploader, then each DOL segment it references, and hash the combined buffer.
- Wii: that page does not cover it. **Unverified**, so for Wii brisk should not compute a hash. Match by game title or ask Dolphin/RetroAchievements (see below).

Practical consequence: for Wii/GameCube, hashing needs disc parsing and is fragile. Easier first version: take the game name from GameTDB, search the RetroAchievements game list for the Wii system (`API_GetGameList.php?i=<wii id>`), and confirm the match with Toby once. Store the RetroAchievements game ID in brisk's per-game cache.

### What is realistic

1. Read-only progress by polling. Fully possible. One call to `API_GetGameInfoAndUserProgress` per game gives the list and what is unlocked. Cache the static parts (titles, descriptions, badges) and re-poll only the progress. Poll when the game page opens and once when a session ends, not on a timer.
2. Live unlock detection. Not possible from outside the emulator in real time. The best you can do is poll `API_GetUserRecentAchievements` (default window 60 minutes) every 30 to 60 seconds during a session and diff by `AchievementID`. Staleness is the poll interval plus whatever delay the server has. The docs do not state server-side delay or caching, so that is **unverified**. Toby's worry is half right: brisk cannot inject or see an unlock itself, but it can see the server's record soon after.
3. Manual path. A small checklist stored in brisk per game that Toby ticks by hand. Cheap, but it duplicates what the API gives for free for any game that has a set. Only useful for games with no RetroAchievements set.

### Smallest useful first version

- Config: RetroAchievements username and Web API key, stored on the host as a secret and never committed.
- For each emulator game with a known RetroAchievements game ID: show `NumAwardedToUser` of `NumAchievements`, the badge grid with locked ones dimmed, and the latest unlock date.
- Refresh on game page open and after a session ends. No live feed.
- Games without a set show nothing, not an empty box.

## B. Art and descriptions

| Source | Needs from Toby | Cost | Match | Gives |
|---|---|---|---|---|
| GameTDB | nothing | free | disc ID | titles, covers, discs, boxes (Wii, GameCube) |
| SteamGridDB | account, API key | free | name search or Steam ID | grid, hero, logo, icon |
| IGDB | Twitch account and client credentials | free, non-commercial | name search | descriptions, cover, artwork |
| ScreenScraper | account, plus developer credentials | free, with limits | hash, ROM name, ID | descriptions, art, per region (retro) |
| TheGamesDB | API key request | free | name | descriptions, art |

### GameTDB

- Covers live at `https://art.gametdb.com/`, looked up with the 6-character game ID plus region code for GameCube and Wii. Evidence: the rom-properties project documents this use. Source: https://www.github.com/GerbilSoft/rom-properties/blob/master/NETWORK.md
- The FAQ (search snippet only, page returned 403) says covers can be downloaded with tools on the Downloads page, asks that the XML file never be edited, and asks projects using its data to link back to GameTDB. Source: https://gametdb.com/Main/FAQ
- Title database `wiitdb.xml` download exists (Downloads page). I could not open it, so the exact URL, license and fields are **unverified**.
- Caching images locally and hotlinking rules are **unverified**. rom-properties caches downloaded images locally, which suggests caching is accepted practice, not a policy statement. Safest plan: download once per game, cache, link back to GameTDB in the UI credits.
- No long descriptions. It has titles, region, languages, genre/developer/publisher fields (believed, unverified).

### SteamGridDB

- API v2 base `https://www.steamgriddb.com/api/v2`. Auth: `Authorization: Bearer <key>`. Endpoints: `/search/autocomplete/{query}`, `/grids/{game|steam}/{id}`, `/heroes/...`, `/logos/...`, `/icons/...`, `/games/{id|steam}/{id}`. Source: https://raw.githubusercontent.com/SteamGridDB/node-steamgriddb/master/src/index.ts
- Key is generated on the user's preferences page. Source: https://github.com/SteamGridDB/node-steamgriddb/blob/master/README.md
- Grid style and dimension filters exist (same README).
- Search by name returns a SteamGridDB game ID plus `verified` flag. Same README.
- Rate limits and terms: **unverified** (their API docs page returned only a title to my tool). Image license: assets are community uploads; check their terms before redistributing. Local caching for personal use is the conservative case.
- Fits well because brisk already uses Steam-style art (grid, hero, logo), so emulator games look the same.

### IGDB

- Free for non-commercial use under the Twitch Developer Service Agreement. Credentials via a Twitch account with two-factor auth: Client ID and Client Secret, then a client-credentials token. Rate limit 4 requests per second (HTTP 429 over). These come from a search summary; the docs page (https://api-docs.igdb.com/) returned 403. **Unverified against the primary page.**
- Endpoints include games, covers, artworks, and a search. Match is by name (and platform filter). Attribution requirement: **unverified**.
- Only worth adding if GameTDB plus SteamGridDB leave descriptions empty.

### ScreenScraper

- Endpoint `jeuInfos.php` takes `romnom`, `crc`, `md5`, `sha1`, `romtaille`, `systemeid`, plus developer identity (`devid`, `devpassword`, `softname`) and user credentials (`ssid`, `sspassword`). Source: https://api.screenscraper.fr/webapi2.php
- Developer credentials must be requested on their forum. The API "may only be integrated into fully free, distributed applications"; other uses need prior authorization. Quotas are per minute and per day by user level, threads are earned by contributing or donating, and error 430 means the daily quota is exceeded. The API v2 is in beta. Same source.
- Good for retro by hash (RetroArch ROM sets), poor for first release because of the credential process. Defer.

### TheGamesDB

- Has an API key request page and a usage-limit increase page; no terms stated on the home page. Source: https://thegamesdb.net/
- Whether it covers Wii/GameCube art and how it matches is **unverified**. Skip for now.

### Recommended minimal stack

1. Identify the game by disc ID (Wii/GameCube) or ROM hash (retro). See C.
2. Wii/GameCube: GameTDB for canonical title and cover by ID.
3. SteamGridDB for grid, hero, logo, icon. Search by the GameTDB title, take the top verified hit.
4. Description: the RetroAchievements game info (developer, publisher, genre, release date) is free once the key exists. A prose description needs IGDB, so add it only if Toby wants it.
5. Cache everything under the brisk data directory, keyed by disc ID or hash. Never refetch unless Toby presses a refresh button.
6. Weak match handling: exact disc ID or hash = accept. Anything found only by name, or with more than one close candidate, goes in a queue showing up to three candidate cards (art, title, year). Toby picks one or "none". Save the pick so it is not asked again. Never fetch art for an unconfirmed game, so wrong art never shows.

## C. Identifying a game file

### GameCube and Wii discs

Header layout, from https://wiibrew.org/wiki/Wii_disc:

- 0x000, 1 byte: disc ID character
- 0x001, 2 bytes: game code
- 0x003, 1 byte: region code
- 0x004, 2 bytes: maker code
- 0x006: disc number
- 0x007: disc version
- 0x018, 4 bytes: Wii magic word `0x5D1C9EA3` (zero on GameCube discs)
- 0x01C, 4 bytes: GameCube magic word `0xC2339F3D` (the reverse)
- 0x020: game title (64 bytes)

The first 6 bytes (0x000 to 0x005) make the game ID, for example `RMCE01` for Mario Kart Wii (USA). The magic words tell Wii from GameCube.

Containers:

- Plain ISO / GCM: the header is at file offset 0.
- RVZ and WIA: the format keeps "the first 0x80 bytes of the disc image" in the field `dhead`. By my count from the layout, that is at file offset 0x58 (calculated, not stated). WIA files start with `WIA\x01`. The RVZ magic is not stated in that document (it says RVZ is closely based on WIA), so RVZ detection is **unverified**; I believe it is `RVZ\x01`. Source: https://raw.githubusercontent.com/dolphin-emu/dolphin/master/docs/WiaAndRvz.md
- WBFS: not checked. **Unverified.**
- Best option: read the header with a library or with `dolphin-tool`, which ships with Dolphin, instead of parsing it by hand. The `dolphin-tool` header command is **unverified**.

A rename of the file does not change the ID, so brisk need not trust the file name.

### Cartridge ROMs

Use the same hash that RetroAchievements uses for the system (list in section A). That gives a direct match against `API_GetGameList.php?...&h=1` and keeps one hash method per system. For other databases (ScreenScraper takes `crc`, `md5`, `sha1` and file size), compute a plain MD5 or SHA-1 of the whole file as well. Beware headers: NES and SNES copiers add headers, and N64 files come in three byte orders. The RetroAchievements rules above handle both.

## What I could not verify

- Mario Kart Wii set contents (page 403; only search snippets).
- Dolphin blog post about the integration (403), Dolphin menu path for logging in, hardcore requirement for Wii.
- Wii hash method on RetroAchievements and the console ID for Wii.
- RetroAchievements API terms for third-party apps, exact rate numbers, server delay for unlocks, and the media host for images.
- GameTDB terms, `wiitdb.xml` URL and license, hotlinking rules.
- SteamGridDB rate limits and terms.
- IGDB docs (403): rate limit, attribution, commercial rules.
- RVZ magic bytes, WBFS layout, `dolphin-tool` usage.
- TheGamesDB coverage and terms.
