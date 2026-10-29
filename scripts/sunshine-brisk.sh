#!/bin/bash
# Build/install/enable/rollback the brisk-patched Sunshine side by side with the stock package.
# Usage: sunshine-brisk.sh build|install|enable|rollback|status   (-h prints this usage)
# Env overrides (used by the test): SB_PREFIX SB_SRC SB_DROPIN_DIR SB_SERVERINFO SB_PATCH
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck disable=SC2034 # read by die in brisk-common.sh
ME=sunshine-brisk
# shellcheck source=scripts/brisk-common.sh
. "$here/scripts/brisk-common.sh"
TAG=v2026.914.233613
SRC=${SB_SRC:-$HOME/build/sunshine-brisk-build}
PATCH=${SB_PATCH:-$here/patches/sunshine/0001-brisk-tap.patch}
UNIT=$SUNSHINE_UNIT BIN=$SUNSHINE_BIN
usage() { echo "usage: $0 build|install|enable|rollback|status"; }

# Never restart over a live session. No answer = busy-unknown: refuse unless FORCE=1
# (rollback passes ALLOW_DEAD=1: a dead server has no session to lose).
require_idle() {
  local s; s=$(serverinfo)
  if [ -z "$s" ]; then
    [ "${FORCE:-}" = 1 ] || [ "${ALLOW_DEAD:-}" = 1 ] || die "serverinfo gives no answer (busy unknown); set FORCE=1 to restart anyway"
    echo "serverinfo: no answer (restart allowed)"; return
  fi
  serverinfo_idle "$s" && return
  if [ "${BUSY_OK:-}" = 1 ]; then echo "WARNING: Sunshine is busy; FORCE=1 will END THE LIVE STREAM" >&2; return; fi
  die "Sunshine is busy (not FREE / currentgame != 0); refusing to restart"
}

# reset-failed first so a crashed brisk unit cannot block the restart with the start-rate limit
restart() {
  require_idle
  systemctl --user reset-failed "$UNIT" 2>/dev/null || true
  systemctl --user daemon-reload
  systemctl --user restart "$UNIT"
}

# Never raise JOBS above 6 (bawn hung at unbounded -j).
JOBS=${SB_JOBS:-3}
# shellcheck source=scripts/capped.sh
. "$here/scripts/capped.sh"

cmd_build() {
  case $JOBS in ''|*[!0-9]*) die "SB_JOBS must be 1..6" ;; esac
  { [ "$JOBS" -ge 1 ] && [ "$JOBS" -le 6 ]; } || die "SB_JOBS must be 1..6"
  brisk_running && die "unit runs the brisk binary; run rollback before rebuilding (text file busy)"
  case $(realpath -m "$SRC")/ in "$(realpath -m "$HOME/recon-pinned")"/*)
    die "$SRC is under ~/recon-pinned (read-only reference); build from a fresh clone" ;; esac
  cap_check || die "no enforced resource cap"
  if [ ! -d "$SRC/.git" ]; then
    git clone --branch "$TAG" --recursive --depth 1 https://github.com/LizardByte/Sunshine "$SRC"
  fi
  # A stale checkout at another tag would patch the wrong base.
  local at; at=$(git -C "$SRC" describe --tags --exact-match 2>/dev/null || true)
  [ "$at" = "$TAG" ] || die "$SRC is at '${at:-no exact tag}', not $TAG; remove it or point SB_SRC at a fresh clone"
  # apply the patch once; skip if already applied
  if git -C "$SRC" apply --reverse --check "$PATCH" 2>/dev/null; then echo "patch already applied"
  else git -C "$SRC" apply "$PATCH"; fi
  capped cmake -S "$SRC" -B "$SRC/build" -DCMAKE_INSTALL_PREFIX="$SUNSHINE_PREFIX" -DSUNSHINE_ASSETS_DIR=share/sunshine \
    -DSUNSHINE_EXECUTABLE_PATH="$BIN" -DBUILD_TESTS=OFF -DBUILD_DOCS=OFF -DSUNSHINE_ENABLE_CUDA=OFF \
    -DCUDA_FAIL_ON_MISSING=OFF -DBUILD_WERROR=OFF \
    -DCMAKE_DISABLE_FIND_PACKAGE_Udev=ON -DCMAKE_DISABLE_FIND_PACKAGE_Systemd=ON -DCMAKE_BUILD_TYPE=Release
  capped cmake --build "$SRC/build" -j"$JOBS"
  capped cmake --install "$SRC/build"
}

cmd_install() {
  [ -x "$BIN" ] || die "$BIN missing; run build first"
  case $(getcap "$BIN" 2>/dev/null) in *cap_sys_admin*cap_sys_nice*|*cap_sys_nice*cap_sys_admin*) echo "caps already set" ;;
    *) sudoq run --why "KMS capture caps for patched sunshine" -- setcap cap_sys_admin,cap_sys_nice+p "$BIN" ;; esac
  mkdir -p "$DROPIN_DIR"
  # systemd reads % as a specifier: double it.
  printf '[Service]\nExecStart=\nExecStart=%s\n' "${BIN//%/%%}" > "$DROPIN_DIR/brisk.conf"
  echo "wrote $DROPIN_DIR/brisk.conf"
}

cmd_enable() {
  [ -f "$DROPIN_DIR/brisk.conf" ] || die "no brisk.conf; run install first"
  if brisk_running && systemctl --user is-active -q "$UNIT"; then echo "already running brisk binary; nothing to do"; return; fi
  restart
}
cmd_rollback() {
  if [ ! -f "$DROPIN_DIR/brisk.conf" ] && ! brisk_running; then echo "no brisk.conf and unit not on brisk; nothing to roll back"; return; fi
  # idle check BEFORE deleting anything, so a refused rollback changes nothing; FORCE=1 overrides busy (rollback only)
  ALLOW_DEAD=1 BUSY_OK=${FORCE:-} require_idle
  rm -f "$DROPIN_DIR/brisk.conf"
  ALLOW_DEAD=1 BUSY_OK=${FORCE:-} restart
}

cmd_status() {
  systemctl --user show -p ExecStart "$UNIT"
  if [ -n "$(serverinfo)" ]; then echo "serverinfo: answers"; else echo "serverinfo: NO ANSWER"; fi
}

case ${1:-} in
  build|install|enable|rollback|status) "cmd_$1" ;;
  -h|--help) usage ;;
  *) usage >&2; exit 1 ;;
esac
