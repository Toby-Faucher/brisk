#!/bin/bash
# Stop Brisk and restore stock Sunshine: SIGTERM to brisk-up's hostd (it stops its bridge, which
# cancels its own session; a background job of brisk-up ignores SIGINT), then
# sunshine-brisk.sh rollback (refuses while Sunshine is busy).
# Usage: scripts/brisk-down.sh   (no arguments)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
RUN=${XDG_RUNTIME_DIR:?}/brisk
# shellcheck disable=SC2034 # read by die in brisk-common.sh
ME=brisk-down
# shellcheck source=scripts/brisk-common.sh
. "$here/scripts/brisk-common.sh"
case ${1:-} in -h|--help) echo "usage: $0   (no arguments)"; exit 0 ;; esac
[ $# -eq 0 ] || die "usage: $0 (no arguments)"
# The service keeps patched Sunshine on for good; rolling it back under it is not brisk-down's call.
if systemctl --user is-active -q brisk-hostd.service 2>/dev/null || systemctl --user is-enabled -q brisk-hostd.service 2>/dev/null; then
  die "hostd is running as a service: use scripts/brisk-service.sh (uninstall, then rollback via scripts/sunshine-brisk.sh)"
fi

if pid_alive "$RUN/hostd.pid"; then
  pid=$(cat "$RUN/hostd.pid")
  kill -TERM "$pid"
  wait_s=${BRISK_DOWN_WAIT:-60}
  for _ in $(seq "$wait_s"); do pid_alive "$RUN/hostd.pid" || break; sleep 1; done
  pid_alive "$RUN/hostd.pid" && die "hostd $pid still running after $wait_s s; not touching Sunshine"
  echo "hostd stopped"
fi
rm -f "$RUN/hostd.pid"
# Also when stock runs but the drop-in exists (a stranded install): the next restart would run brisk.
if brisk_running || [ -f "$BRISK_DROPIN" ]; then spaced_restart "$SB" rollback; fi
"$SB" status
