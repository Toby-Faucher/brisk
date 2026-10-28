# SteamGridDB notes

Module: `crates/brisk-hostd/src/steamgriddb.rs`. Checked 2026-10-28. The official docs page
(https://www.steamgriddb.com/api/v2) renders client-side and returned only its title, and
`/api/v2/docs` is a 404. Everything below comes from the official wrapper source and a docs
mirror, so "confirmed" means "two or more consistent secondary sources", not "read on the official page".

## Confirmed (wrapper source and docs mirror agree)

- Base URL `https://www.steamgriddb.com/api/v2`. Source: https://raw.githubusercontent.com/SteamGridDB/node-steamgriddb/master/src/index.ts
- Auth: `Authorization: Bearer <key>` (same file). The key comes from the account's preferences page (https://github.com/SteamGridDB/node-steamgriddb/blob/master/README.md).
- Endpoints: `GET search/autocomplete/{term}`, `grids/game/{id}`, `heroes/game/{id}`, `logos/game/{id}`, `icons/game/{id}`; also `/steam/{appid}` variants and `games/id/{id}`. Mirror: https://context7.com/websites/steamgriddb_api_v2
- Filters, comma-joined: `styles`, `dimensions`, `mimes`, `types` (`static`/`animated`); single: `nsfw`, `humor`, `epilepsy` (`true`/`false`/`any`), `oneoftag`, `page` (from 0), `limit` (max 50).
- Grid dimensions: 460x215, 920x430, 600x900, 342x482, 660x930, 512x512, 1024x1024. Hero: 1920x620, 3840x1240, 1600x650. Logos have no dimensions filter. Mimes: png, jpeg, webp (logos: png, webp; icons: png, x-icon).
- Envelope: `success`, `page`, `total`, `limit`, `data`. Game: `id`, `name`, `types`, `verified`, `release_date` (unix seconds, from the node typings). Image: `id`, `score`, `style`, `width`, `height`, `url`, `thumb`, `tags`, `author`, `language`, `notes`.
- Errors: 401 missing or bad token, 404 unknown game, 400 bad request, 207 multi-status.

## Assumed (marked `shortcut:` or UNVERIFIED in the code)

- Rate limit: none published anywhere I could read. The module waits at least 500 ms between calls and never retries; a 429 is returned to the caller.
- Image hosts: not shown in any source I could read (samples use example.com). The allowlist is `cdn2.steamgriddb.com`, `cdn.steamgriddb.com`, `www.steamgriddb.com`, `steamgriddb.com`, from memory of real responses. Confirm with one real response and fix `DEFAULT_HOSTS`. Until then a wrong list fails closed (`HostNotAllowed`), never open.
- `style` is a string in the docs' samples; the node typings say array. Both are accepted.
- Whether an empty search is `200 []` or `404`: both are treated as no hits.
- `limit` is not sent (only documented as a max); first page only.
- Terms of use: could not read. Images are community uploads. The plan is personal use, download once, cache on the host, never re-serve to the public. Read https://www.steamgriddb.com/terms before anything beyond that, and credit SteamGridDB in the UI.

## Behaviour worth knowing

- A name is searched in normalised form (lowercase, tags like `(USA)` and a trailing `Wii` dropped).
- `rank`: one exact normalised match is confident. Several exact matches are confident only if exactly one is verified. Everything else is not confident and carries up to 3 alternatives for Toby to pick. No shared word means no candidate.
- `save_match` writes `art/<key>/match.json`, `load_match` reads it. Stage 3 must check it first and skip the search.
- Art lists never include NSFW, humor or flashing art. Grid asks for 600x900, hero for 1920x620, then falls back to any size once.
- The key is sent only as the Authorization header to the API base, never to the image host, and is never in errors, Debug output or files.

## Turning it on

The key file already exists on the games VM. Put it at `~/.config/brisk/steamgriddb-api-key` (one line, mode 0600) for the user that runs hostd, or set `BRISK_SGDB_KEY_FILE`. No file, or a file that is not a plain key, means the feature is off. No restart flag exists yet: Stage 3 reads the config when it needs art.
