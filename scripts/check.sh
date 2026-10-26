#!/usr/bin/env bash
# The full check list from docs/ui-notes.md ("Rust dev tooling" and "Dev tooling"), in order; stops at the
# first failure. Cargo runs under the memory cap (scripts/capped.sh, needs systemd-run), so this is meant for bawn.
# Usage: scripts/check.sh
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"
export PATH=$HOME/.cargo/bin:$PATH
. scripts/capped.sh
step() { echo; echo "== $*"; }
embed=crates/brisk-hostd/tests/fixtures/embed-dist

step "cargo fmt --check"
capped cargo fmt --check
step "clippy"
capped cargo clippy --workspace --all-targets -- -D warnings
step "nextest"
capped cargo nextest run --workspace --profile ci
step "doc tests"
capped cargo test --doc --workspace
step "cargo deny"
capped cargo deny check
step "check-pure-rust"
scripts/check-pure-rust.sh
step "clippy, embed feature"
BRISK_EMBED_DIR=$embed capped cargo clippy --workspace --all-targets --features brisk-hostd/embed -- -D warnings
step "nextest, embed feature"
BRISK_EMBED_DIR=$embed capped cargo nextest run -p brisk-hostd --features embed --profile ci
for t in scripts/test-*.sh; do
  step "$t"
  "$t"
done
step "web: npm run ci"
npm --prefix web run ci
echo
echo "ALL CHECKS PASSED"
