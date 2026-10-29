#!/usr/bin/env bash
# Test for scripts/capped.sh with a stub systemd-run on PATH: capped passes the limits and runs the
# command, cap_check fails when the cap is not enforced, BRISK_NO_CAP runs plain. Run: scripts/test-capped.sh
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
t=$(mktemp -d) || exit 2
trap 'rm -rf "$t"' EXIT
mkdir "$t/bin"
# The stub logs its arguments; it answers the memory.max probe with $STUB_MAX, else runs the command after the options.
cat > "$t/bin/systemd-run" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_LOG"
[ -n "${STUB_FAIL:-}" ] && exit 1
case "$*" in *memory.max*) echo "$STUB_MAX"; exit 0 ;; esac
while [ $# -gt 0 ] && [ "$1" != nice ]; do shift; done
exec "$@"
EOF
chmod +x "$t/bin/systemd-run"
export PATH="$t/bin:$PATH" STUB_LOG="$t/log"
fail=0
check() { # name want_rc got_rc
  if [ "$2" != "$3" ]; then echo "FAIL $1: rc $3, want $2"; fail=1; else echo "ok   $1"; fi
}
in_shell() { (unset BRISK_NO_CAP; [ -n "${NOCAP:-}" ] && export BRISK_NO_CAP=1; . "$here/scripts/capped.sh"; "$@"); }

: > "$STUB_LOG"
out=$(in_shell capped echo hi); check "capped runs the command" 0 $?
[ "$out" = hi ] || { echo "FAIL capped output: '$out'"; fail=1; }
grep -q -- "--scope -q -p MemoryHigh=7G -p MemoryMax=9G" "$STUB_LOG" || { echo "FAIL capped did not pass the limits"; fail=1; }
in_shell capped false; check "capped passes the exit code" 1 $?

STUB_MAX=9663676416 in_shell cap_check; check "cap_check passes when enforced" 0 $?
STUB_MAX=max in_shell cap_check 2>/dev/null; check "cap_check fails when not enforced" 1 $?
STUB_FAIL=1 in_shell cap_check 2>/dev/null; check "cap_check fails when the scope fails" 1 $?

: > "$STUB_LOG"
out=$(NOCAP=1 in_shell capped echo plain); check "BRISK_NO_CAP runs plain" 0 $?
[ "$out" = plain ] || { echo "FAIL no-cap output: '$out'"; fail=1; }
STUB_FAIL=1 NOCAP=1 in_shell cap_check; check "BRISK_NO_CAP cap_check is a no-op" 0 $?
[ -s "$STUB_LOG" ] && { echo "FAIL BRISK_NO_CAP still called systemd-run"; fail=1; }
exit $fail
