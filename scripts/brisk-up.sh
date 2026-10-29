#!/bin/bash
# Start Brisk by hand (the dev path; normally hostd runs as a service, scripts/brisk-service.sh):
# patched Sunshine, then brisk-hostd in the foreground. hostd serves the page and API on
# http://127.0.0.1:8443 (reach it from the laptop over `ssh -L`), starts/stops the bridge itself
# and makes/renews the bridge certificate at each stream start. Ctrl-C is passed on to hostd
# as SIGTERM; hostd stops its bridge first (the bridge cancels its Sunshine session); then run
# scripts/brisk-down.sh to put stock Sunshine back. ($RUN/hostd.pid holds hostd's pid.)
# Usage: scripts/brisk-up.sh   (no arguments; -h prints this usage)
# Env: BRISK_APP (default 870779643, Desktop), BRISK_LISTEN (the bridge's address; default: hostd
#      asks `tailscale ip -4` at each stream start), plus hostd's own BRISK_* (data dir, steam dir ...).
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
APP=${BRISK_APP:-870779643}
PORT=8443 # hostd binds 127.0.0.1:8443
RUN=${XDG_RUNTIME_DIR:?}/brisk
# shellcheck disable=SC2034 # read by die in brisk-common.sh
ME=brisk-up
# shellcheck source=scripts/brisk-common.sh
. "$here/scripts/brisk-common.sh"
case ${1:-} in -h|--help) echo "usage: $0   (no arguments; settings come from the environment, see the top of this script)"; exit 0 ;; esac
[ $# -eq 0 ] || die "usage: $0 (no arguments; settings come from the environment)"
# Two hostds would fight over 8443 (and the service's restart would undo brisk-down).
if systemctl --user is-active -q brisk-hostd.service 2>/dev/null; then
  die "hostd is running as a service: use scripts/brisk-service.sh (status, logs, link)"
fi
if systemctl --user is-enabled -q brisk-hostd.service 2>/dev/null; then
  die "brisk-hostd.service is enabled (it would start again and fight over 8443). To use brisk-up: systemctl --user disable --now brisk-hostd (or scripts/brisk-service.sh uninstall)"
fi
# Checked before anything else: a bad address would only fail in the bridge, after Sunshine
# was already switched and restarted.
if [ -n "${BRISK_LISTEN:-}" ]; then
  [[ $BRISK_LISTEN =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}:[0-9]{1,5}$ ]] || die "listen address '$BRISK_LISTEN' is not IPv4:PORT"
  [ "${BRISK_LISTEN%:*}" != 0.0.0.0 ] || die "refusing to listen on all interfaces (0.0.0.0)"
elif [ -z "$(tailscale ip -4 2>/dev/null | head -1 || true)" ]; then
  echo "brisk-up: tailscale has no IPv4 now; streams fail until it is up (hostd asks at each start)" >&2
fi
mkdir -p "$RUN" && chmod 700 "$RUN"

# shellcheck source=scripts/capped.sh
. "$here/scripts/capped.sh"

pid_alive "$RUN/hostd.pid" && die "hostd is already running (pid $(cat "$RUN/hostd.pid"))"

# 1. Build (no-op when up to date) inside the same memory-capped scope as the Sunshine build.
cap_check || die "no enforced resource cap; not building"
build_release "$here"
if [ ! -f "$here/web/dist/index.html" ]; then
  ( cd "$here/web" && capped npm ci --no-audit --no-fund && capped npm run build )
fi

# 2. (The bridge certificate is hostd's job now: ~/.config/brisk/tls, ECDSA P-256, 13 days,
#    renewed at a stream start once older than 10 days; see ensure_cert in stream.rs.)

# 3. Patched Sunshine (sunshine-brisk.sh refuses unless Sunshine is idle: FREE, no app running).
if ! brisk_running; then
  patched_sunshine_on # the caps are already set, so install needs no root
  for _ in $(seq 30); do sunshine_idle && break; sleep 1; done
fi
sunshine_idle || die "Sunshine is not idle (busy, an app running, or not answering); not launching"

# 4. hostd (page + API on loopback; the page line with the secret comes from hostd itself).
trap hostd_exit_cleanup EXIT
cat <<MSG
On the laptop:   ssh -N -L $PORT:127.0.0.1:$PORT ${HOSTNAME:-$(uname -n)}
Then open the "page" URL below in Chrome (the secret is kept in ~/.config/brisk/secret, so the link
stays the same across runs). Stop: Ctrl-C here (sent on as SIGTERM), then scripts/brisk-down.sh.
MSG
# Background jobs of a script start with SIGINT ignored, so pass Ctrl-C (and TERM/HUP) on as
# SIGTERM. Set before the launch, so a Ctrl-C right after it still reaches hostd; $! is unset
# until then (no other background job here), and a Ctrl-C before the launch just ends the script.
trap 'if [ -n "${!:-}" ]; then kill -TERM "$!" 2>/dev/null; else exit 130; fi' INT TERM HUP
BRISK_APP=$APP BRISK_BRIDGE_BIN=$here/target/release/brisk-bridge \
  "$here/target/release/brisk-hostd" "$here/web/dist" &
echo $! > "$RUN/hostd.pid"
# The first wait returns as soon as a trapped signal comes in; the second waits for hostd to exit.
wait $! || true
wait $! 2>/dev/null || true
