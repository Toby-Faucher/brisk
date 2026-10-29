#!/usr/bin/env bash
# The full check list from docs/ui-notes.md ("Rust dev tooling" and "Dev tooling"), in order; stops at the
# first failure. Cargo and npm run under the memory cap (scripts/capped.sh, needs systemd-run), so this is
# meant for bawn; BRISK_NO_CAP=1 runs them plain (CI).
set -euo pipefail

all=(fmt clippy test doc deny pure wording embed scripts web browser)
usage() {
  cat <<EOF
Usage: scripts/check.sh [step]...
Runs the named steps in the order given. No step runs them all: ${all[*]}.
Steps:
  fmt      cargo fmt --check
  clippy   clippy -D warnings
  test     nextest (ci profile)
  doc      doc tests
  deny     cargo deny check
  pure     scripts/check-pure-rust.sh
  wording  scripts/check-wording.sh
  embed    clippy and nextest with the embed feature
  scripts  every scripts/test-*.sh, 300 s each
  web      npm run ci in web/
  browser  browser tests and the spring token check in web/
  fuzz     check and fmt the fuzz crate (needs nightly; not in the full run)
  fast     fmt clippy test
EOF
}

steps=()
for a in "$@"; do
  case $a in
    -h | --help) usage; exit 0 ;;
    fast) steps+=(fmt clippy test) ;;
    fmt | clippy | test | doc | deny | pure | wording | embed | scripts | web | browser | fuzz) steps+=("$a") ;;
    *) echo "unknown step: $a" >&2; usage >&2; exit 2 ;;
  esac
done
[ ${#steps[@]} -gt 0 ] || steps=("${all[@]}")

here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"
export PATH=$HOME/.cargo/bin:$PATH
. scripts/capped.sh
cap_check || exit 1
step() { echo; echo "== $*"; }
embed=crates/brisk-hostd/tests/fixtures/embed-dist

run_step() {
  case $1 in
    fmt) step "cargo fmt --check"; capped cargo fmt --check ;;
    clippy) step "clippy"; capped cargo clippy --workspace --all-targets -- -D warnings ;;
    test) step "nextest"; capped cargo nextest run --workspace --profile ci ;;
    doc) step "doc tests"; capped cargo test --doc --workspace ;;
    deny) step "cargo deny"; capped cargo deny check ;;
    pure) step "check-pure-rust"; scripts/check-pure-rust.sh ;;
    wording) step "check-wording"; scripts/check-wording.sh ;;
    embed)
      step "clippy, embed feature"
      BRISK_EMBED_DIR=$embed capped cargo clippy --workspace --all-targets --features brisk-hostd/embed -- -D warnings
      step "nextest, embed feature"
      BRISK_EMBED_DIR=$embed capped cargo nextest run -p brisk-hostd --features embed --profile ci
      ;;
    scripts)
      for t in scripts/test-*.sh; do
        step "$t"
        timeout 300 "$t"
      done
      ;;
    web) step "web: npm run ci"; capped npm --prefix web run ci ;;
    browser)
      step "web: browser tests"
      # Off CI a missing headless shell is a skip, not a failure (web/vitest.config.ts says how to install it).
      if [ -z "${CI:-}" ] && [ -z "${BRISK_CHROMIUM:-}" ] \
        && ! compgen -G "${PLAYWRIGHT_BROWSERS_PATH:-$HOME/.cache/ms-playwright}/chromium_headless_shell-*" >/dev/null; then
        echo "skipped: no Playwright headless shell (npx playwright install chromium-headless-shell, or set BRISK_CHROMIUM)"
      else
        capped npm --prefix web run test:browser
      fi
      step "web: spring tokens"
      npm --prefix web run springs:check
      ;;
    fuzz)
      step "fuzz crate (nightly)"
      if rustup toolchain list | grep -q nightly; then
        capped cargo +nightly check --manifest-path fuzz/Cargo.toml
        cargo fmt --manifest-path fuzz/Cargo.toml --check
      else
        echo "skipped: no nightly toolchain (rustup toolchain install nightly)"
      fi
      ;;
  esac
}

for s in "${steps[@]}"; do run_step "$s"; done
echo
echo "ALL CHECKS PASSED"
