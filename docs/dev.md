# Brisk dev tooling

How the code is built and checked. `scripts/check.sh` runs the whole check list in order. Env vars for the
scripts: [env.md](env.md).

## Rust dev tooling
- **Toolchain pin**: `rust-toolchain.toml` names an exact release (1.99.1) with `profile = "minimal"` and
  `components = ["clippy", "rustfmt"]`. rustup can install a missing pinned toolchain on the next
  `cargo` call, but `scripts/brisk-service.sh update` does not rely on that: before the capped build it runs
  `rustup toolchain install` (reads rust-toolchain.toml, 900 s timeout, needs network once after a bump) and
  aborts before any swap or restart if that fails.
  Bump deliberately about every 6 weeks: edit the version, `cargo update`, run the checks below.
  Docs and build tools are not in `minimal`; add a component to the list if a build needs it.
- **Edition 2024** for all crates (`edition.workspace`); `publish = false` on all of them.
- **Lints**: `[workspace.lints]` in the root `Cargo.toml`, inherited with `[lints] workspace = true`.
  `unsafe_op_in_unsafe_fn = deny`; clippy `dbg_macro`, `todo`, `unimplemented`, `undocumented_unsafe_blocks`
  = warn (every `unsafe` needs a `// SAFETY:` comment right above the statement). No print lints (bridge and
  hostd print on purpose); `unwrap_used` is skipped on purpose: src has 112 `unwrap()`s today, so the lint would only add noise.
  nextest runs one process per test, which also avoids races on the env vars one `tap_live.rs` test sets.
- **Checks** (all must pass; use `. scripts/capped.sh; capped cargo ...` for builds on bawn):
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo nextest run --workspace` (profile `ci` = no fail-fast, config in `.config/nextest.toml`),
  `cargo test --doc --workspace` (nextest does not run doc-tests), `cargo deny check`,
  `scripts/check-pure-rust.sh`, `scripts/check-wording.sh`, `scripts/test-*.sh`. The optional `embed` feature needs a built dist at
  compile time, so its checks set one: `BRISK_EMBED_DIR=crates/brisk-hostd/tests/fixtures/embed-dist cargo
  clippy --workspace --all-targets --features brisk-hostd/embed -- -D warnings` and the same for
  `cargo nextest run -p brisk-hostd --features embed`.
- **cargo-deny** (`deny.toml`): graph limited to the six native targets (quinn-proto's wasm-only `ring` edge is
  out of scope); allowed licences MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0, Zlib, CC0; bans `proc-macro1`
  (the Aug 2026 typosquat), `openssl-sys`, `aws-lc-sys`, `ring`, wildcards; sources crates.io only. Seven advisories
  are ignored on purpose, each with a reason and a TODO in `deny.toml`: five `rustls-webpki 0.102.8` (from
  `rustls-rustcrypto`, only `alg_id` constants are used: unreachable), `rsa` Marvin (a direct dependency of
  brisk-proto and brisk-client; private-key use only in the client, so it needs a local attacker; kept until a
  fix or replacement) and `crypto-bigint` 0.5.5 (via p256/p384 from `rustls-rustcrypto`; REACHABLE: every
  bridge handshake signs with P-256 for any tailnet peer, accepted because remote timing is impractical).
  `exclude-dev = true` means advisories in dev-only crates are not reported.
  Duplicate-version warnings are expected. Install: `cargo install --locked cargo-nextest cargo-deny`
  (cargo-deny's `zstd-sys` needs a C compiler and free space in /tmp; the tool is not part of the product).
- **Purity allowlist**: `scripts/check-pure-rust.sh` fails on any `*-sys`/`*_sys` crate or `cc`, `cmake`,
  `bindgen`, `pkg-config`, `vcpkg`, `cxx-build` in the x86_64-linux normal+build graph unless
  `scripts/pure-rust-allow.txt` has a line `crate | reason | date`. One entry today: `linux-raw-sys`
  (rustix's syscall tables, pure Rust, no C; added 2026-10-26 with the pidfd work); read the header of that
  file for why `libc`, `autocfg` and `windows-sys` are not listed. graviola (Steam HTTPS) passes without an
  entry: its verified assembly is Rust `global_asm!`, no C compiler runs.
- **New crates: 14-day rule.** Pin new dependencies exactly (`=x.y.z`) to the newest release at least 14
  days old (crates.io publish date), including new transitive crates (`cargo update -p <crate> --precise`),
  unless it is a security fix. 2026-10-26: ureq 3.4.2 (3.5.0 too new), rustls-graviola 0.4.0, graviola
  0.4.1, rustls-platform-verifier 0.7.1, rustix 1.1.5, sd-notify 0.5.0, tower-http 0.7.1 (2026-08-31,
  feature `fs` only), include_dir 0.7.4 (optional, `embed`); new transitive with tower-http: tokio-util
  0.7.20 (2026-10-09), unicase 2.10.0 (2026-10-05), mime_guess 2.0.5, http-range-header 0.4.2,
  futures-sink 0.3.34, all MIT or MIT/Apache; transitive ureq-proto 0.6.4,
  rustls-native-certs 0.8.4, webpki-root-certs 1.0.9, combine 4.6.8, schannel 0.1.29 held back. `scripts/test-check-pure-rust.sh`
  proves the script with a stub `cargo`.

## Fuzzing and property tests
Every parser of bytes Brisk did not produce (network, tap, browser, Sunshine HTTP/RTSP/SDP, control,
input, audio) is listed with its trust level and coverage in `docs/fuzz-inventory.md`, which also
records what this found (three bugs fixed, among them a key press lost after a send timeout; one
behaviour described and left; one bounded worst case noted).
- **Property tests** (proptest =1.12.0, dev-dependency, `default-features = false, features = ["std"]`:
  no fork/timeout, inputs are kept small instead) run with everything else: `cargo nextest run --workspace`.
  Files: brisk-proto `tests/prop_parsers.rs` (any input, no panic, peak heap under 16 B per input byte
  + 64 KiB, measured by a counting `#[global_allocator]` in that test binary) and `tests/prop_roundtrip.rs`
  (round trips, Reed-Solomon under random loss, AES wrappers), brisk-bridge `tests/prop_input.rs` and
  `tests/prop_audio.rs`, and `#[cfg(test)]` modules in brisk-client `session.rs`, `tap.rs`, `rtsp.rs`.
  Default 256 cases per property (41 tests, about 4 s of test time in parallel; the suite's wall
  time stays about 10 s, set by a 10 s control timeout test); longer local runs:
  `PROPTEST_CASES=5000 cargo nextest run -p brisk-proto` (or `-p brisk-bridge`). A failure prints
  the shrunk input and writes `tests/<file>.proptest-regressions`; commit that file only for a real bug.
- **cargo-fuzz** (0.13.2, `cargo install --locked cargo-fuzz`, nightly: `rustup toolchain install
  nightly --profile minimal`) in `fuzz/`, eight targets: `rtsp`, `rtp_fec`, `control_enet`, `input`,
  `http_sdp`, `tap`, `audio`, `bridge_input` (list: `cargo +nightly fuzz list`). Run from `fuzz/`:
  `cargo +nightly fuzz run <target> corpus/<target> seeds/<target> -- -max_total_time=300 -rss_limit_mb=1024`
  (on bawn wrap it in `capped` and keep `CARGO_TARGET_DIR`/`TMPDIR` off the small /tmp: the
  ASan build of libFuzzer needs a few hundred MB). Stateful targets split the input into datagrams
  (`len u16 LE` + bytes, `fuzz_targets/common.rs`). `fuzz/seeds/` holds small hand-built valid
  samples (committed, about 5 KB); the generated `fuzz/corpus/`, `fuzz/artifacts/` (crash inputs)
  and `fuzz/target/` are git-ignored. Reproduce a crash: `cargo +nightly fuzz run <target> artifacts/<target>/crash-...`.
  Every real crash gets a minimal unit test in the crate first, then the fix.
- **Purity exception**: libfuzzer-sys compiles libFuzzer (C++, via `cc`) and needs nightly, which
  the pure-Rust rule forbids for the product. So `fuzz/` is its own workspace (empty `[workspace]`
  table, own `Cargo.lock`), not a member of the root workspace: the product build,
  `scripts/brisk-service.sh update`, `scripts/check-pure-rust.sh` and `cargo deny check` all read
  the root workspace graph and never see it. Never add `fuzz` to the root `members`.
  For the same reason deny's licence check never sees libfuzzer-sys, which is
  `(MIT OR Apache-2.0) AND NCSA` (libFuzzer's LLVM licence); NCSA is deliberately not in
  deny.toml's allow list, since nothing in the product uses it.
- **Keep `fuzz/Cargo.lock` in step** with the root `Cargo.lock`: after a `cargo update` or bump,
  copy the root lock over it and run `cargo +nightly fuzz build` (cargo adds only the fuzz crates),
  so the fuzzed code builds against the versions the product ships.

## Web dev tooling
- Commands (in web/): `npm run ci` = `biome check .` + `npm run check` + `npm test` + `npm run build` (run it before merging);
  `npm run check` = `astro check` + `svelte-check` (fails on errors; one known svelte-check warning is left: GhostBar's
  `fileInput` is set by `bind:this` and only read in click handlers, never in the template or an effect, so it does not
  need `$state`; making it `$state` would only add reactivity nothing uses); `npm test` = the node unit project only (fast, no
  DOM); `npm run test:browser` = Svelte component tests in real Chrome (separate, not in `ci`).
- Pinning: `web/.npmrc` has `save-exact=true` and every dependency in package.json is an exact version. New packages and
  bumps take the newest version that is at least 14 days old on npm (`npm view <pkg> time --json`; for new transitive
  deps `npm install --before=<date>` where the tree allows it), unless the newer one is a security fix (vitest 5.0.5 and its
  browser packages, fast-uri 3.1.9). Check `npm audit`, `npm ls` and `hasInstallScript` in the lock after a change
  (today only `fsevents`, optional, macOS).
- Biome (`@biomejs/biome`, config in `web/biome.jsonc`) formats `.ts`, `.json` and `.css` (2 spaces, single quotes, 140
  columns) and lints `.ts` plus the script block of `.svelte` and `.astro` files. It does not format `.svelte` or `.astro`.
  It only reads their script block, so a name used only in the template looks unused; the unused-name rules, `useConst` and
  `useImportType` are off for those files, as Biome's language support page advises. Rules that fight the house style are off
  with a reason next to each. `npm run fmt` formats and applies the safe fixes; `npm run ci` runs `biome check .` without
  writing. The spring block in `tokens.css` has a `biome-ignore format` so `springs:check` still matches it. The one
  formatting-only commit is listed in `.git-blame-ignore-revs` (`git config blame.ignoreRevsFile .git-blame-ignore-revs`).
- TypeScript is pinned to 6.0.3: TypeScript 7 (the Go port) is out, but `svelte-check` and `@astrojs/check` declare
  `typescript ^5 || ^6`. Retry when they widen. `web/tsconfig.json` extends `astro/tsconfigs/strict` and includes src,
  `scripts/**/*.mjs` and `vitest.config.ts`; vite defines are declared in `src/env.d.ts`. The scripts are included without
  `checkJs`: turning it on gives about 450 errors, mostly the untyped `window.__x` probes inside `page.evaluate`. `@ts-expect-error` only with a one-line reason; no blanket `any`.
- Accessibility: `web/scripts/axe.mjs` (`@axe-core/playwright`, MPL-2.0) runs the WCAG 2.0/2.1/2.2 A and AA rules and fails
  on serious or critical violations; exceptions live in its `EXCEPTIONS` list with a one-line reason (today: the command
  menu listbox, scrolled from the input through aria-activedescendant). Used by ui-smoke and by
  `node web/scripts/axe-check.mjs` (against `npx astro preview --host 127.0.0.1 --port 4424`, mocked /api; Home, Library,
  Machines, History list + session, every Settings tab, Stream idle and failed, command menu, Quit and restart dialogs, game detail panel).
- Browser tests: `*.browser.test.ts` next to the component, run by the `browser` project in `web/vitest.config.ts`
  (`@vitest/browser-playwright` + `vitest-browser-svelte`, Playwright's own chrome-headless-shell; `BRISK_CHROMIUM`
  overrides it, and a missing one prints how to install it). Reduced motion is forced in the test browser, so no dialog or
  menu animation can make a test wait or flake. Its API server is pinned to `127.0.0.1:4460` (strictPort): Vitest browser mode had critical advisories
  for an exposed API, so never 0.0.0.0. `astro:transitions/client` is aliased to `src/test/astro-transitions.ts` (a `navigate`
  spy). Covered: CommandMenu (Ctrl+K, filter, Enter navigates, Esc returns focus), the Quit confirm (focus in, Tab trap,
  Esc, focus back; and with a test-only transition: still mounted with its text mid-exit, then unmounted), Settings tabs (arrows/Home/End and the hash), GameCard (a click opens the detail panel, never plays or installs), GameDetail (store facts, "No store details", Esc returns focus to the card, Play/Install/Quit).

## How to run the smoke test
`cd web && npm run build && node scripts/ui-smoke.mjs` (needs `target/release/brisk-hostd`, or set `BRISK_HOSTD`; port 8443 free).
It starts hostd on fixtures, the fake bridge, a fake `steam` and a mock Steam API (sentinel key, 139 games) in a temp dir,
drives headless Chrome through Home, Library, Machines, History, Settings (every tab), the restart dialog, Ctrl+K,
accent, the 403 guard, a launch to `live` and the ghost bar, runs axe (see Web dev tooling) on each of those views, then
prints PASS/FAIL (exit 0 only if all pass) and screenshot paths. It cannot prove video: WebTransport to the fake bridge
fails, so Stream is only checked for degrading to "Lost the connection" without page errors. hostd's listen port and Host
check are fixed at 8443, so while the live service holds 8443 the smoke prints `SMOKE SKIPPED (exit 3)` and exits 3
(exit codes: 0 pass, 1 fail, 3 skipped; `BRISK_SMOKE_ALLOW_SKIP=1` makes a skip exit 0). It is not part of `npm run ci`.
Use `web/scripts/axe-check.mjs` for the a11y part meanwhile.
