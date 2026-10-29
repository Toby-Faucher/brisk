#!/bin/bash
# Stubbed test of brisk-up.sh's exit cleanup: after Ctrl-C hostd's bridge may still be cancelling
# its Sunshine session, so hostd.pid must stay until hostd is gone (brisk-down waits on it).
# Also spaced_restart's gap. Touches nothing real: "hostd" is a sleep named brisk-hostd, $RUN a
# temp dir.
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/lib-test.sh
. "$here/lib-test.sh"
RUN=$t
# shellcheck source=scripts/brisk-common.sh
. "$here/brisk-common.sh"
set +e

fake_hostd 1; echo $! > "$RUN/hostd.pid"   # a hostd that needs 1 s to stop
hostd_exit_cleanup & c=$!
sleep 0.3
[ -f "$RUN/hostd.pid" ] || fail "hostd.pid removed while hostd still runs"
wait $c
[ -f "$RUN/hostd.pid" ] && fail "hostd.pid kept after hostd exited"

# A hostd that outlives the wait keeps its pid file (brisk-down deals with it).
fake_hostd 5; b=$!; echo $b > "$RUN/hostd.pid"
BRISK_EXIT_WAIT=1 hostd_exit_cleanup
[ -f "$RUN/hostd.pid" ] || fail "pid file of a still-running hostd removed"
kill $b

# A pid that is alive but not hostd's is stale: the file goes at once.
sleep 5 & o=$!; echo $o > "$RUN/hostd.pid"
BRISK_EXIT_WAIT=3 hostd_exit_cleanup
[ -f "$RUN/hostd.pid" ] && fail "stale hostd.pid kept"
kill $o

# spaced_restart waits out the gap (default 100 s) after the last restart, then records the new
# one. date and sleep are shell functions here, so the wait is exact and takes no time.
date() { echo 5000; }
sleep() { echo "slept $*"; }
echo 4990 > "$RUN/last-restart"
out=$(unset BRISK_RESTART_GAP; spaced_restart echo ran)
[ "$out" = $'waiting 90s (Sunshine restarts stay 100 s apart)\nslept 90\nran' ] || fail "spaced_restart gap: $out"
[ "$(cat "$RUN/last-restart")" = 5000 ] || fail "last-restart not updated"
echo 0 > "$RUN/last-restart"
[ "$(BRISK_RESTART_GAP=1 spaced_restart echo ran)" = ran ] || fail "spaced_restart waited after an old restart"
unset -f date sleep
[ $bad = 0 ] && echo "ok: brisk-up exit cleanup"
exit $bad
