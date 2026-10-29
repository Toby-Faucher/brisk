# Contributing

Brisk is a personal project, but changes are welcome. Keep them small and focused.

## Before you open a pull request

Run `scripts/check.sh`. It runs every check in order and stops at the first failure: rustfmt, clippy with
warnings as errors, the tests (nextest and doc tests), `cargo deny`, the pure-Rust check, the wording check,
clippy and tests again with the `embed` feature, the script tests, and the web checks (`npm run ci`). All of them must pass. On a
small machine, run builds under the memory cap: `. scripts/capped.sh; capped cargo ...`. Details:
[docs/dev.md](docs/dev.md).

## House rules

- **Pinned versions.** The Rust toolchain is pinned in `rust-toolchain.toml`. Bump it on purpose, then run
  `cargo update` and every check. npm dependencies are exact versions. A new package or a bump takes the
  newest version that is at least 14 days old, unless a newer one fixes a security issue. Run `npm audit`
  after a change. New crates follow the same 14-day rule.
- **Pure Rust.** The product has no C or C++ code. `scripts/check-pure-rust.sh` enforces it. The fuzz
  workspace in `fuzz/` is the one exception and is kept outside the main workspace.
- **`unsafe` needs a reason.** Put a `// SAFETY:` comment right above each unsafe statement.
- **Plain writing.** README, docs and the web app's text should read like a person wrote them: short,
  plain sentences, no em dashes, no stock filler phrases. `scripts/check-wording.sh` checks this. Reword
  the sentence instead of adding it to the allowlist.
- **No secrets in the repo.** It is public. Never commit API keys, the `x-brisk-secret`, PINs, private
  keys, logs, tailnet names or IPs, or home directory paths. Use placeholders such as `100.64.0.1`.
- **No root in scripts.** Scripts never call `sudo`. A step that needs root is printed for a person to run.
- **Do not replace `web/dist` by hand.** It is a symlink that `scripts/brisk-service.sh update` swaps.

## Branches and pull requests

1. Branch off `main` with a short name, such as `fix/close-x-padding` or `feat/art-picker`.
2. Make small commits, each with a clear message.
3. Run `scripts/check.sh`, push the branch and open a pull request against `main`. Say what changed and
   how you checked it.
4. Update the docs in the same pull request when behaviour, a route or an env var changes
   ([docs/env.md](docs/env.md) lists every env var).
