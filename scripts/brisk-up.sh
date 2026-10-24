#!/bin/bash
# Start Brisk for a browser on the tailnet: patched Sunshine, a fresh short-lived certificate,
# the page on http://127.0.0.1:8443 (reach it from the laptop over `ssh -L`), then the bridge in
# the foreground. Ctrl-C stops the bridge (it cancels its Sunshine session); then run
# scripts/brisk-down.sh to put stock Sunshine back.
# Usage: scripts/brisk-up.sh [bridge args, e.g. --secs 600]
# Env: BRISK_APP (default 870779643, Desktop), BRISK_LISTEN (default <tailscale ip>:4433),
#      BRISK_PAGE_PORT (default 8443)
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
APP=${BRISK_APP:-870779643}
LISTEN=${BRISK_LISTEN:-$(tailscale ip -4 | head -1):4433}
PORT=${BRISK_PAGE_PORT:-8443}
TLS=$HOME/.config/brisk/tls
RUN=${XDG_RUNTIME_DIR:?}/brisk
SB=$here/scripts/sunshine-brisk.sh
die() { echo "brisk-up: $*" >&2; exit 1; }
mkdir -p -m 700 "$RUN" "$TLS"

# shellcheck source=scripts/brisk-common.sh
. "$here/scripts/brisk-common.sh"

[ -f "$RUN/bridge.pid" ] && kill -0 "$(cat "$RUN/bridge.pid")" 2>/dev/null && die "a bridge is already running (pid $(cat "$RUN/bridge.pid"))"

# 1. Build (no-op when up to date), capped like every heavy step on bawn.
( cd "$here" && PATH=$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=4 nice -n 19 cargo build --release -q -p brisk-bridge )
if [ ! -f "$here/web/dist/index.html" ]; then
  ( cd "$here/web" && nice -n 19 npm ci --no-audit --no-fund && nice -n 19 npm run build )
fi

# 2. Certificate: ECDSA P-256, 13 days (Chrome's serverCertificateHashes allows at most 14),
#    renewed when older than 10 days. Never committed (*.pem is gitignored; it lives outside the repo).
if [ ! -f "$TLS/bridge.pem" ] || [ -n "$(find "$TLS/bridge.pem" -mtime +10)" ]; then
  ( umask 077; openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 13 \
      -subj /CN=brisk-bridge -keyout "$TLS/bridge.key.pem" -out "$TLS/bridge.pem" 2>/dev/null )
  echo "new certificate $TLS/bridge.pem (13 days)"
fi

# 3. Patched Sunshine (sunshine-brisk.sh refuses unless serverinfo is FREE).
if ! brisk_running; then
  "$SB" install # writes the drop-in; the caps are already set (no root)
  spaced_restart "$SB" enable
  for _ in $(seq 30); do serverinfo_free && break; sleep 1; done
fi
serverinfo_free || die "Sunshine is not FREE (busy or not answering); not launching"

# 4. Page on loopback only.
python3 -m http.server --bind 127.0.0.1 "$PORT" -d "$here/web/dist" >/dev/null 2>&1 &
echo $! > "$RUN/page.pid"
trap 'kill "$(cat "$RUN/page.pid")" 2>/dev/null; rm -f "$RUN/page.pid" "$RUN/bridge.pid"' EXIT

cat <<MSG
On the laptop:   ssh -N -L $PORT:127.0.0.1:$PORT $(hostname)
Then open the "page" URL below in Chrome. Stop: Ctrl-C here, then scripts/brisk-down.sh.
MSG
"$here/target/release/brisk-bridge" --host 127.0.0.1 --app "$APP" --listen "$LISTEN" \
  --cert "$TLS/bridge.pem" --key "$TLS/bridge.key.pem" --page "http://localhost:$PORT/" "$@" &
echo $! > "$RUN/bridge.pid"
wait "$(cat "$RUN/bridge.pid")"
