#!/bin/bash
# Stop Brisk and restore stock Sunshine: SIGTERM to the bridge (it cancels its own session; a
# background job of brisk-up ignores SIGINT),
# stop the page server, then sunshine-brisk.sh rollback (refuses while Sunshine is busy).
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
RUN=${XDG_RUNTIME_DIR:?}/brisk
SB=${BRISK_SB:-$here/scripts/sunshine-brisk.sh} # BRISK_SB: test stub
# shellcheck source=scripts/brisk-common.sh
. "$here/scripts/brisk-common.sh"
# The service keeps patched Sunshine on for good; rolling it back under it is not brisk-down's call.
if systemctl --user is-active -q brisk-hostd.service 2>/dev/null || systemctl --user is-enabled -q brisk-hostd.service 2>/dev/null; then
  echo "brisk-down: hostd is running as a service: use scripts/brisk-service.sh (uninstall, then rollback via scripts/sunshine-brisk.sh)" >&2
  exit 1
fi

if [ -f "$RUN/bridge.pid" ] && kill -0 "$(cat "$RUN/bridge.pid")" 2>/dev/null; then
  pid=$(cat "$RUN/bridge.pid")
  kill -TERM "$pid"
  wait_s=${BRISK_DOWN_WAIT:-60}
  for _ in $(seq "$wait_s"); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
  kill -0 "$pid" 2>/dev/null && { echo "brisk-down: bridge $pid still running after $wait_s s; not touching Sunshine" >&2; exit 1; }
  echo "bridge stopped"
fi
rm -f "$RUN/bridge.pid"
if [ -f "$RUN/page.pid" ]; then kill "$(cat "$RUN/page.pid")" 2>/dev/null || true; rm -f "$RUN/page.pid"; fi
# Also when stock runs but the drop-in exists (a stranded install): the next restart would run brisk.
if brisk_running || [ -f "$BRISK_DROPIN" ]; then spaced_restart "$SB" rollback; fi
"$SB" status
