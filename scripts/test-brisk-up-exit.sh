#!/bin/bash
# Stubbed test of brisk-up.sh's exit cleanup: after Ctrl-C the bridge may still be cancelling its
# Sunshine session, so bridge.pid must stay until that process is gone (brisk-down waits on it).
# Touches nothing real: the "bridge" is a sleep, $RUN a temp dir.
set -u
here=$(cd "$(dirname "$0")" && pwd)
RUN=$(mktemp -d); trap 'rm -rf "$RUN"' EXIT
fail() { echo "FAIL: $*"; exit 1; }
# shellcheck source=scripts/brisk-common.sh
. "$here/brisk-common.sh"
declare -F bridge_exit_cleanup >/dev/null || fail "brisk-common.sh has no bridge_exit_cleanup"

sleep 2 & echo $! > "$RUN/bridge.pid"         # a bridge that needs 2 s to cancel
sleep 60 & page=$!; echo $page > "$RUN/page.pid"
bridge_exit_cleanup & c=$!
sleep 0.5
[ -f "$RUN/bridge.pid" ] || fail "bridge.pid removed while the bridge still runs"
wait $c
[ -f "$RUN/bridge.pid" ] && fail "bridge.pid kept after the bridge exited"
kill -0 $page 2>/dev/null && fail "page server still running"

# A bridge that outlives the wait keeps its pid file (brisk-down deals with it).
sleep 5 & b=$!; echo $b > "$RUN/bridge.pid"
BRISK_EXIT_WAIT=1 bridge_exit_cleanup
[ -f "$RUN/bridge.pid" ] || fail "pid file of a still-running bridge removed"
kill $b
echo "ok: brisk-up exit cleanup"
