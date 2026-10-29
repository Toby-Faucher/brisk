#!/bin/bash
# Stubbed test of brisk-service.sh; touches nothing real (systemctl, tailscale, loginctl, ss, curl,
# journalctl, cargo, npm, systemd-run and sunshine-brisk.sh are stubs; HOME, XDG_RUNTIME_DIR and
# the checkout are temp dirs).
set -eu # setup: any failure stops the test (set +e below, before the cases)
s=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/home/.config/brisk/tls" "$t/run" "$t/d" "$t/root/target/release" "$t/root/web/src" "$t/root/web/dist" "$t/root/web/node_modules"
L=$t/log
stub() { printf '#!/bin/bash\necho "%s $*" >> "%s"\n%s\n' "$1" "$L" "$2" > "$t/bin/$1"; chmod +x "$t/bin/$1"; }
# is-active: SVC_ACTIVE until a start/restart in this run sets $T/state (the B wrapper clears it).
# ACT_FAIL: a unit without the "# old" mark (the new one) fails to start; the old one starts.
stub systemctl 'case $2 in
  show) case "$*" in
    *brisk-hostd*) printf "Type=%s\nSubState=%s\nStatusText=%s\nWatchdogUSec=%s\nActiveEnterTimestamp=%s\n" "${S_TYPE-notify}" "${S_SUB-running}" "${S_STATUS-serving 127.0.0.1:8443, static=disk, stream idle}" "${S_WD-1min}" "${S_START:+@$S_START}";;
    *) echo "ExecStart={ path=${EXEC:-/usr/bin/sunshine} ; }";;
  esac;;
  is-active) [ -f "$T/state" ] && exit "$(cat "$T/state")"; exit "${SVC_ACTIVE:-3}";;
  is-enabled) exit "${SVC_ENABLED:-1}";;
  enable|start|restart) [ "$2" = enable ] && [ "$3" != --now ] && exit 0
    if [ -n "${ACT_FAIL:-}" ] && ! grep -q "# old" "$U"; then echo 3 > "$T/state"; exit 1; fi
    # a hostd binary built "bad" (see the cargo stub) never comes up
    if grep -qs bad "$BRISK_ROOT/target/service/bin/brisk-hostd"; then echo 3 > "$T/state"; exit 1; fi
    echo 0 > "$T/state";;
esac; exit 0'
stub tailscale 'case "$*" in
  "debug prefs") [ -n "${TS_OP:-}" ] && echo "  \"OperatorUser\": \"$(id -un)\",";;
  "serve status --json") cat "$T/serve-${TS_MAPPED:-}.json";;
  "status --json") cat "$T/status.json";;
esac; exit 0'
# Shaped like tailscale 1.104 output (MarshalIndent, 2 spaces). A peer comes first on purpose.
cat > "$t/status.json" <<'J'
{
  "Version": "1.104.1",
  "Peer": {
    "nodekey:1": {
      "DNSName": "laptop.tail0.ts.net.",
      "Online": true
    }
  },
  "Self": {
    "ID": "n1",
    "DNSName": "bawn.tail0.ts.net.",
    "TailscaleIPs": ["100.64.0.9"]
  },
  "CertDomains": ["bawn.tail0.ts.net"]
}
J
root='"bawn.tail0.ts.net:443": {
      "Handlers": {
        "/": {
          "Proxy": "http://127.0.0.1:8481"
        }
      }
    }'
printf '{\n  "TCP": {\n    "443": {\n      "HTTPS": true\n    }\n  },\n  "Web": {\n    %s\n  }\n}\n' "$root" > "$t/serve-.json"
printf '{\n  "TCP": {\n    "443": {\n      "HTTPS": true\n    },\n    "8443": {\n      "HTTPS": true\n    }\n  },\n  "Web": {\n    %s,\n    "bawn.tail0.ts.net:8443": {\n      "Handlers": {\n        "/": {\n          "Proxy": "http://127.0.0.1:8443"\n        }\n      }\n    }\n  }\n}\n' "$root" > "$t/serve-1.json"
# Not ours: :8443 points elsewhere and :443 points at 127.0.0.1:8443.
sed -e 's#127.0.0.1:8443#127.0.0.1:9999#' -e 's#127.0.0.1:8481#127.0.0.1:8443#' "$t/serve-1.json" > "$t/serve-wrong.json"
stub loginctl 'echo "Linger=${LINGER:-yes}"'
# like `ss -Hltn sport = :8443`: only the :8443 listeners of SS_ADDR
stub ss 'for a in ${SS_ADDR:-127.0.0.1:8443}; do case $a in *:8443) echo "LISTEN 0 1024 $a 0.0.0.0:*";; esac; done'
stub journalctl 'echo "page	http://localhost:8443/?s=0123456789abcdef0123456789abcdef&x=1"; echo "bridge	page	http://localhost:8443/?wt=https://100.64.0.9:4433/abcdef0123&hash=ab12"'
stub curl 'a="$*"; case $a in
  *http://127.0.0.1:8443/api/host*) [ "${LOCAL:-up}" = up ] && echo -n 403;;
  *47989/serverinfo*) echo "<currentgame>${GAME:-0}</currentgame><state>${STATE:-SUNSHINE_SERVER_FREE}</state>";;
  *api/stream*) [ "${STREAM:-idle}" = down ] || echo "{\"state\":\"${STREAM:-idle}\"}";;
  *"-H @"*) echo -n "${CODE_AUTH:-200}";;
  *x-brisk-csrf*) echo -n 403;;
  *https*) echo -n "${CODE_NOAUTH:-403}";;
esac'
# cargo build writes CARGO_OUT (default "good") into both release binaries
stub cargo '[ "$1" = build ] || exit 0; for b in brisk-hostd brisk-bridge; do echo "${CARGO_OUT:-good}" > "$BRISK_ROOT/target/release/$b"; done'
# RUSTUP=fail exits 1; RUSTUP=hang outlives BRISK_RUSTUP_TIMEOUT
stub rustup 'case ${RUSTUP:-ok} in fail) exit 1;; hang) exec sleep 30;; esac; exit 0'
# npm run build -- --outDir X: makes X/index.html like astro would
# NPM_FAIL: leaves a partial dir and fails; NPM_EMPTY: "succeeds" without an index.html
stub npm 'o=""; while [ $# -gt 0 ]; do [ "$1" = --outDir ] && o=$2; shift; done; [ -n "$o" ] || exit 0
mkdir -p "$o/_astro"; [ -n "${NPM_FAIL:-}" ] && exit 1; [ -n "${NPM_EMPTY:-}" ] && exit 0; echo built > "$o/index.html"'
# date +%Y%m%d-%H%M%S returns DATE_STAMP when set (a clock stepped back)
printf '#!/bin/sh\n[ -n "${DATE_STAMP:-}" ] && [ "$1" = +%%Y%%m%%d-%%H%%M%%S ] && { echo "$DATE_STAMP"; exit 0; }\nexec /usr/bin/date "$@"\n' > "$t/bin/date"
printf '#!/bin/sh\nshift 2; exec "$@"\n' > "$t/bin/nice"; cp "$t/bin/nice" "$t/bin/ionice"
printf '#!/bin/sh\necho "systemd-run $*" >> "%s"\nwhile [ "$1" != -c3 ]; do shift; done; shift\n[ "$1" = sh ] && { echo 9663676416; exit 0; }\nexec "$@"\n' "$L" > "$t/bin/systemd-run"
printf '#!/bin/sh\necho "sb $*" >> "%s"\n' "$L" > "$t/sb"
chmod +x "$t"/bin/* "$t/sb"
export T=$t PATH=$t/bin:$PATH HOME=$t/home XDG_RUNTIME_DIR=$t/run BRISK_SB=$t/sb SB_DROPIN_DIR=$t/d BRISK_ROOT=$t/root
export U=$t/home/.config/systemd/user/brisk-hostd.service BRISK_UP_WAIT=2 BRISK_RESTART_GAP=1
SB_BIN=$t/home/.local/opt/sunshine-brisk/bin/sunshine # the patched Sunshine binary brisk_running looks for
SV=$t/root/target/service/bin
# Each run starts with no started/failed state (see the systemctl stub).
printf '#!/bin/sh\nrm -f "%s/state"\nexec "%s" "$@"\n' "$t" "$s/brisk-service.sh" > "$t/bsvc"; chmod +x "$t/bsvc"
B=$t/bsvc
echo 0123456789abcdef0123456789abcdef > "$t/home/.config/brisk/secret"
# A fake brisk-up hostd: a sleep whose process name is brisk-hostd.
ln -s "$(command -v sleep)" "$t/brisk-hostd"
set +e
# Waits until the fake hostd has exec'd (its name is the parent's until then).
execed() { for _ in $(seq 100); do [ "$(cat "/proc/$1/comm" 2>/dev/null)" = brisk-hostd ] && return; sleep 0.05; done; echo "fake hostd $1 did not start"; }
bad=0
fail() { echo "FAIL: $*"; sed 's/^/    log: /' "$L" 2>/dev/null; bad=1; }

# install: refuses without the release binaries, then installs once and is idempotent.
: > "$L"; "$B" install >/dev/null 2>&1 && fail "install without binaries"
grep -q 'enable' "$L" && fail "enabled without binaries"
touch "$t/root/target/release/brisk-hostd" "$t/root/target/release/brisk-bridge" "$t/root/web/dist/index.html"
chmod +x "$t/root/target/release/"*
: > "$L"; "$B" install >/dev/null 2>&1 || fail "install failed"
grep -q "^ExecStart=$SV/brisk-hostd $t/root/web/dist$" "$U" || fail "unit ExecStart: $(grep ExecStart "$U")"
grep -q "^Environment=BRISK_BRIDGE_BIN=$SV/brisk-bridge$" "$U" || fail "unit bridge path"
grep -q '@ROOT@' "$U" && fail "unit kept an @ROOT@ token"
[ -x "$SV/brisk-hostd" ] && [ -x "$SV/brisk-bridge" ] || fail "install did not copy the binaries into target/service/bin"
grep -q '^systemctl --user daemon-reload' "$L" && grep -q '^systemctl --user enable --now brisk-hostd.service' "$L" || fail "no daemon-reload + enable --now"
grep -q sudo "$L" && fail "install used sudo"
out=$("$B" install 2>&1) || fail "second install failed"; grep -q 'unit unchanged' <<<"$out" || fail "second install rewrote the unit"
mkdir -p "$t/run/brisk"; "$t/brisk-hostd" 30 & p=$!; execed $p; echo $p > "$t/run/brisk/hostd.pid"
"$B" install >/dev/null 2>&1 && fail "install beside a brisk-up hostd"; kill $p
# a stale hostd.pid (the pid now belongs to another process) does not block install
sleep 30 & p=$!; echo $p > "$t/run/brisk/hostd.pid"
"$B" install >/dev/null 2>&1 || fail "install refused over a stale hostd.pid"; kill $p; rm -f "$t/run/brisk/hostd.pid"
# a checkout path with # and & in it is written into the unit as is
r2="$t/r#1&2"; cp -a "$t/root" "$r2"; rm -rf "$r2/target/service"
BRISK_ROOT=$r2 BRISK_UNIT_DIR=$t/u2 "$B" install >/dev/null 2>&1 || fail "install from a path with # and &"
grep -qxF "ExecStart=$r2/target/service/bin/brisk-hostd $r2/web/dist" "$t/u2/brisk-hostd.service" \
  || fail "path with # and &: $(grep ExecStart "$t/u2/brisk-hostd.service")"
# a changed unit is applied to a running service by a restart (enable --now would not)
echo '# old' >> "$U"
: > "$L"; SVC_ACTIVE=0 "$B" install >/dev/null 2>&1 || fail "install over a changed unit failed"
grep -q '^systemctl --user restart brisk-hostd.service' "$L" || fail "changed unit not restarted"
: > "$L"; SVC_ACTIVE=0 "$B" install >/dev/null 2>&1; grep -q 'restart' "$L" && fail "unchanged unit restarted"
# a changed unit while a stream is live (or the API is silent): nothing written, no backup, no reload
for e in STREAM=live STREAM=down; do
  echo '# old' >> "$U"; old=$(cat "$U"); rm -f "$U.bak"
  : > "$L"; out=$(env SVC_ACTIVE=0 "$e" "$B" install 2>&1) && fail "$e: install of a changed unit succeeded"
  grep -q 'unit changed; not installed while' <<<"$out" || fail "$e: no 'not installed' message: $out"
  [ "$(cat "$U")" = "$old" ] || fail "$e: unit file changed"
  [ -e "$U.bak" ] && fail "$e: backup created"
  grep -qE 'daemon-reload|restart|enable' "$L" && fail "$e: manager touched"
done
# same file but newer than the running process (an install that could not restart): restart when idle
SVC_ACTIVE=0 "$B" install >/dev/null 2>&1 || fail "idle install of the changed unit failed"
touch -d '1 hour ago' "$U"; : > "$L"; SVC_ACTIVE=0 S_START=$(date +%s) "$B" install >/dev/null 2>&1
grep -q restart "$L" && fail "restarted a service started after its unit"
touch "$U"; s=$(( $(date +%s) - 60 ))
: > "$L"; out=$(SVC_ACTIVE=0 S_START=$s STREAM=live "$B" install 2>&1) && fail "pending unit: install over a live stream succeeded"
grep -q 'unit installed but service not restarted yet' <<<"$out" || fail "pending unit not reported: $out"
grep -qE 'daemon-reload|restart' "$L" && fail "pending unit: manager touched over a live stream"
: > "$L"; out=$(SVC_ACTIVE=0 S_START=$s "$B" install 2>&1) || fail "pending unit, idle: install failed: $out"
grep -q '^systemctl --user restart brisk-hostd.service' "$L" || fail "pending unit not restarted when idle"
echo '# old' >> "$U"
: > "$L"; SVC_ACTIVE=0 STREAM=down FORCE=1 "$B" install >/dev/null 2>&1; grep -q '^systemctl --user restart' "$L" || fail "FORCE=1 did not restart"

# safe rollout: the previous unit is kept as .bak; a new unit that does not come up is rolled back.
echo '# old' >> "$U"; old=$(cat "$U")
: > "$L"; SVC_ACTIVE=0 "$B" install >/dev/null 2>&1 || fail "install of a good new unit failed"
[ "$(cat "$U.bak")" = "$old" ] || fail "previous unit not backed up"
grep -q '# old' "$U" && fail "new unit not written"
grep -q '^curl .*http://127.0.0.1:8443/api/host' "$L" || fail "did not wait for 8443 to answer"
for e in ACT_FAIL=1 LOCAL=down; do
  echo '# old' >> "$U"; old=$(cat "$U")
  : > "$L"; out=$(env SVC_ACTIVE=0 "$e" "$B" install 2>&1) && fail "install succeeded with $e"
  grep -q 'new unit failed; previous unit restored' <<<"$out" || fail "$e: no restore message: $out"
  [ "$(cat "$U")" = "$old" ] || fail "$e: previous unit not restored"
  [ "$(grep -c '^systemctl --user daemon-reload' "$L")" = 2 ] || fail "$e: restored unit not reloaded"
  # a restart after the second reload, then the came_up check again
  awk '/daemon-reload/ { n++ } n == 2 && /^systemctl --user restart / { r = 1 } r && /^curl .*8443\/api\/host/ { c = 1 } END { exit !c }' "$L" \
    || fail "$e: restored unit not restarted and checked"
done
grep -q 'did not come up either' <<<"$out" || fail "LOCAL=down: restored unit's failure not reported: $out"
# no backup (a first install that fails): says so, exits non-zero
rm -f "$U" "$U.bak"
out=$(ACT_FAIL=1 "$B" install 2>&1) && fail "a failing first install succeeded"
grep -q 'no previous unit' <<<"$out" || fail "missing backup not reported: $out"
grep -q '^systemctl --user disable --now brisk-hostd.service' "$L" || fail "failed first install left enabled"
"$B" install >/dev/null 2>&1 || fail "reinstall after the failed first install"

# serve-on/off: only ever the :8443 mapping; without operator rights a sudoq line, no call.
want='tailscale serve --bg --https=8443 http://127.0.0.1:8443'
: > "$L"; out=$("$B" serve-on 2>&1); rc=$?
[ $rc = 2 ] || fail "serve-on without operator: rc $rc"
grep -qF "sudoq run --why \"Brisk page on the tailnet at :8443 (root mapping untouched)\" -- $want" <<<"$out" || fail "no sudoq line: $out"
grep -q '^tailscale serve --bg' "$L" && fail "serve ran without operator rights"
: > "$L"; TS_OP=1 "$B" serve-on >/dev/null 2>&1 || fail "serve-on as operator failed"
grep -qx "$want" "$L" || fail "serve-on did not map :8443"
: > "$L"; TS_OP=1 TS_MAPPED=1 "$B" serve-on >/dev/null 2>&1; grep -q '^tailscale serve --bg' "$L" && fail "serve-on not idempotent"
: > "$L"; TS_OP=1 TS_MAPPED=1 "$B" serve-off >/dev/null 2>&1; grep -qx 'tailscale serve --https=8443 off' "$L" || fail "serve-off"
: > "$L"; TS_OP=1 "$B" serve-off >/dev/null 2>&1; grep -q 'off$' "$L" && fail "serve-off not idempotent"
# a mapping that is not exactly :8443 -> 127.0.0.1:8443 does not count as ours
: > "$L"; TS_OP=1 TS_MAPPED=wrong "$B" serve-on >/dev/null 2>&1; grep -qx "$want" "$L" || fail "wrong mapping counted as ours"
grep -h '^tailscale serve' "$L" | grep -v -e 'status --json' -e '--https=8443' && fail "a serve call outside :8443"

# sunshine-permanent: refuses on a busy host, else install + enable through sunshine-brisk.sh.
: > "$L"; STATE=SUNSHINE_SERVER_BUSY GAME=1 "$B" sunshine-permanent >/dev/null 2>&1 && fail "permanent on a busy host"
grep -q '^sb ' "$L" && fail "sunshine-brisk ran on a busy host"
# FREE but an app still running: enable would refuse, so install must not write the drop-in
: > "$L"; GAME=1 "$B" sunshine-permanent >/dev/null 2>&1 && fail "permanent with an app running"
grep -q '^sb ' "$L" && fail "sunshine-brisk ran with an app running"
: > "$L"; "$B" sunshine-permanent >/dev/null 2>&1 || fail "sunshine-permanent failed"
grep -q '^sb install' "$L" && grep -q '^sb enable' "$L" || fail "no sb install + enable"
: > "$t/d/brisk.conf"; : > "$L"; EXEC=$SB_BIN "$B" sunshine-permanent >/dev/null 2>&1
grep -q '^sb ' "$L" && fail "sunshine-permanent not idempotent"

# update: capped build, web build only when sources changed, no restart over a live stream.
touch -d '2 days ago' "$t/root/web/src/a.ts" "$t/root/web/package-lock.json"; touch "$t/root/web/dist/index.html" "$t/root/web/node_modules/.package-lock.json"
sv0=$(cat "$SV/brisk-hostd")
: > "$L"; CARGO_OUT=v2 SVC_ACTIVE=0 STREAM=live "$B" update >/dev/null 2>&1 && fail "update restarted over a live stream"
# ... and the running hostd's binaries stay as they were (the new ones were built, not swapped in)
[ "$(cat "$t/root/target/release/brisk-hostd")" = v2 ] || fail "cargo stub did not build"
[ "$(cat "$SV/brisk-hostd")" = "$sv0" ] && [ "$(cat "$SV/brisk-bridge")" = "$sv0" ] || fail "refused update replaced the service binaries"
ls "$SV"/*.new >/dev/null 2>&1 && fail "refused update left staged binaries: $(ls "$SV")"
grep -q 'ionice -c3 cargo build --release -q -p brisk-hostd -p brisk-bridge' "$L" || fail "build not capped"
grep -q '^rustup toolchain install --no-self-update' "$L" || fail "toolchain not installed before the build"
grep -q '^npm' "$L" && fail "npm ran with web/dist up to date"
grep -q 'restart' "$L" && fail "restart over a live stream"
# a failed or hung toolchain install stops update before the build, the web swap and any restart
for m in fail hang; do
  : > "$L"; RUSTUP=$m BRISK_RUSTUP_TIMEOUT=1 SVC_ACTIVE=0 "$B" update >/dev/null 2>&1 && fail "update went on after rustup $m"
  grep -qE 'cargo build|^npm|restart' "$L" && fail "update built or restarted after rustup $m"
done
: > "$L"; SVC_ACTIVE=0 "$B" update >/dev/null 2>&1 || fail "update failed"
grep -q '^systemctl --user restart brisk-hostd.service' "$L" || fail "update did not restart"
[ "$(cat "$SV/brisk-hostd")" = good ] && [ "$(cat "$SV/brisk-bridge")" = good ] || fail "update did not put the new binaries in place"
[ "$(cat "$SV/brisk-hostd.prev")" = "$sv0" ] || fail "update kept no previous hostd for a rollback"
touch "$t/root/web/src/a.ts"; touch -d '1 day ago' "$t/root/web/dist/index.html"
: > "$L"; "$B" update >/dev/null 2>&1; grep -q '^npm run build -- --outDir '"$t/root/web/dist."'[0-9]' "$L" || fail "web not rebuilt into a new dir after a source change"
grep -q '^npm ci' "$L" && fail "npm ci with node_modules up to date"
# the build dir is swapped in by renaming a symlink; the old plain dist is kept aside
w=$t/root/web
[ -L "$w/dist" ] && [ "$(cat "$w/dist/index.html")" = built ] || fail "web/dist is not a symlink to the new build"
ls -d "$w"/dist.plain-* >/dev/null 2>&1 && [ -f "$(ls -d "$w"/dist.plain-* | head -1)/index.html" ] || fail "old plain dist not kept for rollback: $(ls "$w")"
builds() { find "$w" -maxdepth 1 -name 'dist.*' -type d | sort; }
src_changed() { sleep 1.1; touch "$w/src/a.ts"; touch -d '1 day ago' "$w/dist/index.html"; }
for i in 1 2; do src_changed; "$B" update >/dev/null 2>&1 || fail "update $i"; done
[ "$(builds | wc -l)" = 2 ] || fail "want the live + one previous build, have: $(builds)"
[ -e "$w/dist.link" ] && fail "temp link left"
cur=$(readlink "$w/dist"); [ -f "$w/$cur/index.html" ] || fail "dist points at a missing build ($cur)"
# a failed or empty build: removed, web/dist and both kept builds untouched, update fails
before=$(builds); live=$(readlink "$w/dist")
for e in NPM_FAIL=1 NPM_EMPTY=1; do
  src_changed; env "$e" "$B" update >/dev/null 2>&1 && fail "update succeeded with $e"
  [ "$(readlink "$w/dist")" = "$live" ] || fail "$e: web/dist moved"
  [ "$(builds)" = "$before" ] || fail "$e: builds changed: $(builds)"
done
# the page is swapped only with the restart: a live stream or a silent API keeps the old page
for e in STREAM=live STREAM=down; do
  src_changed; : > "$L"; env SVC_ACTIVE=0 "$e" "$B" update >/dev/null 2>&1 && fail "update went on with $e"
  [ "$(readlink "$w/dist")" = "$live" ] || fail "$e: new page against the old hostd"
  [ "$(builds)" = "$before" ] || fail "$e: new build left behind: $(builds)"
  grep -q restart "$L" && fail "$e: restarted"
done
src_changed; : > "$L"; SVC_ACTIVE=0 STREAM=down FORCE=1 "$B" update >/dev/null 2>&1 || fail "FORCE=1 update"
[ "$(readlink "$w/dist")" != "$live" ] && grep -q '^systemctl --user restart' "$L" || fail "FORCE=1: no swap + restart"
# clock stepped back: the new (live) build sorts oldest by name; it and the newest other one stay
prev=$(readlink "$w/dist"); src_changed
DATE_STAMP=19990101-000000 "$B" update >/dev/null 2>&1 || fail "update with an old clock"
[ "$(readlink "$w/dist")" = dist.19990101-000000 ] || fail "not swapped to the old-stamped build"
[ "$(builds)" = "$(printf '%s\n' "$w/dist.19990101-000000" "$w/$prev" | sort)" ] || fail "prune by name, not by mtime/live: $(builds)"
# the same old stamp again (exists: gets -PID) is still the live one afterwards
src_changed; DATE_STAMP=19990101-000000 "$B" update >/dev/null 2>&1 || fail "update with a taken stamp"
cur=$(readlink "$w/dist"); [ -f "$w/$cur/index.html" ] && [ "$cur" != dist.19990101-000000 ] || fail "taken stamp: dist -> $cur"
[ "$(builds | wc -l)" = 2 ] || fail "taken stamp: $(builds)"
# a new build that does not come up: the previous binaries and page go back, restarted and checked
src_changed; prev=$(readlink "$w/dist"); : > "$L"
out=$(CARGO_OUT=bad SVC_ACTIVE=0 "$B" update 2>&1) && fail "update succeeded with a hostd that does not come up"
grep -q 'previous build restored (see' <<<"$out" || fail "rollback: no restore message: $out"
[ "$(cat "$SV/brisk-hostd")" = good ] && [ "$(cat "$SV/brisk-bridge")" = good ] || fail "rollback: previous binaries not restored"
[ "$(readlink "$w/dist")" = "$prev" ] || fail "rollback: page not restored ($(readlink "$w/dist"), want $prev)"
[ "$(grep -c '^systemctl --user restart brisk-hostd' "$L")" = 2 ] || fail "rollback: want two restarts"
awk '/^systemctl --user restart / { n++ } n == 2 && /^curl .*8443\/api\/host/ { c = 1 } END { exit !c }' "$L" \
  || fail "rollback: restored build not checked after its restart"

# doctor: all good, then each broken check fails it.
: > "$t/home/.config/brisk/tls/bridge.pem"
dr() { env SVC_ACTIVE=0 SVC_ENABLED=0 TS_MAPPED=1 EXEC="$SB_BIN" "$@" "$B" doctor 2>&1; }
out=$(dr) || fail "doctor failed on a good setup: $out"
[ -n "${VERBOSE:-}" ] && echo "$out"
out=$(dr "SS_ADDR=127.0.0.1:8443 100.64.0.9%tailscale0:443 100.64.0.9%tailscale0:8443 [fd7a:115c:a1e0::9]%tailscale0:8443") \
  || fail "doctor failed with tailscaled's tailscale0 listeners: $out"
for e in SS_ADDR=0.0.0.0:8443 "SS_ADDR=127.0.0.1:8443 [::]:8443" "SS_ADDR=127.0.0.1:8443 *:8443" "SS_ADDR=127.0.0.1:8443 192.168.1.5:8443" \
    SS_ADDR=100.64.0.9%tailscale0:8443 TS_MAPPED=wrong CODE_NOAUTH=200 CODE_AUTH=403 LINGER=no TS_MAPPED= SVC_ACTIVE=3 \
    S_TYPE=simple S_SUB=start-pre S_STATUS= S_WD=0 S_WD=30s; do
  out=$(dr "$e") && fail "doctor passed with $e"
  grep -q '^FAIL' <<<"$out" || fail "no FAIL line with $e"
done
grep -q '0123456789abcdef0123' "$L" && fail "the secret appeared on a command line"
# the secret header files are per run and removed afterwards
ls "$t/run/brisk" | grep -q -e '^hdr\.' -e '^doctor\.hdr$' && fail "secret header file left: $(ls "$t/run/brisk")"
# ... also when the run is killed mid-request (a curl that hangs on /api/stream)
mkdir -p "$t/hang"
printf '#!/bin/bash\ncase "$*" in *api/stream*) exec sleep 5;; esac\nexec "%s" "$@"\n' "$t/bin/curl" > "$t/hang/curl"; chmod +x "$t/hang/curl"
PATH=$t/hang:$PATH SVC_ACTIVE=0 "$B" update >/dev/null 2>&1 & k=$!
for _ in $(seq 50); do ls "$t/run/brisk" | grep -q '^hdr\.' && break; sleep 0.1; done
ls "$t/run/brisk" | grep -q '^hdr\.' || fail "no header file seen during the hanging request"
# TERM to the script and to the $(restart_ok) subshell that holds the file
pkill -TERM -P "$k"; kill -TERM "$k"; wait "$k"
sleep 0.5; ls "$t/run/brisk" | grep -q '^hdr\.' && fail "header file left after TERM: $(ls "$t/run/brisk")"
rm -f "$SV"/*.new # the killed update had staged binaries
# usage
"$B" --help | grep -q '^usage: ' || fail "--help"
"$B" bogus >/dev/null 2>&1 && fail "unknown command accepted"

# link, logs, uninstall.
"$B" link | grep -qx $'page\thttps://bawn.tail0.ts.net:8443/?s=0123456789abcdef0123456789abcdef' || fail "link"
: > "$L"; out=$("$B" logs -f); grep -qx 'journalctl --user -u brisk-hostd.service --no-pager -f' "$L" || fail "logs"
[ "$out" = $'page\thttp://localhost:8443/?s=<hidden>&x=1\nbridge\tpage\thttp://localhost:8443/?wt=<hidden>&hash=ab12' ] \
  || fail "logs did not hide the secret and the session token: $out"
: > "$L"; "$B" uninstall >/dev/null 2>&1 || fail "uninstall failed"
[ -f "$U" ] && fail "unit left after uninstall"; grep -q 'disable --now brisk-hostd.service' "$L" || fail "not disabled"
"$B" uninstall >/dev/null 2>&1 || fail "uninstall not idempotent"

if command -v shellcheck >/dev/null; then shellcheck -x "$s/brisk-service.sh" "$s/brisk-up.sh" "$s/brisk-down.sh" || fail shellcheck; fi
[ $bad = 0 ] && echo "ok: brisk-service"
exit $bad
