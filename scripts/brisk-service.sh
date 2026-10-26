#!/bin/bash
# brisk-hostd as a systemd user service, reached from the tailnet through `tailscale serve`.
# Never uses sudo: a step that needs root is printed as a `sudoq run ...` command for Toby.
# Usage: scripts/brisk-service.sh install|update|status|logs|link|uninstall|serve-on|serve-off|
#                                 sunshine-permanent|doctor
# Env (tests): BRISK_ROOT (where target/ and web/dist are; default this checkout), BRISK_UNIT_DIR,
#   BRISK_SB, SB_DROPIN_DIR, BRISK_TS_NAME (skip the tailscale name lookup), FORCE=1 (update:
#   restart even while a stream is live).
set -euo pipefail
src=$(cd "$(dirname "$0")/.." && pwd)
here=${BRISK_ROOT:-$src}
UNIT=brisk-hostd.service
UNIT_DIR=${BRISK_UNIT_DIR:-$HOME/.config/systemd/user}
CONF=$HOME/.config/brisk
RUN=${XDG_RUNTIME_DIR:?}/brisk
SB=${BRISK_SB:-$src/scripts/sunshine-brisk.sh}
SERVE=(tailscale serve --bg --https=8443 http://127.0.0.1:8443)
SERVE_OFF=(tailscale serve --https=8443 off)
die() { echo "brisk-service: $*" >&2; exit 1; }
# shellcheck source=scripts/brisk-common.sh
. "$src/scripts/brisk-common.sh"
mkdir -p -m 700 "$RUN"

ts_name() {
  [ -n "${BRISK_TS_NAME:-}" ] && { echo "$BRISK_TS_NAME"; return; }
  tailscale status --json 2>/dev/null | sed -n 's/.*"DNSName": *"\([^"]*\)\.".*/\1/p' | head -1
}
# Header file for curl, so the secret never shows in a process list.
secret_header() {
  [ -r "$CONF/secret" ] || return 1
  ( umask 077; printf 'x-brisk-secret: %s\n' "$(tr -d '[:space:]' < "$CONF/secret")" > "$RUN/doctor.hdr" )
  echo "$RUN/doctor.hdr"
}
is_operator() { tailscale debug prefs 2>/dev/null | grep -q "\"OperatorUser\": *\"$(id -un)\""; }
serve_mapped() { tailscale serve status --json 2>/dev/null | grep -q 'http://127.0.0.1:8443'; }
# Runs a tailscale serve command if we may; else says why and prints the sudoq line for Toby.
ts_root() {
  local why=$1; shift
  if is_operator; then "$@"; return; fi
  cat <<MSG
'$*' needs root (toby is not the tailscale operator). Ask Toby to run:
  sudoq run --why "$why" -- $*
(or once: sudoq run --why "let toby manage tailscale serve" -- tailscale set --operator=$(id -un))
MSG
  return 2
}

cmd_install() {
  local b
  for b in brisk-hostd brisk-bridge; do
    [ -x "$here/target/release/$b" ] || die "$here/target/release/$b missing; run: $0 update (or cargo build --release)"
  done
  [ -f "$here/web/dist/index.html" ] || die "$here/web/dist missing; run: $0 update (or npm run build in web/)"
  if [ -f "$RUN/bridge.pid" ] && kill -0 "$(cat "$RUN/bridge.pid")" 2>/dev/null; then
    die "a manual brisk-up hostd is running (pid $(cat "$RUN/bridge.pid")); stop it first (Ctrl-C there)"
  fi
  mkdir -p "$UNIT_DIR"
  # The template points at ~/Projects/brisk; another checkout gets its own path written in.
  local tmp; tmp=$(mktemp)
  sed "s#%h/Projects/brisk#$here#g" "$src/deploy/$UNIT" > "$tmp"
  [ "$here" = "$HOME/Projects/brisk" ] || echo "note: the unit runs $here (not ~/Projects/brisk)"
  if cmp -s "$tmp" "$UNIT_DIR/$UNIT"; then echo "unit unchanged"; rm -f "$tmp"
  else mv "$tmp" "$UNIT_DIR/$UNIT"; chmod 644 "$UNIT_DIR/$UNIT"; echo "wrote $UNIT_DIR/$UNIT"; fi
  systemctl --user daemon-reload
  systemctl --user enable --now "$UNIT"
  echo "enabled and started $UNIT. Next: $0 serve-on, then $0 doctor"
}

# Rebuilds what changed, then restarts the service (not over a live stream unless FORCE=1).
cmd_update() {
  # shellcheck source=scripts/capped.sh
  . "$src/scripts/capped.sh"
  cap_check || die "no enforced resource cap; not building"
  ( cd "$here" && PATH=$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=4 capped cargo build --release -q -p brisk-hostd -p brisk-bridge )
  local w=$here/web
  if [ ! -f "$w/node_modules/.package-lock.json" ] || [ "$w/package-lock.json" -nt "$w/node_modules/.package-lock.json" ]; then
    ( cd "$w" && capped npm ci --no-audit --no-fund )
  fi
  if [ ! -f "$w/dist/index.html" ] || [ -n "$(find "$w/src" "$w/public" "$w/package-lock.json" "$w"/astro.config.* -newer "$w/dist/index.html" 2>/dev/null | head -1)" ]; then
    ( cd "$w" && capped npm run build )
  else
    echo "web/dist is up to date"
  fi
  systemctl --user is-active -q "$UNIT" || { echo "$UNIT is not running; built only (install starts it)"; return; }
  local h st=""
  if h=$(secret_header); then st=$(curl -s --max-time 5 -H "@$h" http://127.0.0.1:8443/api/stream || true); rm -f "$h"; fi
  case $st in *'"state":"live"'*|*'"state":"starting"'*)
    [ "${FORCE:-}" = 1 ] || die "a stream is live; not restarting (FORCE=1 to end it)" ;; esac
  systemctl --user restart "$UNIT"
  echo "restarted $UNIT"
}

cmd_status() {
  systemctl --user status --no-pager "$UNIT" || true
  local n; n=$(ts_name)
  echo "page: https://${n:-<tailscale name>}:8443/  (and http://localhost:8443/ over ssh -L)"
  if serve_mapped; then echo "tailscale serve: :8443 mapped"; else echo "tailscale serve: :8443 NOT mapped ($0 serve-on)"; fi
}

cmd_logs() { journalctl --user -u "$UNIT" --no-pager "${@:--n200}"; }

# The links with the secret (for a browser that has none and no allowed tailnet login).
cmd_link() {
  [ -r "$CONF/secret" ] || die "no $CONF/secret yet (hostd makes it on its first start)"
  local s n; s=$(tr -d '[:space:]' < "$CONF/secret"); n=$(ts_name)
  echo "page	https://${n:-<tailscale name>}:8443/?s=$s"
  echo "page	http://localhost:8443/?s=$s"
}

cmd_uninstall() {
  systemctl --user disable --now "$UNIT" 2>/dev/null || true
  rm -f "$UNIT_DIR/$UNIT"
  systemctl --user daemon-reload
  echo "removed $UNIT. Left as is: $CONF (secret, tls, allowed-users), the Sunshine drop-in, the serve mapping ($0 serve-off)"
}

cmd_serve_on() {
  if serve_mapped; then echo "tailscale serve: :8443 already mapped"; return; fi
  ts_root "Brisk page on the tailnet at :8443 (root mapping untouched)" "${SERVE[@]}"
}
cmd_serve_off() {
  if ! serve_mapped; then echo "tailscale serve: :8443 not mapped"; return; fi
  ts_root "remove the Brisk :8443 tailnet mapping (root mapping untouched)" "${SERVE_OFF[@]}"
}

# Patched Sunshine for good (the tap falls back to plain RTP without a bridge, so Moonlight works).
cmd_sunshine_permanent() {
  if brisk_running && [ -f "$BRISK_DROPIN" ]; then echo "patched Sunshine already runs with its drop-in"; return; fi
  serverinfo_free || die "Sunshine is not FREE (busy or not answering); nothing changed"
  "$SB" install
  spaced_restart "$SB" enable
}

cmd_doctor() {
  local bad=0 n h code l
  ok() { echo "ok    $*"; }
  no() { echo "FAIL  $*"; bad=1; }
  info() { echo "info  $*"; }
  if loginctl show-user "$(id -un)" -p Linger 2>/dev/null | grep -q '^Linger=yes'; then ok "linger on"; else no "linger off (sudoq run --why 'start brisk at boot' -- loginctl enable-linger $(id -un))"; fi
  if systemctl --user is-enabled -q "$UNIT" 2>/dev/null; then ok "$UNIT enabled"; else no "$UNIT not enabled ($0 install)"; fi
  if systemctl --user is-active -q "$UNIT" 2>/dev/null; then ok "$UNIT active"; else no "$UNIT not active ($0 logs)"; fi
  l=$(ss -Hltn 'sport = :8443' 2>/dev/null | awk '{print $4}')
  if [ -z "$l" ]; then no "nothing listens on :8443"
  elif echo "$l" | grep -qv '^127\.0\.0\.1:8443$'; then no "8443 listens beyond loopback: $(echo "$l" | tr '\n' ' ')"
  else ok "8443 on loopback only"; fi
  if serve_mapped; then ok "tailscale serve maps :8443"; else no "tailscale serve :8443 not mapped (or not readable) ($0 serve-on)"; fi
  n=$(ts_name)
  if [ -z "$n" ]; then no "no tailscale name (tailscale down?)"
  else
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 "https://$n:8443/api/host" || true)
    if [ "$code" = 403 ]; then ok "https://$n:8443 answers 403 without credentials"; else no "https://$n:8443/api/host without credentials: '$code' (want 403)"; fi
    if h=$(secret_header); then
      code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 -H "@$h" "https://$n:8443/api/host" || true); rm -f "$h"
      if [ "$code" = 200 ]; then ok "https://$n:8443 answers 200 with the secret"; else no "https://$n:8443/api/host with the secret: '$code' (want 200)"; fi
    else no "no $CONF/secret (hostd never started?)"; fi
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 -H 'x-brisk-csrf: 1' "https://$n:8443/api/host" || true)
    info "with this machine's own tailnet login (no secret): $code (200 only if it is in allowed-users)"
  fi
  local pem=$CONF/tls/bridge.pem d
  if [ -f "$pem" ]; then
    d=$(( ($(date +%s) - $(stat -c %Y "$pem")) / 86400 ))
    if [ "$d" -lt 13 ]; then ok "bridge certificate $d days old (renewed at a stream start after 10)"
    else info "bridge certificate $d days old: expired, hostd renews it at the next stream start"; fi
  else info "no bridge certificate yet (hostd makes it at the first stream start)"; fi
  if [ -f "$BRISK_DROPIN" ]; then ok "Sunshine drop-in present"; else no "no Sunshine drop-in ($0 sunshine-permanent)"; fi
  if brisk_running; then ok "Sunshine runs the patched binary"; else no "Sunshine runs the stock binary ($0 sunshine-permanent)"; fi
  if serverinfo_free; then ok "Sunshine FREE"; else info "Sunshine not FREE (a stream may be on, or it is down)"; fi
  return $bad
}

case ${1:-} in
  install|update|status|logs|link|uninstall|doctor) c=$1; shift; "cmd_$c" "$@" ;;
  serve-on) cmd_serve_on ;;
  serve-off) cmd_serve_off ;;
  sunshine-permanent) cmd_sunshine_permanent ;;
  *) die "usage: $0 install|update|status|logs|link|uninstall|serve-on|serve-off|sunshine-permanent|doctor" ;;
esac
