# GameTDB notes for the Wii and GameCube lookup

Date of findings: 2026-10-28. `crates/brisk-hostd/src/gametdb.rs` implements the lookup. The GameTDB site itself
(gametdb.com, www.gametdb.com, its Downloads page, FAQ and game pages) answered HTTP 403 to my fetch tool, so
nearly everything below comes from search snippets or third-party pages. Nothing was fetched from the real servers
by the tests or the code.

## Confirmed (from a search snippet or a third-party page, not from the site)

- The Downloads page for the Wii and GameCube database lists `wiitdb.zip`: https://www.gametdb.com/Wii/Downloads
- The FAQ says changes can take up to an hour to show up in `wiitdb.zip`: https://gametdb.com/Main/FAQ
- The FAQ says never to edit the XML file, and asks projects that use the data to link back (snippet only).
- `art.gametdb.com` hosts box, cover and media scans for GameCube, Wii, Wii U, DS and 3DS. Layout is
  `sys/media/region/gameID.ext`, for example `https://art.gametdb.com/3ds/cart/US/AREE.png`.
  Source: https://www.github.com/GerbilSoft/rom-properties/blob/master/NETWORK.md
- A Wii disc image is cached by rom-properties as `wii/disc/US/RSBE01.png`, so `wii/<media>/<region>/<id>.png`
  is a real path. Same source. GameCube and Wii use the 6-character id plus a region code.
- Anecdote (GBAtemp, DS only): a region folder can lack a file that the `EN` folder has. We fall back to `EN`.
- The only legal page found is the privacy policy: https://www.gametdb.com/Main/Legal

## Assumed (UNVERIFIED, marked with `shortcut:` in the code)

- Direct URL of the zip and of a plain `wiitdb.xml`. No default is built in, so no URL is invented.
- The XML shape: `<game name=".."><id/><region/><languages>EN,FR</languages><locale lang="EN"><title/></locale>
  <developer/><publisher/><date year=".." month=".." day=".."/></game>`. This is from memory of the format.
  The scanner is tolerant: missing parts become empty values.
- Real size of `wiitdb.xml`. The download cap is 64 MiB and the cache is a JSON index, so a bigger file is refused.
- Cover folder names. Only `cover` and `disc` are seen in a source. `cover3D`, `coverfull`, `discM` and `box`
  are guesses, and the extension is assumed to be `.png` for all of them.
- GameCube art lives under `/wii/` too.
- The 4th id character to region folder table (E=US, J=JA, K=KO, W=TW, D=DE, F=FR, S=ES, I=IT, H=NL, U=AU, R=RU,
  anything else EN). The code tries the mapped folder, then `EN`, on a 404.
- Update cadence of the database: not stated anywhere I could read. The cache lives 7 days.
- Terms on caching and hotlinking: no statement found. We download each image once and keep it, which is
  what rom-properties does. Hotlinking from the UI is avoided on purpose.

## Terms we follow

- Never edit the database text. The cache stores parsed fields only, the downloaded file is not modified.
- Show a credit link to GameTDB (gametdb.com) wherever a title or cover from it is shown.
- One database fetch per 7 days at most, one image fetch per image, errors do not retry in a loop.

## Decision for Toby: the zip

The site offers `wiitdb.zip`. Unpacking it needs deflate, and no pure-Rust inflate is in `Cargo.lock`
(and the rule is no new dependencies in this experiment). Options:

1. Add a small pure-Rust inflate crate (for example `miniz_oxide`), pinned, after `cargo deny` and the
   pure-Rust check pass. Then the default source can be the zip.
2. Keep plain `wiitdb.xml` only. Toby (or a script) unpacks the zip once and the host serves or points
   `BRISK_GAMETDB_XML_URL` at the XML. A `.zip` URL gives `Error::ZipUnsupported`.
3. Write a small inflate by hand (about 150 lines). Not recommended for a first version.

Also needed: once the site is reachable from a browser, confirm the UNVERIFIED items above, and check
the real file size against the 64 MiB cap.

## Wiring for Stage 3

1. The disc id comes from `discid.rs` (other branch).
2. On first sight of a game, call `gametdb::lookup(cfg, id)` on a blocking thread. `NotFound` or any error
   means "no card art yet", not a failure. Weak or missing matches go to the confirm queue.
3. Store the entry's title, year and publisher in the per-game cache. The art download code (`fetch_art`, region
   folders, `BRISK_GAMETDB_ART_BASE`) was removed unused in the round 3 cleanup; it is in git history
   (before the commit "hostd: drop the unused GameTDB art download").
4. Add the GameTDB credit link to the UI credits.
5. `docs/env.md` lists `BRISK_GAMETDB_XML_URL`.
