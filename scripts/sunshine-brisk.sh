#!/bin/bash
# Build/install/enable/rollback the brisk-patched Sunshine side by side with the stock package.
# Usage: sunshine-brisk.sh build|install|enable|rollback|status
# Env overrides (used by the test): SB_PREFIX SB_SRC SB_DROPIN_DIR SB_SERVERINFO SB_PATCH
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
UNIT=app-dev.lizardbyte.app.Sunshine.service
TAG=v2026.914.233613
PREFIX=${SB_PREFIX:-$HOME/.local/opt/sunshine-brisk}
SRC=${SB_SRC:-$HOME/build/sunshine-brisk-build}
DROPIN_DIR=${SB_DROPIN_DIR:-$HOME/.config/systemd/user/$UNIT.d}
PATCH=${SB_PATCH:-$here/patches/sunshine/0001-brisk-tap.patch}
URL=${SB_SERVERINFO:-http://127.0.0.1:47989/serverinfo}
BIN=$PREFIX/bin/sunshine
die() { echo "sunshine-brisk: $*" >&2; exit 1; }

serverinfo() { curl -s --max-time 5 "$URL" || true; }

# Never restart over a live session; an unanswered server (already broken) is fine to restart.
require_idle() {
  local s; s=$(serverinfo)
  [ -z "$s" ] && { echo "serverinfo: no answer (restart allowed)"; return; }
  case $s in *SUNSHINE_SERVER_FREE*"<currentgame>0</currentgame>"*|*"<currentgame>0</currentgame>"*SUNSHINE_SERVER_FREE*) ;;
    *) die "Sunshine is busy (not FREE / currentgame != 0); refusing to restart" ;; esac
}

restart() { require_idle; systemctl --user daemon-reload; systemctl --user restart "$UNIT"; }

# Resource cap: an OOM kills the build, not the box. Never raise JOBS above 6 (bawn hung at unbounded -j).
JOBS=${SB_JOBS:-6}
[ "$JOBS" -ge 1 ] && [ "$JOBS" -le 6 ] || die "SB_JOBS must be 1..6"
capped() {
  systemd-run --user --scope -q -p MemoryHigh=7G -p MemoryMax=9G -p MemorySwapMax=0 -p CPUWeight=20 \
    nice -n 19 ionice -c3 "$@"
}

cmd_build() {
  if [ ! -d "$SRC/.git" ]; then
    git clone --branch "$TAG" --recursive --depth 1 https://github.com/LizardByte/Sunshine "$SRC"
  fi
  # apply the patch once; skip if already applied
  if git -C "$SRC" apply --reverse --check "$PATCH" 2>/dev/null; then echo "patch already applied"
  else git -C "$SRC" apply "$PATCH"; fi
  capped cmake -S "$SRC" -B "$SRC/build" -DCMAKE_INSTALL_PREFIX="$PREFIX" -DSUNSHINE_ASSETS_DIR=share/sunshine \
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
  printf '[Service]\nExecStart=\nExecStart=%%h/.local/opt/sunshine-brisk/bin/sunshine\n' > "$DROPIN_DIR/brisk.conf"
  echo "wrote $DROPIN_DIR/brisk.conf"
}

cmd_enable() { [ -f "$DROPIN_DIR/brisk.conf" ] || die "no brisk.conf; run install first"; restart; }
cmd_rollback() { rm -f "$DROPIN_DIR/brisk.conf"; restart; }

cmd_status() {
  systemctl --user show -p ExecStart "$UNIT"
  if [ -n "$(serverinfo)" ]; then echo "serverinfo: answers"; else echo "serverinfo: NO ANSWER"; fi
}

case ${1:-} in
  build|install|enable|rollback|status) "cmd_$1" ;;
  *) die "usage: $0 build|install|enable|rollback|status" ;;
esac
