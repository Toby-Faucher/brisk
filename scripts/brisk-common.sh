# Shared by brisk-up.sh, brisk-down.sh, brisk-service.sh and sunshine-brisk.sh (sourced). The
# caller sets ME (its name in messages) and, for the pid and restart helpers, RUN.
# Env overrides (tests): SB_PREFIX SB_DROPIN_DIR SB_SERVERINFO BRISK_RESTART_GAP.
SUNSHINE_UNIT=app-dev.lizardbyte.app.Sunshine.service
SUNSHINE_PREFIX=${SB_PREFIX:-$HOME/.local/opt/sunshine-brisk}
SUNSHINE_BIN=$SUNSHINE_PREFIX/bin/sunshine
SERVERINFO_URL=${SB_SERVERINFO:-http://127.0.0.1:47989/serverinfo}
DROPIN_DIR=${SB_DROPIN_DIR:-$HOME/.config/systemd/user/$SUNSHINE_UNIT.d}
BRISK_DROPIN=$DROPIN_DIR/brisk.conf

die() { echo "${ME:-brisk}: $*" >&2; exit 1; }

serverinfo() { curl -s --max-time 5 "$SERVERINFO_URL" || true; }
# Idle = FREE and no app running (currentgame 0). No answer counts as not idle.
# $1: serverinfo text already fetched (default: ask now).
sunshine_idle() {
  local s=${1-$(serverinfo)}
  [[ $s == *'<state>SUNSHINE_SERVER_FREE</state>'* && $s == *'<currentgame>0</currentgame>'* ]]
}
# The Sunshine unit runs the patched binary.
brisk_running() { systemctl --user show -p ExecStart "$SUNSHINE_UNIT" 2>/dev/null | grep -qF "path=$SUNSHINE_BIN"; }

# The pid in file $1 is a live brisk-hostd. Checked by name too: after a crash or reboot the
# file can hold a pid that now belongs to some other process.
pid_alive() {
  local pid
  pid=$(cat "$1" 2>/dev/null) || return 1
  [[ $pid =~ ^[0-9]+$ ]] && [ "$(cat "/proc/$pid/comm" 2>/dev/null)" = brisk-hostd ]
}

# Sunshine restarts at least BRISK_RESTART_GAP s apart (default 100; the last one is remembered
# in $RUN/last-restart).
spaced_restart() {
  local last now gap=${BRISK_RESTART_GAP:-100} wait
  last=$(cat "$RUN/last-restart" 2>/dev/null || echo 0); now=$(date +%s); wait=$((last + gap - now))
  if [ "$wait" -gt 0 ]; then echo "waiting ${wait}s (Sunshine restarts stay $gap s apart)"; sleep "$wait"; fi
  "$@"
  date +%s > "$RUN/last-restart"
}
# brisk-up's EXIT trap. After Ctrl-C hostd's bridge is still cancelling its Sunshine session:
# keep hostd.pid until hostd is gone (up to $BRISK_EXIT_WAIT s, default 90; the cancel retries
# can take 45 s), so brisk-down waits for it instead of restarting Sunshine under it.
hostd_exit_cleanup() {
  [ -f "$RUN/hostd.pid" ] || return 0
  for _ in $(seq "${BRISK_EXIT_WAIT:-90}"); do pid_alive "$RUN/hostd.pid" || break; sleep 1; done
  pid_alive "$RUN/hostd.pid" || rm -f "$RUN/hostd.pid"
}
