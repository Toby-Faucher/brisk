#!/bin/bash
# Stubbed test of brisk-up.sh's exit cleanup: after Ctrl-C hostd's bridge may still be cancelling
# its Sunshine session, so hostd.pid must stay until hostd is gone (brisk-down waits on it).
# Also spaced_restart's gap. Touches nothing real: "hostd" is a sleep named brisk-hostd, $RUN a
# temp dir.
set -eu # setup: any failure stops the test (set +e below, before the cases)
here=$(cd "$(dirname "$0")" && pwd)
RUN=$(mktemp -d); trap 'rm -rf "$RUN"' EXIT
fail() { echo "FAIL: $*"; exit 1; }
# shellcheck source=scripts/brisk-common.sh
. "$here/brisk-common.sh"
declare -F hostd_exit_cleanup >/dev/null || fail "brisk-common.sh has no hostd_exit_cleanup"
ln -s "$(command -v sleep)" "$RUN/brisk-hostd"
set +e
# Waits until the fake hostd has exec'd (its name is the parent's until then).
execed() { for _ in $(seq 100); do [ "$(cat "/proc/$1/comm" 2>/dev/null)" = brisk-hostd ] && return; sleep 0.05; done; echo "fake hostd $1 did not start"; }

"$RUN/brisk-hostd" 2 & execed $!; echo $! > "$RUN/hostd.pid"   # a hostd that needs 2 s to stop
hostd_exit_cleanup & c=$!
sleep 0.5
[ -f "$RUN/hostd.pid" ] || fail "hostd.pid removed while hostd still runs"
wait $c
[ -f "$RUN/hostd.pid" ] && fail "hostd.pid kept after hostd exited"

# A hostd that outlives the wait keeps its pid file (brisk-down deals with it).
"$RUN/brisk-hostd" 5 & b=$!; execed $b; echo $b > "$RUN/hostd.pid"
BRISK_EXIT_WAIT=1 hostd_exit_cleanup
[ -f "$RUN/hostd.pid" ] || fail "pid file of a still-running hostd removed"
kill $b

# A pid that is alive but not hostd's is stale: the file goes at once.
sleep 5 & o=$!; echo $o > "$RUN/hostd.pid"
BRISK_EXIT_WAIT=3 hostd_exit_cleanup
[ -f "$RUN/hostd.pid" ] && fail "stale hostd.pid kept"
kill $o

# spaced_restart waits out BRISK_RESTART_GAP after the last restart, then records the new one.
date +%s > "$RUN/last-restart"
out=$(BRISK_RESTART_GAP=1 spaced_restart echo ran)
grep -q '^waiting 1s' <<<"$out" && grep -qx ran <<<"$out" || fail "spaced_restart gap: $out"
echo 0 > "$RUN/last-restart"
[ "$(BRISK_RESTART_GAP=1 spaced_restart echo ran)" = ran ] || fail "spaced_restart waited after an old restart"
[ "$(cat "$RUN/last-restart")" -gt 0 ] || fail "last-restart not updated"
echo "ok: brisk-up exit cleanup"
