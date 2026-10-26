# Queued work: tooling, Rust hardening, platform pass

Status: QUEUED by Toby on 2026-10-25 ("queue up changes 1-11 frontend and 1-10 for the rust side ... done afterwards").
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
