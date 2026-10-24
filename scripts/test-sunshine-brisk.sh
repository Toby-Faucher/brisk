#!/bin/bash
# Stubbed test for sunshine-brisk.sh install/enable/rollback/status; touches nothing real.
set -eu
s=$(cd "$(dirname "$0")" && pwd)/sunshine-brisk.sh
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/p/bin"; : > "$t/p/bin/sunshine"; chmod +x "$t/p/bin/sunshine"
for c in systemctl sudoq getcap systemd-run nice ionice git cmake nproc; do printf '#!/bin/sh\necho "%s $*" >> "%s/log"\n' "$c" "$t" > "$t/bin/$c"; done
printf '#!/bin/sh\necho "<currentgame>0</currentgame><state>${STATE:-SUNSHINE_SERVER_FREE}</state>"\n' > "$t/bin/curl"
chmod +x "$t"/bin/*
export PATH=$t/bin:$PATH SB_PREFIX=$t/p SB_DROPIN_DIR=$t/d
chk() { grep -qF -- "$1" "$2" || { echo "FAIL: '$1' not in $2"; cat "$2"; exit 1; }; }
$s install >/dev/null
chk "sudoq run --why KMS capture caps for patched sunshine -- setcap cap_sys_admin,cap_sys_nice+p $t/p/bin/sunshine" "$t/log"
printf '[Service]\nExecStart=\nExecStart=%%h/.local/opt/sunshine-brisk/bin/sunshine\n' | cmp - "$t/d/brisk.conf"
: > "$t/log"; $s enable >/dev/null; chk "systemctl --user daemon-reload" "$t/log"; chk "systemctl --user restart app-dev.lizardbyte.app.Sunshine.service" "$t/log"
: > "$t/log"; STATE=SUNSHINE_SERVER_BUSY $s enable >/dev/null 2>&1 && { echo "FAIL: busy should refuse"; exit 1; }
grep -q restart "$t/log" && { echo "FAIL: restarted while busy"; exit 1; }
$s rollback >/dev/null; [ ! -e "$t/d/brisk.conf" ] || { echo "FAIL: dropin remains"; exit 1; }
$s rollback >/dev/null  # idempotent
$s status | grep -q "serverinfo: answers" || { echo "FAIL: status"; exit 1; }
# build: capped scope, -j<=6
mkdir -p "$t/src/.git"; : > "$t/log"
printf '#!/bin/sh\necho "systemd-run $*" >> "%s/log"\n' "$t" > "$t/bin/systemd-run"
printf '#!/bin/sh\necho "cmake $*" >> "%s/log"\n' "$t" > "$t/bin/cmake"
printf '#!/bin/sh\nshift;shift;exec "$@"\n' > "$t/bin/ionice"; printf '#!/bin/sh\nshift;shift;exec "$@"\n' > "$t/bin/nice"
printf '#!/bin/sh\nexit 0\n' > "$t/bin/git"
SB_SRC=$t/src $s build >/dev/null 2>&1 || true
chk "-p MemoryHigh=7G -p MemoryMax=9G -p MemorySwapMax=0 -p CPUWeight=20 nice -n 19 ionice -c3 cmake --build $t/src/build -j6" "$t/log"
grep -E 'cmake --build' "$t/log" | grep -qE -- ' -j[1-6]$' || { echo "FAIL: -j"; exit 1; }
SB_JOBS=20 SB_SRC=$t/src $s build >/dev/null 2>&1 && { echo "FAIL: -j20 accepted"; exit 1; }
echo "all passed"
