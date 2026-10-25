#!/bin/bash
# Stubbed test of brisk-up.sh / brisk-down.sh guards; touches nothing real (systemctl, curl,
# tailscale, cargo, npm, openssl, python3, systemd-run and sunshine-brisk.sh are stubs; HOME and
# XDG_RUNTIME_DIR are temp dirs).
set -u
s=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/home" "$t/run" "$t/d"
L=$t/log
for c in cargo npm openssl python3 nice ionice; do printf '#!/bin/sh\necho "%s $*" >> "%s"\n' "$c" "$L" > "$t/bin/$c"; done
printf '#!/bin/sh\nshift 2; exec "$@"\n' > "$t/bin/nice"; printf '#!/bin/sh\nshift 2; exec "$@"\n' > "$t/bin/ionice"
printf '#!/bin/sh\n[ -n "${TS_NONE:-}" ] || echo 100.64.0.9\n' > "$t/bin/tailscale"
printf '#!/bin/sh\necho "<currentgame>${GAME:-0}</currentgame><state>${STATE:-SUNSHINE_SERVER_FREE}</state>"\n' > "$t/bin/curl"
printf '#!/bin/sh\n[ "$2" = show ] && echo "ExecStart={ path=${EXEC:-/usr/bin/sunshine} ; }"\nexit 0\n' > "$t/bin/systemctl"
# systemd-run: logs, fails if SR_FAIL, reports the enforced memory.max for the probe, else runs the command
printf '#!/bin/sh\necho "systemd-run $*" >> "%s"\n[ -z "${SR_FAIL:-}" ] || exit 1\nwhile [ "$1" != -c3 ]; do shift; done; shift\n[ "$1" = sh ] && { echo 9663676416; exit 0; }\nexec "$@"\n' "$L" > "$t/bin/systemd-run"
printf '#!/bin/sh\necho "sb $*" >> "%s"\n' "$L" > "$t/sb"
chmod +x "$t"/bin/* "$t/sb"
export PATH=$t/bin:$PATH HOME=$t/home XDG_RUNTIME_DIR=$t/run BRISK_SB=$t/sb SB_DROPIN_DIR=$t/d
bad=0
fail() { echo "FAIL: $*"; sed 's/^/    log: /' "$L" 2>/dev/null; bad=1; } # keep going: report every failure
no_sb() { ! grep -q '^sb ' "$L" || fail "$1: sunshine-brisk.sh was called"; }

# 1. Busy host: brisk-up writes nothing (no install, no enable) and fails.
: > "$L"; STATE=SUNSHINE_SERVER_BUSY GAME=1 "$s/brisk-up.sh" >/dev/null 2>&1 && fail "brisk-up succeeded on a busy host"
no_sb "busy host"
# 4. ... and its build went through the capped scope (the busy check comes after the build).
grep -q '^systemd-run --user --scope -q -p MemoryHigh=7G -p MemoryMax=9G -p MemorySwapMax=0 -p CPUWeight=20 nice -n 19 ionice -c3 cargo build --release' "$L" \
  || fail "cargo build not capped"
grep -q '^cargo build' "$L" || fail "cargo build did not run"

# 4. No enforceable cap: refuse to build at all.
: > "$L"; SR_FAIL=1 "$s/brisk-up.sh" >/dev/null 2>&1 && fail "brisk-up built without a cap"
grep -q '^cargo' "$L" && fail "cargo ran uncapped"
no_sb "no cap"

# 3. Bad listen address: refused before any Sunshine action.
for env in TS_NONE=1 BRISK_LISTEN=0.0.0.0:4433 BRISK_LISTEN=bawn:4433; do
  : > "$L"; env "$env" "$s/brisk-up.sh" >/dev/null 2>&1 && fail "brisk-up accepted $env"
  pkill -f "^$t/run" 2>/dev/null
  no_sb "$env"
done

# 1. A drop-in left behind while stock Sunshine runs: brisk-down rolls back.
: > "$L"; : > "$t/d/brisk.conf"
"$s/brisk-down.sh" >/dev/null 2>&1 || fail "brisk-down failed"
grep -q '^sb rollback' "$L" || fail "brisk-down skipped rollback with brisk.conf present"
rm -f "$t/d/brisk.conf"
: > "$L"; "$s/brisk-down.sh" >/dev/null 2>&1; grep -q '^sb rollback' "$L" && fail "rollback without cause"

# 2. brisk-down stops a bridge started in the background of a script (SIGINT ignored) with TERM.
mkdir -p "$t/run/brisk"; sleep 300 & b=$!; echo $b > "$t/run/brisk/bridge.pid"
: > "$L"; BRISK_DOWN_WAIT=3 "$s/brisk-down.sh" >/dev/null 2>&1 || { kill $b; fail "brisk-down could not stop the bridge"; }
kill -0 $b 2>/dev/null && { kill $b; fail "bridge still running"; }
[ $bad = 0 ] && echo "ok: brisk-up/down guards"
exit $bad
