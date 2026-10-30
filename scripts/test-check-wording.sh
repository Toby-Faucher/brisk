#!/usr/bin/env bash
# Test for scripts/check-wording.sh: bad phrases fail, plain text passes, the allowlist and the
# skipped paths work. Run: scripts/test-check-wording.sh
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
t=$(mktemp -d) || exit 2
trap 'rm -rf "$t"' EXIT
# Setup steps exit 2 on failure (no pipefail: `run | grep` below expects run to fail).
mkdir -p "$t/scripts" "$t/docs/plans" "$t/web/src/ui" || exit 2
cp "$here/scripts/wording-banned.txt" "$t/scripts/" || exit 2
: > "$t/scripts/wording-allow.txt" || exit 2
fail=0
run() { WORDING_ROOT=$t "$here/scripts/check-wording.sh" 2>&1; }
expect() { # name want_rc
  out=$(run); rc=$?
  if [ "$rc" != "$2" ]; then echo "FAIL $1: rc $rc, want $2"; echo "$out"; fail=1; else echo "ok   $1"; fi
}

echo "Plain words about a stream." > "$t/README.md"
expect "plain text passes" 0

echo "Delve into a seamless, robust experience." > "$t/README.md"
expect "stock phrases fail" 1
run | grep -q "README.md:1:" || { echo "FAIL: hit lacks file:line"; fail=1; }

printf 'A line — with a dash.\n' > "$t/README.md"
expect "em dash fails" 1

printf 'It is smooth.\n' > "$t/README.md"
echo "Leverage this." > "$t/docs/plans/p.md"
expect "plans are skipped" 0

echo 'toast("Seamless play")' > "$t/web/src/ui/a.ts"
expect "web strings are checked" 1
echo 'const x = "robust";' > "$t/web/src/ui/a.test.ts"; rm "$t/web/src/ui/a.ts"
expect "tests are skipped" 0

mkdir -p "$t/site" || exit 2
echo '<p>A seamless experience</p>' > "$t/site/index.html"
expect "site pages are checked" 1
echo '<p>Plain words about a stream.</p>' > "$t/site/index.html"
expect "plain site page passes" 0
rm "$t/site/index.html"

echo "Run the test harness." > "$t/README.md"
expect "test harness is fine" 0
echo "Harness the power of it." > "$t/README.md"
expect "harness the power fails" 1

echo "A robust claim." > "$t/README.md"
echo "README.md | robust claim | quoted from the spec" > "$t/scripts/wording-allow.txt"
expect "allowlist lets a line through" 0
echo "A robust plan." >> "$t/README.md"
expect "allowlist is per line text" 1
echo "Don't call it robust." > "$t/README.md"
echo "README.md | Don't call it robust | an apostrophe in the text" > "$t/scripts/wording-allow.txt"
expect "allowlist text with an apostrophe" 0
exit $fail
