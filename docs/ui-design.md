# Brisk UI: design (v1)

Status: APPROVED by Toby on 2026-10-25. Nothing in this document is built yet; next step is the build plan (docs/plans/).
Companion material: the three mockup rounds (Claude artifacts, links at the end), `docs/m2-notes.md` (what the streaming page already does), `research/shaders/README.md` (shader work).

## 1. Intent and why

Brisk today is a fast, pure-Rust way to stream a game from a Sunshine host to Chrome: a bridge on the host (`brisk-bridge`) plus a page (`web/`) that decodes video and audio and sends input. It works, and it is one unstyled page with a stats box.

Toby wants Brisk to become **his whole dashboard**, "from game streaming to workouts, to an agent orchestrator, and more I haven't thought of yet". Game streaming is step one. The UI therefore has two jobs: be a beautiful, calm, quick front door to game streaming now, and be a **modular shell** that other things (agent orchestration, curated news, workouts, files, terminal, smart home, Proxmox/Dockge, Tailscale devices, Sunshine settings) can plug into later without a redesign.

Toby is the only user. He opens it on a laptop in Chrome mostly, sometimes on a phone. Everything happens on his tailnet.

## 2. Goals, non-goals, anti-goals

Goals
- Streaming remains the fastest path: connect and play in a few clicks, nothing on screen while playing.
- Easy to digest: dense but calm, clear hierarchy, nothing you have to decode.
- A shell where a new module is a self-contained folder that registers itself (sidebar entry, Home widget, pages, settings), with no edits to the shell.
- Feels crafted and alive (lively but tasteful motion, fun sounds, colored filled icons, a drifting-smoke background), like the work of Emil Kowalski and Linear.
- Choose which machine to stream from (one host today, several later).

Non-goals for v1
- Guests, sharing sessions, accounts or sign-in (tailnet-only is the security model).
- Touch/phone controls, on-screen gamepad, recording/clips, multiple simultaneous streams, mobile-first layout, Firefox/Safari, installable PWA.
- Any module other than streaming (agents, news, workouts, files, terminal...). v1 only provides the *slots* for them.
- A controller (gamepad) UI beyond "it just works".

Anti-goals (would make it a failure even if it technically works)
- Anything "not easy to digest": clutter, too many controls on screen, unclear states.
- Any shader or animation cost on the stream screen (it would undo the latency work). The page-enter, tab and scroll-reveal motion (docs/ui-notes.md, Motion) therefore never runs on /stream.
- A look that feels like old-school Moonlight / moonlight-web / Steam Link.

## 3. Constraints

- Stack (Toby's decision): Astro + Svelte 5 + Bits UI (headless components, styled by us). Styling approach: whatever fits Bits UI (builder's call; plain CSS variables recommended, see section 7).
- Browser: Chrome. Firefox 158+ desktop kinda works (connects and streams, but about 100 ms latency and 30-40 fps, likely software decode; no HEVC, no Keyboard Lock). Desktop first; phone must be usable but is secondary.
- The page is served and reached today as: bridge on bawn binds the tailnet IP for WebTransport; the static page is served on `127.0.0.1:8443` on bawn and reached with `ssh -N -L 8443:127.0.0.1:8443 bawn`. v1 should keep working with that, and may improve it (Tailscale serve) if the builder can do so without root surprises.
- Security model: tailnet-only, no sign-in. The WebTransport session token and pinned certificate hash stay as they are (see `docs/m2-notes.md`).
- Dark theme only (Toby's decision). Accent color is user-pickable from six swatches (mint, indigo, pink, orange, lime, sky); mint is the default.
- Existing streaming code in `web/src/lib/{video,audio,input}.ts` is tested and measured (62 tests, p50 numbers in `docs/m0-baseline.md`). It moves into the Stream module as is; do not rewrite it.
- bawn is shared with Toby's games; heavy builds must stay resource-capped as in earlier milestones (see `scripts/capped.sh`).

## 4. Look and feel (decided)

- Reference: Linear (dense but calm, dark, typographic, subtle motion, command menu), Emil Kowalski (taste in motion), Kombai-style dark card dashboards.
- Layout: left sidebar like Linear; Home is a **bento of soft rounded cards** (one card per module), chosen from three options in round 1.
- Corners soft and round (about 16 to 22 px on cards). Icons **filled and colored** per module (accent for Home, amber Library, blue Stream, sky Machines, grey Settings, violet Agents, rose News, green Power/Workouts). Geist for text, Geist Mono for numbers. Motion lively (springy hover lifts, sliding panels) with `prefers-reduced-motion` honored. Friendly copy. Fun UI sounds (click, connect chime), with a mute switch.
- Shaders (see `research/shaders/README.md`): **background smoke only**: a faint drifting-smoke background on Home in the accent color, with no pointer interaction. Cards have no shaders (plain lift and border hover only). **All shaders are off while a stream is open.** Fallback when WebGL2 or float textures are missing, or the shaders setting is off: plain CSS gradient. A water-ripple version is shelved for later. (Steer from Toby, 2026-10-25: no mouse effect, no card shaders; this supersedes the round 1b hover-shader mockup.)
- Mockups for all of this: round 1 (shell + Home) and round 1b (bento with shader hover), round 2 (stream + library).

## 5. Screens in v1

1. **Shell**: sidebar (Home, Library, Stream, Machines, Settings, then module slots, with "soon" entries greyed), a command menu (Ctrl+K: jump to any page, launch a game, switch machine, change quality, toggle stats), keyboard shortcuts, and a host pill showing the active machine's state.
2. **Home**: bento cards: host status (online, Sunshine ready, uptime, GPU/CPU temps, network path), Resume (last game, one click), Quick actions (restart Sunshine, quality presets; Wake is a disabled "later" button), Recent sessions, Add a module. Module widgets appear here as cards.
3. **Library (Toby picked option B, "Shelves")**: a "continue where you left off" hero, then shelves: Installed, Not installed (dimmed, with size and an Install action), plus filters (All, Installed, Not installed, Recent) and search. Clicking a game launches it and starts streaming. Shows every game on the host, installed or not.
4. **Stream (Toby picked option A, "Ghost bar")**: nothing on screen. Pushing the pointer to the top edge slides down a slim glass pill: host chip with latency, mute and volume, keyboard capture, clipboard / file drop (clipboard sync both ways, files to ~/Downloads/brisk; see ui-notes.md), quality (presets Fast, Balanced, Crisp, plus an Advanced panel with resolution, fps, bitrate, codec), stats toggle, fullscreen, disconnect. Stats (hidden by default) open as a floating card with graphs for delay, frame rate and bitrate. On connection drop it reconnects silently (a small toast, keys are released as the bridge already does). Esc is taken by Chrome, so pointer release is **hold Esc** with a ring and a countdown (Keyboard Lock in fullscreen).
5. **Machines**: list of hosts with status (on/off, Sunshine state, uptime, temperatures, running apps, network path), restart Sunshine. **No Wake-on-LAN in v1** (it needs an always-on sender; `dockge` can do it later). Pairing is automatic on the tailnet. One host (bawn) at v1; the data model supports several.
6. **Settings**: minimal at v1 (accent color, sounds on/off, shaders on/off, default quality preset). Toby does not yet know which settings he will change often.

## 6. Architecture

Shell and modules
- A **module** is a folder with a manifest: `id`, title, icon, color, routes, an optional Home widget component, optional settings section, optional command-menu entries. The shell discovers modules at build time and builds the sidebar, routes, Home bento and command menu from them. Streaming, Library and Machines are the first modules; Agents/News/Workouts appear as disabled "soon" entries until they exist.
- Pages are Astro routes; interactive parts are Svelte 5 islands; Bits UI provides accessible primitives (dialogs, command menu, switches, sliders, tooltips). Shared design tokens live in one CSS file.

Data and backend (the one big new piece)
- The page needs data Sunshine cannot give it: the game list with installed state, host stats, launch (and, later, wake). Claude's call (section 9): a small **host agent**, one Rust binary in this repo (`brisk-hostd`, a user systemd service, no root), that runs on bawn as Toby's user inside his session. It serves **both the built static page and a JSON API from one origin, bound to 127.0.0.1:8443** (replacing `python3 -m http.server`), so the existing `ssh -N -L 8443:127.0.0.1:8443 bawn` tunnel keeps working, there is no CORS, and nothing new is exposed on the tailnet. State-changing routes (launch, install, restart) require the per-run secret that the printed page URL already carries (same pattern as the WebTransport token).
- API (v1): `GET /api/host` (online, Sunshine state, uptime, temperatures where a sensor exists, network path from `tailscale status --json`), `GET /api/games` (installed from Steam's local `appmanifest_*.acf`; owned-but-not-installed from the source chosen in section 10; size, last played, cover URL), `POST /api/launch {appid}`, `POST /api/install {appid}`, `POST /api/sunshine/restart`, `GET /api/sessions` (recent sessions), plus the module-registry data it needs.
- Launch = run `steam steam://rungameid/<id>` inside bawn's graphical session (the same way Mewgenics was started during testing), while the stream uses the existing Sunshine "Steam Big Picture" app. Install = `steam://install/<id>`; Steam shows its own progress.
- Persistence: recent sessions and last-played live in a small JSON (or SQLite) file in `~/.local/share/brisk/` on bawn, written by the agent. A session record is `{appid, name, started, ended, host, preset}`.
- Degrade gracefully: any card whose data source is missing (no temperature sensor, no tailscale CLI, etc.) shows a dash and a one-line "not available here", never an error.
- The bridge keeps its WebTransport role unchanged.

Streaming
- The Stream module wraps the existing video/audio/input code. The Connect click remains (Chrome autoplay policy); it can be the Resume/Launch click.
- **Quality is fixed when a session starts** (today 1080p60 H.264 at 20 Mbps; the page cannot change it mid-session). Choosing a preset therefore does a quick, silent reconnect with new launch parameters. Presets: Fast = 1080p60, 10 Mbps; Balanced = 1080p60, 20 Mbps (default); Crisp = 1080p60, 40 Mbps. The Advanced panel adds resolution, fps, bitrate and codec (HEVC arrives with M3) and applies the same way. This needs a small bridge change: accept launch parameters from the page (builder to scope it).
- **Silent reconnect:** the bridge's session token is per bridge run and reusable (Toby has reloaded with the same URL many times), and a dying old session lingers for up to 10 s, so the page can reconnect with the same token. Builder to confirm there is no race with the old session.

## 7. Visual system notes for the builder

- Tokens: bg #0b0c0f, panel #121419, panel2 #171a21, line #22262f, text #eceef3, muted #8e96a5, dim #7a8292 (was #5d6575; raised to 4.5:1 for WCAG AA), accent per swatch (mint #5eead4 default). Geist 400/500/600/700, Geist Mono 400/500 (variable fonts since 2026-10-26; 650 is written as 700). Easing `cubic-bezier(.2,.8,.2,1)`.
- The shader code is WebGL2 GLSL under `research/shaders/ports/` (own implementation, no library). The earlier plan to use the shaders.com Svelte package (MIT, WebGPU) remains an option; decide when building the Home background.
- Cover art: placeholder gradients with an initial in the mockups; real art source is open (section 8).

## 8. Left to the builder

Component structure, file layout, state management, testing approach (Playwright with the existing headless harness plus Vitest), how the module registry is implemented (Astro content collections, import.meta.glob, or a generated manifest), the exact API shape, sound design, copy, and accessibility details (keyboard operability, focus rings, reduced motion).

## 9. Calls Claude made (Toby can veto any of them)

- One Rust host agent serves page + API on 127.0.0.1:8443 (same origin through the existing tunnel), user service, no root; mutating routes need the per-run secret from the printed URL.
- Steam is the only library in v1 (Toby: "Steam on the host"). Launch via `steam://rungameid`, install via `steam://install`.
- Acceptance: all existing tests stay green (62 page tests, the Rust workspace), the stream's latency is re-measured against `docs/m0-baseline.md` after the UI lands (the stream path must not regress), and the UI matches the exported mockups in `docs/ui-mockups/` (open them in Chrome; they are self-contained HTML).
- Command-menu game list comes from the same client store as the Library (fetched once, refreshed on focus); stale data is acceptable.
- Geist fonts self-hosted; module registry via `import.meta.glob`; cover art from Steam's public CDN with the gradient placeholder as fallback.

- A host agent in Rust for games, launch and status (rather than scraping Sunshine's app list).
- Cover art from Steam's public image CDN keyed by app id, with the gradient+initial placeholder as fallback.
- Sounds are short synthesized tones generated in the page (no audio files to license or host).
- Settings page kept minimal at v1.
- The command menu is part of the shell from day one, since the ghost bar and library both lean on it.

## 10. Toby's answers (all open questions resolved)

1. **Wake-on-LAN: skipped in v1.** Later, `dockge` (always-on, on the tailnet) can send the magic packet; at that point the site/agent can also live there.
2. **Not-installed games: resolved with a Steam Web API key.** Steam ID64 `76561198799313419` (public, not a secret). The anonymous games page is behind Steam's sign-in wall even though "Game details" is Public, so the host agent uses `IPlayerService/GetOwnedGames` with Toby's own key, stored **only on bawn** at `~/.config/brisk/steam-api-key` (mode 0600, inside the private `~/.config/brisk/` folder; 32 characters). Verified 2026-10-25: the call returns 139 owned games with names and playtime (Mewgenics, Hades II, Stardew Valley, Elden Ring, Rust and more). The key must never be written to the repo, logs, API responses, artifacts or chat. If it is ever suspected leaked, regenerate it at https://steamcommunity.com/dev/apikey.
   - Names and playtime come from `GetOwnedGames` (`include_appinfo=1`); store pages, capsule art and sizes from the public store API and Steam's CDN, keyed by app id; installed state and install size from bawn's local `appmanifest_*.acf`.
   - The mockup game list and hours are made-up sample data; the real library is 139 games, so the Library needs paging or virtualised shelves and good search from day one.

## Mockups (Claude artifacts, private)

- Round 1: https://claude.ai/artifact/7qkFKiKaj7vpe3WEA1JGLk (three Home directions; Toby chose bento cards)
- Round 1b: https://claude.ai/artifact/A9H3toH3gpyUFVvg8mdoMj (bento with hover shaders)
- Round 2: https://claude.ai/artifact/24qrCvhmKZUvgtaT2nHech (stream and library; Toby chose Ghost bar and Shelves)
- Shaders: https://claude.ai/artifact/MCDKv7CqAtKmSnMZysuJy4
