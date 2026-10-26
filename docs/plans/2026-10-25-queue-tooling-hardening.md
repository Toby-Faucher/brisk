# Queued work: tooling, Rust hardening, platform pass

Status: IN PROGRESS overnight 2026-10-26 (Toby: 'do all of the things in the queue including this stuff here ... I am going to sleep so just continue implementing'). Originally QUEUED by Toby on 2026-10-25 ("queue up changes 1-11 frontend and 1-10 for the rust side ... done afterwards").
Do NOT start until `hosting` and `anime-pass` are merged (they touch package.json / lockfile / hostd). Sources:
`research/ecosystem-frontend-2026-10.md`, `research/ecosystem-rust-2026-10.md`, `research/anime-feature-map.md`.

## Order (avoids lockfile and hostd merge conflicts)

1. **Tooling pass** (front end 2-5, 10, 11; Rust 1-4): one branch each side, small commits, no behaviour change.
2. **Rust hardening pass** (Rust 5-8, 9, 10): hostd/bridge behaviour changes, one reviewed branch.
3. **Platform pass** (front end 6-9): CSS/browser features replacing code; item 9 touches the Stream page, so it needs a careful, measured review.

## Front end (numbers are Toby's list)

1. Update Chrome 152 -> 156: Toby, done by hand (not a task).
2. `save-exact=true` in `web/.npmrc`; pin every dependency exactly in package.json.
3. Bump layerchart 2.6.1 -> 2.9.1 and mediabunny 1.61.4 -> 1.62.1 (check the History charts and the replay clip after).
4. Type-checking: `svelte-check`, `typescript` pinned 6.0.3 (NOT 7: the Svelte/Astro checkers stop at 6.x), a `tsconfig.json`; add `npm run check`; fix what it finds.
5. `@axe-core/playwright` in `web/scripts/ui-smoke.mjs` (a11y violations fail the smoke test).
6. `@starting-style` exits for dialogs and menus (Bits UI `data-state` hooks).
7. Popover API + CSS anchor positioning for tooltips.
8. Same-document View Transition for the Library filter/search reshuffle (compare with the anime.js FLIP result before choosing).
9. Screen Wake Lock on the Stream page (laptop must not sleep mid-game). STREAM ROUTE: measure, keep it tiny, review hard.
10. Variable Geist fonts: `@fontsource-variable/geist` and `geist-mono` (about 21 kB lighter).
11. Vitest browser mode (`@vitest/browser-playwright`, `vitest-browser-svelte`) for the first Svelte component tests; keep its API on loopback (earlier critical advisories).

## Rust (numbers are Toby's list)

1. `rust-toolchain.toml` pinned to 1.99.1.
2. Edition 2024 for all crates and a `[workspace.lints]` table (start with a conservative clippy set).
3. `cargo-nextest` and `cargo-deny` (config in repo: licences, advisories, bans, sources); document in docs/ui-notes.md.
4. Make `scripts/check-pure-rust.sh` reject every `-sys` crate / build-time C compiler dependency except an explicit allowlist (it only rejects four names today); note `wayland-backend` has an unconditional `cc` build-dep.
5. `ureq` + `rustls-graviola` replacing the `curl` spawn for the Steam API (keep the key off argv/env/logs; keep the tests with the mock server; check-pure-rust must still pass).
6. `tower-http` `ServeDir` replacing hand-written static serving (range, precompressed, ETag; keep the 404-for-missing-asset and traversal tests).
7. `rustix` pidfd (`pidfd_open` + `pidfd_send_signal`) replacing the `kill` spawn in Quit game (fixes the pid-reuse race; removes unsafe blocks).
8. `sd-notify` (Type=notify, READY, watchdog) once the hosting service is merged.
9. `cargo-fuzz` targets for the brisk-proto parsers (RTSP, RTP, FEC, ENet-ish control, input) plus `proptest` round-trips.
10. `rust-embed` single-binary deploy (embed web/dist), decided together with the hosting service layout.

Deferred with their modules: `tantivy` (Agents memory), xterm.js + `portable-pty` (Terminal), `marked` + Sanitizer + Svelte Flow (Agents), jassub + media-chrome + mediabunny HLS (Media).
Known risks to remember: 7 unreachable advisories from `rustls-rustcrypto` (no replacement for the bridge's QUIC crypto yet); the Aug 2026 crates.io malware campaign (use cargo-deny, review new build scripts).

## Motion pass 2 (added 2026-10-26, Toby: do all of it)

From research/anime-feature-map.md tier 1-2 ideas not yet built, with the rules in docs/ui-notes.md 'Motion rules' (anime.js never on /stream, lazy, revert on destroy, reduced motion, end-state fallback on load failure):
- #6 status-dot ring (host pill, Machines) - S
- #5 icon morphs (mute slash, play to spinner; NOT on /stream) - S-M
- #11 split-text Library headline - S
- #15 controller-test result chips stagger + drift % count-up - S
- #12 connection path dot on Machines (bawn to laptop, direct/relay) - S-M
- #13 Machines status transitions - M
- #9 'Add a module' card swells into the picker (layout animation) - M-L
- #14 session cover grows into detail header (shared element) - L
- #10 Home card drag-to-reorder (default: drag handle, spring drop, order saved in hostd prefs, 'Reset layout' button) - L

## Overnight rulings (made without Toby)
- Merging reviewed, fully green branches into LOCAL main is authorised by 'just continue implementing' (no push: no remote is configured).
- rust-embed ships as an OPTIONAL cargo feature `embed`; the default build keeps serving web/dist from disk because the hosting update/rollback flow (atomic dist symlink swap) depends on it.
- sd-notify: hostd calls sd_notify only when NOTIFY_SOCKET is set; the unit moves to Type=notify only after a transient-unit test; deploy keeps a backup of the previous unit file and `doctor` must pass or the previous unit is restored.
- npm packages: pick the newest exact version that is at least 14 days old (supply-chain caution after the Aug 2026 crates.io campaign), unless it is a security fix.
- ntfy: still no topic from Toby; phone push (Remote Control) inactive: results are written to docs/plans/overnight-2026-10-26-report.md and the chat.
- Nothing needing root/sudoq is run overnight; no reboot test; no live service restart except `scripts/brisk-service.sh update` which fails closed when a stream is live.
