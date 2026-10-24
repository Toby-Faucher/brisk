#!/bin/bash
# Stop Brisk and restore stock Sunshine: SIGINT to the bridge (it cancels its own session),
# stop the page server, then sunshine-brisk.sh rollback (refuses while Sunshine is busy).
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
RUN=${XDG_RUNTIME_DIR:?}/brisk
SB=$here/scripts/sunshine-brisk.sh
# shellcheck source=scripts/brisk-common.sh
. "$here/scripts/brisk-common.sh"

if [ -f "$RUN/bridge.pid" ] && kill -0 "$(cat "$RUN/bridge.pid")" 2>/dev/null; then
  pid=$(cat "$RUN/bridge.pid")
  kill -INT "$pid"
  for _ in $(seq 60); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
  kill -0 "$pid" 2>/dev/null && { echo "brisk-down: bridge $pid still running after 60 s; not touching Sunshine" >&2; exit 1; }
  echo "bridge stopped"
fi
rm -f "$RUN/bridge.pid"
if [ -f "$RUN/page.pid" ]; then kill "$(cat "$RUN/page.pid")" 2>/dev/null || true; rm -f "$RUN/page.pid"; fi
if brisk_running; then spaced_restart "$SB" rollback; fi
"$SB" status
