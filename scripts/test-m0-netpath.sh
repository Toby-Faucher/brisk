#!/bin/sh
# Fixture test for m0-netpath.sh parsing; no live peer needed.
set -eu
d=$(cd "$(dirname "$0")" && pwd); f=$d/fixtures; s=$d/m0-netpath.sh
chk() { echo "$out" | grep -qF -- "$1" || { echo "FAIL: missing '$1' in:"; echo "$out"; exit 1; }; }
export TS_STATUS_JSON=$f/status.json TS_PING_OUT=$f/tsping.txt PING_OUT=$f/ping.txt
out=$($s direct-peer); chk "status: direct CurAddr=192.0.2.5:41641"; chk "samples=20 min=1.000ms median=31.000ms p99=40.000ms"; chk "10 path: DERP(den)"
out=$($s derp-peer.example.ts.net);  chk "status: DERP Relay=den"
out=$($s 192.0.2.9); chk "plain ping"; chk "samples=5 min=0.110ms median=0.330ms p99=0.550ms"
rc=0; $s bawn >/dev/null 2>&1 || rc=$?
[ "$rc" = 2 ] || { echo "FAIL: self should exit 2, got $rc"; exit 1; }
echo "all passed"
