#!/usr/bin/env bash
# Test for scripts/capped.sh with a stub systemd-run on PATH: capped passes the limits and runs the
# command, cap_check fails when the cap is not enforced, BRISK_NO_CAP runs plain. Run: scripts/test-capped.sh
here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=scripts/lib-test.sh
. "$here/scripts/lib-test.sh"
stub_cap
set +e
check() { # name want_rc got_rc
  if [ "$2" != "$3" ]; then fail "$1: rc $3, want $2"; else echo "ok   $1"; fi
}
in_shell() { (unset BRISK_NO_CAP; [ -n "${NOCAP:-}" ] && export BRISK_NO_CAP=1; . "$here/scripts/capped.sh"; "$@"); }

out=$(in_shell capped echo hi); check "capped runs the command" 0 $?
[ "$out" = hi ] || fail "capped output: '$out'"
grep -q -- "--scope -q -p MemoryHigh=7G -p MemoryMax=9G" "$L" || fail "capped did not pass the limits"
in_shell capped false; check "capped passes the exit code" 1 $?

in_shell cap_check; check "cap_check passes when enforced" 0 $?
MEMMAX=max in_shell cap_check 2>/dev/null; check "cap_check fails when not enforced" 1 $?
SR_FAIL=1 in_shell cap_check 2>/dev/null; check "cap_check fails when the scope fails" 1 $?

: > "$L"
out=$(NOCAP=1 in_shell capped echo plain); check "BRISK_NO_CAP runs plain" 0 $?
[ "$out" = plain ] || fail "no-cap output: '$out'"
SR_FAIL=1 NOCAP=1 in_shell cap_check; check "BRISK_NO_CAP cap_check is a no-op" 0 $?
[ -s "$L" ] && fail "BRISK_NO_CAP still called systemd-run"
exit $bad
