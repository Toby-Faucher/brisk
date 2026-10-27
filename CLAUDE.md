# brisk: rules for agents

Rust workspace (`crates/`) plus an Astro + Svelte UI (`web/`). Details: `docs/ui-notes.md` ("Rust dev tooling", "Dev tooling").

- **Cargo** lives in `~/.cargo/bin` (add it to PATH). On bawn run builds capped: `. scripts/capped.sh; capped cargo ...`.
- **Toolchain** is pinned in `rust-toolchain.toml` (1.99.1), edition 2024. Bump deliberately, then `cargo update` and run all checks.
- **npm pins**: `web/.npmrc` has `save-exact=true`; every dependency is an exact version. A new package or bump takes the newest
  version at least 14 days old, unless the newer one is a security fix. Check `npm audit` after a change.
- **`unsafe`** needs a `// SAFETY:` comment right above the statement (clippy `undocumented_unsafe_blocks`).
- **`web/dist`** is a symlink to `web/dist.<stamp>`, swapped by `scripts/brisk-service.sh update`. Do not replace it by hand.
- **Checks** (all must pass): `scripts/check.sh` runs the whole list: fmt, clippy `-D warnings`, nextest, doc tests, `cargo deny check`,
  `scripts/check-pure-rust.sh`, `scripts/check-wording.sh`, the embed-feature clippy and nextest runs, `scripts/test-*.sh`, `npm --prefix web run ci`.
- **Wording**: README, docs and the web app's strings must read like a person wrote them. `scripts/check-wording.sh` fails on stock AI phrases and em dashes (list: `scripts/wording-banned.txt`). Write plain, short sentences; do not allowlist a phrase just to get past it.
- **Product code is pure Rust** (no C/C++ in the bridge); `scripts/check-pure-rust.sh` enforces it.
- **Root**: never run `sudo` directly. Use `sudoq run --why "<reason>" -- <cmd>`; exit 77 = denied (stop and ask), 75 = no answer.
- **The repo is public**: never commit secrets (Steam API key, `x-brisk-secret`, PINs, keys, logs), tailnet IDs or home paths.
  `.gitignore` already blocks `*.pem`, `*.key`, `identity/`.
- Env vars: `docs/env.md`. Web check scripts: `web/scripts/README.md`.
