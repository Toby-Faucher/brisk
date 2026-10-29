#!/bin/bash
# brisk-hostd as a systemd user service, reached from the tailnet through `tailscale serve`.
# Never uses sudo: a step that needs root is printed as a `sudoq run ...` command for Toby.
# Usage: scripts/brisk-service.sh install|update|status|logs|link|uninstall|serve-on|serve-off|
#                                 sunshine-permanent|doctor   (-h prints this usage)
# The service runs copies of the binaries in target/service/bin, which update replaces only
# together with the restart (so cargo can rebuild target/release under a live hostd).
# Env (tests): BRISK_ROOT (where target/ and web/dist are; default this checkout), BRISK_UNIT_DIR,
#   BRISK_SB, SB_DROPIN_DIR, BRISK_TS_NAME (skip the tailscale name lookup), FORCE=1 (update:
#   restart even while a stream is live).
set -euo pipefail
src=$(cd "$(dirname "$0")/.." && pwd)
here=${BRISK_ROOT:-$src}
SVC_BIN=$here/target/service/bin
UNIT=brisk-hostd.service
UNIT_DIR=${BRISK_UNIT_DIR:-$HOME/.config/systemd/user}
CONF=$HOME/.config/brisk
RUN=${XDG_RUNTIME_DIR:?}/brisk
SB=${BRISK_SB:-$src/scripts/sunshine-brisk.sh}
SERVE=(tailscale serve --bg --https=8443 http://127.0.0.1:8443)
SERVE_OFF=(tailscale serve --https=8443 off)
USAGE="usage: $0 install|update|status|logs|link|uninstall|serve-on|serve-off|sunshine-permanent|doctor"
ME=brisk-service
# shellcheck source=scripts/brisk-common.sh
. "$src/scripts/brisk-common.sh"
mkdir -p -m 700 "$RUN"

# Self.DNSName only (peers have one too): the first DNSName inside the top-level "Self" block,
# trailing dot dropped, and only if it looks like a host name.
ts_name() {
  [ -n "${BRISK_TS_NAME:-}" ] && { echo "$BRISK_TS_NAME"; return; }
  local n
  n=$(tailscale status --json 2>/dev/null | awk '
    /^  "Self": *\{/ { s = 1; next }
    s && /^  [}]/ { exit }
    s && match($0, /"DNSName": *"[^"]*"/) {
      v = substr($0, RSTART, RLENGTH); sub(/^"DNSName": *"/, "", v); sub(/\.?"$/, "", v); print v; exit
    }')
  [[ $n =~ ^[A-Za-z0-9.-]+$ ]] && echo "$n"
  return 0
}
# Header file for curl, so the secret never shows in a process list.
secret_header() {
  [ -r "$CONF/secret" ] || return 1
  ( umask 077; printf 'x-brisk-secret: %s\n' "$(tr -d '[:space:]' < "$CONF/secret")" > "$RUN/doctor.hdr" )
  echo "$RUN/doctor.hdr"
}
is_operator() { tailscale debug prefs 2>/dev/null | grep -q "\"OperatorUser\": *\"$(id -un)\""; }
# Our mapping exactly: a Web entry "<name>:8443" whose handler proxies to http://127.0.0.1:8443
# (the root entry "<name>:443" -> orch must not count, nor another port pointing at us).
serve_mapped() {
  tailscale serve status --json 2>/dev/null | awk '
    match($0, /"[^"]+:[0-9]+": *\{/) { host = substr($0, RSTART + 1, RLENGTH); sub(/".*/, "", host) }
    /"Proxy": *"http:\/\/127\.0\.0\.1:8443\/?"/ && host ~ /:8443$/ { found = 1 }
    END { exit !found }'
}
# May the running hostd be restarted? Only when its API answers "idle". Live, starting, no
# answer, no secret: no (fail closed); FORCE=1 overrides. Prints why when it refuses.
restart_ok() {
  [ "${FORCE:-}" = 1 ] && return 0
  local h st=""
  if h=$(secret_header); then st=$(curl -s --max-time 5 -H "@$h" http://127.0.0.1:8443/api/stream || true); rm -f "$h"; fi
  case $st in
    *'"state":"idle"'*) return 0 ;;
    *'"state":"live"'*|*'"state":"starting"'*) echo "a stream is live" ;;
    *) echo "hostd's API did not answer (stream state unknown)" ;;
  esac
  return 1
}
# Builds the page into a new web/dist.<stamp> (printed); nothing is swapped yet. A failed or
# empty build is removed, so it never counts as a build.
build_web() {
  local w=$here/web new
  new=$w/dist.$(date +%Y%m%d-%H%M%S)
  [ -e "$new" ] && new=$new-$$
  if ! ( cd "$w" && capped npm run build -- --outDir "$new" ) >&2; then rm -rf "$new"; die "web build failed; web/dist unchanged"; fi
  [ -f "$new/index.html" ] || { rm -rf "$new"; die "the web build made no index.html; web/dist unchanged"; }
  echo "$new"
}
# web/dist is a symlink to a build dir, renamed over the old link (atomic), so hostd never serves
# a half-written build. Keeps the live build and the most recent other one (by mtime).
swap_web() {
  local w=$here/web new=$1 live d kept=0
  # One-time move of a plain dist dir (brisk-up's build) aside, under a name of its own.
  if [ -d "$w/dist" ] && [ ! -L "$w/dist" ]; then mv -T "$w/dist" "$w/dist.plain-$(date +%s)-$$"; fi
  ln -sfn "$(basename "$new")" "$w/dist.link"
  mv -T "$w/dist.link" "$w/dist"
  echo "web/dist -> $(basename "$new")"
  live=$(readlink -f "$w/dist")
  # Newest first by mtime, so a clock step or an odd name never decides; the live build is never touched.
  while IFS= read -r d; do
    [ "$(readlink -f "$d")" = "$live" ] && continue
    if [ -f "$d/index.html" ] && [ $kept = 0 ]; then kept=1; continue; fi
    rm -rf "$d"
  done < <(find "$w" -maxdepth 1 -name 'dist.*' -type d -printf '%T@ %p\n' | sort -rn | cut -d' ' -f2-)
}
# The service's binaries live in target/service/bin under fixed names (hostd puts its own path
# in the Steam launch line, so it must not change). An update copies them in as *.new and
# renames them over the live ones (atomic; the running hostd keeps its file) only together with
# the restart; the replaced ones stay as *.prev for a rollback.
BINS=(brisk-hostd brisk-bridge)
stage_bin() {
  local b
  mkdir -p "$SVC_BIN" || die "could not make $SVC_BIN; service unchanged"
  for b in "${BINS[@]}"; do
    cp -p "$here/target/release/$b" "$SVC_BIN/$b.new" || { unstage_bin; die "could not copy $b; service unchanged"; }
  done
}
unstage_bin() { local b; for b in "${BINS[@]}"; do rm -f "$SVC_BIN/$b.new"; done; }
swap_bin() {
  local b
  for b in "${BINS[@]}"; do
    [ -e "$SVC_BIN/$b" ] && ln -f "$SVC_BIN/$b" "$SVC_BIN/$b.prev"
    mv -T "$SVC_BIN/$b.new" "$SVC_BIN/$b"
  done
  echo "target/service/bin: new binaries in place"
}
# Puts the *.prev binaries back; fails when there are none.
unswap_bin() {
  local b
  for b in "${BINS[@]}"; do [ -e "$SVC_BIN/$b.prev" ] || return 1; done
  for b in "${BINS[@]}"; do mv -T "$SVC_BIN/$b.prev" "$SVC_BIN/$b"; done
  echo "target/service/bin: previous binaries restored"
}
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
  [ -f "$here/web/dist/index.html" ] || die "$here/web/dist missing; run: $0 update (or npm run build in web/)"
  if pid_alive "$RUN/hostd.pid"; then
    die "a manual brisk-up hostd is running (pid $(cat "$RUN/hostd.pid")); stop it first (Ctrl-C there)"
  fi
  # The first install (or one from before target/service) copies the release binaries in.
  if [ ! -x "$SVC_BIN/brisk-hostd" ] || [ ! -x "$SVC_BIN/brisk-bridge" ]; then
    for b in brisk-hostd brisk-bridge; do
      [ -x "$here/target/release/$b" ] || die "$here/target/release/$b missing; run: $0 update (or cargo build --release)"
    done
    stage_bin; swap_bin
  fi
  mkdir -p "$UNIT_DIR"
  # The template has @ROOT@ where this checkout's path goes. Bash substitution, no sed; the
  # quoted replacement keeps a & in the path literal (bash 5.2 patsub_replacement).
  local tmp changed=0 was=0 unit; tmp=$(mktemp)
  unit=$(<"$src/deploy/$UNIT")
  printf '%s\n' "${unit//@ROOT@/"$here"}" > "$tmp"
  local pending=0 why
  systemctl --user is-active -q "$UNIT" && was=1
  if cmp -s "$tmp" "$UNIT_DIR/$UNIT"; then
    rm -f "$tmp"
    # Same file, but the running process may predate it (an earlier install that could not restart).
    if [ $was = 1 ] && unit_newer_than_service; then pending=1; echo "unit installed but service not restarted yet"
    else echo "unit unchanged"; fi
  else changed=1; fi
  # Over a live stream nothing is written and the manager is not reloaded: a reload could hand the
  # running process the new unit's settings (e.g. a watchdog it never pings).
  if [ $was = 1 ] && [ $((changed + pending)) -gt 0 ] && ! why=$(restart_ok); then
    rm -f "$tmp"
    if [ $changed = 1 ]; then die "unit changed; not installed while $why; run $0 install again when idle (or FORCE=1)"; fi
    die "unit installed but service not restarted yet; not restarting while $why; run $0 install again when idle (or FORCE=1)"
  fi
  if [ $changed = 1 ]; then
    # The previous unit is kept as .bak: a new unit that does not come up is rolled back to it.
    [ -f "$UNIT_DIR/$UNIT" ] && cp -p "$UNIT_DIR/$UNIT" "$UNIT_DIR/$UNIT.bak"
    mv "$tmp" "$UNIT_DIR/$UNIT"; chmod 644 "$UNIT_DIR/$UNIT"; echo "wrote $UNIT_DIR/$UNIT"
  fi
  systemctl --user daemon-reload
  # Type=notify: start blocks until READY and fails on a timeout; judged below, not by set -e.
  local started=1 tried=$((changed + pending))
  systemctl --user enable --now "$UNIT" || started=0
  # enable --now leaves a running unit alone: a changed unit needs a restart to apply.
  if [ $was = 1 ] && [ $tried -gt 0 ]; then
    systemctl --user restart "$UNIT" || true; echo "restarted $UNIT (unit changed)"
  fi
  if [ $tried -gt 0 ] && ! came_up; then rollback; fi
  [ $started = 1 ] || die "$UNIT failed to start ($0 logs)"
  echo "enabled and started $UNIT. Next: $0 serve-on, then $0 doctor"
}

# Up = systemd says active (with Type=notify: hostd sent READY) and 127.0.0.1:8443 answers HTTP
# (403 is fine). Waits up to BRISK_UP_WAIT seconds (45).
came_up() {
  local i code
  for ((i = 0; i < ${BRISK_UP_WAIT:-45}; i++)); do
    if systemctl --user is-active -q "$UNIT"; then
      code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 2 http://127.0.0.1:8443/api/host || true)
      [[ $code =~ ^[1-5][0-9][0-9]$ ]] && return 0
    fi
    sleep 1
  done
  return 1
}

# The unit file is newer than the running process (whole seconds; same second = applied).
unit_newer_than_service() {
  local t
  t=$(systemctl --user show -p ActiveEnterTimestamp --timestamp=unix "$UNIT" 2>/dev/null | sed -n 's/^ActiveEnterTimestamp=@//p')
  [[ $t =~ ^[0-9]+$ ]] && [ "$(stat -c %Y "$UNIT_DIR/$UNIT")" -gt "$t" ]
}

# The new unit did not come up: put the previous one back and start it. Without a previous unit
# (a first install) it is disabled, so it cannot restart-loop at boot.
rollback() {
  local b=$UNIT_DIR/$UNIT.bak
  if [ ! -f "$b" ]; then
    systemctl --user disable --now "$UNIT" || true
    die "new unit failed to come up and there is no previous unit ($b) to restore; $UNIT disabled; see $0 logs"
  fi
  cp -p "$b" "$UNIT_DIR/$UNIT"
  systemctl --user daemon-reload
  systemctl --user restart "$UNIT" || true
  came_up || die "new unit failed; previous unit restored but it did not come up either (see $0 logs)"
  die "new unit failed; previous unit restored (see $0 logs)"
}

# Rebuilds what changed, then restarts the service (not over a live stream unless FORCE=1).
cmd_update() {
  # shellcheck source=scripts/capped.sh
  . "$src/scripts/capped.sh"
  # A bumped rust-toolchain.toml would otherwise download inside the capped nice-19 build with no
  # timeout; fetch it first, and stop before anything is built, swapped or restarted if that fails.
  ( cd "$here" && PATH=$HOME/.cargo/bin:$PATH timeout "${BRISK_RUSTUP_TIMEOUT:-900}" rustup toolchain install --no-self-update ) \
    || die "pinned toolchain not installed; service unchanged"
  cap_check || die "no enforced resource cap; not building"
  ( cd "$here" && PATH=$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=4 capped cargo build --release -q -p brisk-hostd -p brisk-bridge )
  local w=$here/web
  if [ ! -f "$w/node_modules/.package-lock.json" ] || [ "$w/package-lock.json" -nt "$w/node_modules/.package-lock.json" ]; then
    ( cd "$w" && capped npm ci --no-audit --no-fund )
  fi
  local new="" why
  if [ ! -f "$w/dist/index.html" ] || [ -n "$(find "$w/src" "$w/public" "$w/package-lock.json" "$w"/astro.config.* -newer "$w/dist/index.html" 2>/dev/null | head -1)" ]; then
    new=$(build_web) || exit 1
  else
    echo "web/dist is up to date"
  fi
  stage_bin
  if ! systemctl --user is-active -q "$UNIT"; then
    swap_bin
    [ -z "$new" ] || swap_web "$new"
    echo "$UNIT is not running; built only (install starts it)"; return
  fi
  # Binaries and page are swapped only together with the restart: never a new page against the
  # old API, and never new binaries under a running hostd.
  if ! why=$(restart_ok); then
    unstage_bin
    [ -z "$new" ] || rm -rf "$new"
    echo "kept the running binaries and page (the new builds are dropped)"
    die "$why; not restarting (FORCE=1 to end it); run update again later"
  fi
  local oldweb; oldweb=$(readlink "$here/web/dist" || true)
  swap_bin
  [ -z "$new" ] || swap_web "$new"
  systemctl --user restart "$UNIT" || true
  if came_up; then echo "restarted $UNIT"; return; fi
  # The new build did not come up: put the previous binaries (and page) back and restart.
  unswap_bin || die "$UNIT did not come up on the new build and there is no previous build to restore; see $0 logs"
  [ -z "$new" ] || [ -z "$oldweb" ] || swap_web "$here/web/$oldweb"
  systemctl --user restart "$UNIT" || true
  came_up || die "new build failed; previous build restored but it did not come up either (see $0 logs)"
  die "new build failed to come up; previous build restored (see $0 logs)"
}

cmd_status() {
  systemctl --user status --no-pager "$UNIT" || true
  local n; n=$(ts_name)
  echo "page: https://${n:-<tailscale name>}:8443/  (and http://localhost:8443/ over ssh -L)"
  if serve_mapped; then echo "tailscale serve: :8443 mapped"; else echo "tailscale serve: :8443 NOT mapped ($0 serve-on)"; fi
}

# hostd never logs the secret under systemd; the filter is a second line of defence for transcripts.
cmd_logs() {
  journalctl --user -u "$UNIT" --no-pager "${@:--n200}" \
    | sed -u -E -e 's/([?&]s=)[^[:space:]&#]+/\1<hidden>/g' -e 's/([?&]wt=)[^[:space:]&#]+/\1<hidden>/g'
}

# The links with the secret (for a browser that has none and no allowed tailnet login).
cmd_link() {
  [ -r "$CONF/secret" ] || die "no $CONF/secret yet (hostd makes it on its first start)"
  local s n; s=$(tr -d '[:space:]' < "$CONF/secret"); n=$(ts_name)
  echo "page	https://${n:-<tailscale name>}:8443/?s=$s"
  echo "page	http://localhost:8443/?s=$s"
}

cmd_uninstall() {
  systemctl --user disable --now "$UNIT" 2>/dev/null || true
  rm -f "$UNIT_DIR/$UNIT" "$UNIT_DIR/$UNIT.bak"
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
  # The same test sunshine-brisk.sh enable uses, so a refused enable never strands the drop-in.
  sunshine_idle || die "Sunshine is not idle (busy, an app running, or not answering); nothing changed"
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
  local sh st wd
  sh=$(systemctl --user show -p Type -p SubState -p StatusText -p WatchdogUSec "$UNIT" 2>/dev/null || true)
  if grep -qx 'Type=notify' <<<"$sh"; then ok "Type=notify"; else no "$UNIT is not Type=notify ($0 install)"; fi
  st=$(sed -n 's/^StatusText=//p' <<<"$sh")
  if grep -qx 'SubState=running' <<<"$sh" && [ -n "$st" ]; then ok "running: $st"
  else no "not running with a status text ($(grep '^SubState=' <<<"$sh"), status '$st')"; fi
  wd=$(sed -n 's/^WatchdogUSec=//p' <<<"$sh")
  # WatchdogSec=60 shows as 1min.
  if [ "$wd" = 1min ]; then ok "watchdog 60s"; else no "watchdog '$wd' on $UNIT, want 60s (WatchdogSec=60; $0 install)"; fi
  # tailscaled (TUN mode) binds its serve ports on tailscale0 (`100.x%tailscale0:8443`, `[fd7a:…]%tailscale0:8443`):
  # those are fine. Anything else besides hostd's 127.0.0.1:8443 (0.0.0.0, [::], *, a LAN address) is not.
  l=$(ss -Hltn 'sport = :8443' 2>/dev/null | awk '{print $4}')
  local beyond; beyond=$(grep -v -e '^127\.0\.0\.1:8443$' -e '%tailscale0:8443$' <<<"$l" | grep . || true)
  if ! grep -qx '127\.0\.0\.1:8443' <<<"$l"; then no "hostd is not listening on 127.0.0.1:8443"
  elif [ -n "$beyond" ]; then no "8443 listens beyond loopback: $(echo "$beyond" | tr '\n' ' ')"
  else ok "8443 on loopback only (plus tailscaled on tailscale0)"; fi
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
  if sunshine_idle; then ok "Sunshine idle"; else info "Sunshine not idle (a stream or app may be on, or it is down)"; fi
  return $bad
}

case ${1:-} in
  install|update|status|logs|link|uninstall|doctor) c=$1; shift; "cmd_$c" "$@" ;;
  serve-on) cmd_serve_on ;;
  serve-off) cmd_serve_off ;;
  sunshine-permanent) cmd_sunshine_permanent ;;
  -h|--help) echo "$USAGE" ;;
  *) die "$USAGE" ;;
esac
