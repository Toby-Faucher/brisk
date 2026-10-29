# shellcheck shell=bash
# Shared by brisk-up.sh, brisk-down.sh, brisk-service.sh and sunshine-brisk.sh (sourced, always by
# a path with a /). The caller sets ME (its name in messages) and, for the pid and restart
# helpers, RUN.
# Env overrides (tests): SB_PREFIX SB_DROPIN_DIR SB_SERVERINFO BRISK_RESTART_GAP BRISK_SB.
SUNSHINE_UNIT=app-dev.lizardbyte.app.Sunshine.service
SUNSHINE_PREFIX=${SB_PREFIX:-$HOME/.local/opt/sunshine-brisk}
SUNSHINE_BIN=$SUNSHINE_PREFIX/bin/sunshine
SERVERINFO_URL=${SB_SERVERINFO:-http://127.0.0.1:47989/serverinfo}
DROPIN_DIR=${SB_DROPIN_DIR:-$HOME/.config/systemd/user/$SUNSHINE_UNIT.d}
# shellcheck disable=SC2034 # used by the scripts that source this
BRISK_DROPIN=$DROPIN_DIR/brisk.conf
SB=${BRISK_SB:-${BASH_SOURCE[0]%/*}/sunshine-brisk.sh}

die() { echo "${ME:-brisk}: $*" >&2; exit 1; }

serverinfo() { curl -s --max-time 5 "$SERVERINFO_URL" || true; }
# Idle = FREE and no app running (currentgame 0). No answer counts as not idle.
# serverinfo_idle takes serverinfo text already fetched; sunshine_idle asks now.
serverinfo_idle() { [[ $1 == *'<state>SUNSHINE_SERVER_FREE</state>'* && $1 == *'<currentgame>0</currentgame>'* ]]; }
sunshine_idle() { serverinfo_idle "$(serverinfo)"; }
# The Sunshine unit runs the patched binary.
brisk_running() { systemctl --user show -p ExecStart "$SUNSHINE_UNIT" 2>/dev/null | grep -qF "path=$SUNSHINE_BIN"; }

# Patched Sunshine on: install writes the drop-in, enable restarts Sunshine. The idle test comes
# first and is the one enable uses, so a busy host never gets a drop-in that a later restart
# would pick up.
patched_sunshine_on() {
  sunshine_idle || die "Sunshine is not idle (busy, an app running, or not answering); nothing changed"
  "$SB" install
  spaced_restart "$SB" enable
}

# Release build of the bridge and hostd in checkout $1. The caller sources capped.sh and runs
# cap_check first. 4 jobs: bawn hung once at an unbounded -j.
build_release() {
  ( cd "$1" && PATH=$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=4 capped cargo build --release -q -p brisk-bridge -p brisk-hostd )
}

# The pid in file $1 is a live brisk-hostd. Checked by name too: after a crash or reboot the
# file can hold a pid that now belongs to some other process. Uses read, not cat: it runs in
# wait loops.
pid_alive() {
  local pid="" comm=""
  { read -r pid || [ -n "$pid" ]; } 2>/dev/null < "$1" || return 1
  [[ $pid =~ ^[0-9]+$ ]] && { read -r comm; } 2>/dev/null < "/proc/$pid/comm" && [ "$comm" = brisk-hostd ]
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
