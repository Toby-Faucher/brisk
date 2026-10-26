#!/bin/bash
# Stubbed test of brisk-service.sh; touches nothing real (systemctl, tailscale, loginctl, ss, curl,
# journalctl, cargo, npm, systemd-run and sunshine-brisk.sh are stubs; HOME, XDG_RUNTIME_DIR and
# the checkout are temp dirs).
set -u
s=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/home/.config/brisk/tls" "$t/run" "$t/d" "$t/root/target/release" "$t/root/web/src" "$t/root/web/dist" "$t/root/web/node_modules"
L=$t/log
stub() { printf '#!/bin/bash\necho "%s $*" >> "%s"\n%s\n' "$1" "$L" "$2" > "$t/bin/$1"; chmod +x "$t/bin/$1"; }
stub systemctl 'case $2 in show) echo "ExecStart={ path=${EXEC:-/usr/bin/sunshine} ; }";; is-active) exit "${SVC_ACTIVE:-3}";; is-enabled) exit "${SVC_ENABLED:-1}";; esac; exit 0'
stub tailscale 'case "$*" in
  "debug prefs") [ -n "${TS_OP:-}" ] && echo "  \"OperatorUser\": \"$(id -un)\",";;
  "serve status --json") [ -n "${TS_MAPPED:-}" ] && echo "\"Proxy\": \"http://127.0.0.1:8443\"";;
  "status --json") printf "  \"Self\": {\n    \"DNSName\": \"bawn.tail0.ts.net.\",\n";;
esac; exit 0'
stub loginctl 'echo "Linger=${LINGER:-yes}"'
stub ss 'echo "LISTEN 0 1024 ${SS_ADDR:-127.0.0.1:8443} 0.0.0.0:*"'
stub journalctl ''
stub curl 'a="$*"; case $a in
  *47989/serverinfo*) echo "<currentgame>${GAME:-0}</currentgame><state>${STATE:-SUNSHINE_SERVER_FREE}</state>";;
  *api/stream*) echo "{\"state\":\"${STREAM:-idle}\"}";;
  *"-H @"*) echo -n "${CODE_AUTH:-200}";;
  *x-brisk-csrf*) echo -n 403;;
  *https*) echo -n "${CODE_NOAUTH:-403}";;
esac'
for c in cargo npm; do stub $c ''; done
printf '#!/bin/sh\nshift 2; exec "$@"\n' > "$t/bin/nice"; cp "$t/bin/nice" "$t/bin/ionice"
printf '#!/bin/sh\necho "systemd-run $*" >> "%s"\nwhile [ "$1" != -c3 ]; do shift; done; shift\n[ "$1" = sh ] && { echo 9663676416; exit 0; }\nexec "$@"\n' "$L" > "$t/bin/systemd-run"
printf '#!/bin/sh\necho "sb $*" >> "%s"\n' "$L" > "$t/sb"
chmod +x "$t"/bin/* "$t/sb"
export PATH=$t/bin:$PATH HOME=$t/home XDG_RUNTIME_DIR=$t/run BRISK_SB=$t/sb SB_DROPIN_DIR=$t/d BRISK_ROOT=$t/root
B=$s/brisk-service.sh U=$t/home/.config/systemd/user/brisk-hostd.service
echo 0123456789abcdef0123456789abcdef > "$t/home/.config/brisk/secret"
bad=0
fail() { echo "FAIL: $*"; sed 's/^/    log: /' "$L" 2>/dev/null; bad=1; }

# install: refuses without the release binaries, then installs once and is idempotent.
: > "$L"; "$B" install >/dev/null 2>&1 && fail "install without binaries"
grep -q 'enable' "$L" && fail "enabled without binaries"
touch "$t/root/target/release/brisk-hostd" "$t/root/target/release/brisk-bridge" "$t/root/web/dist/index.html"
chmod +x "$t/root/target/release/"*
: > "$L"; "$B" install >/dev/null 2>&1 || fail "install failed"
grep -q "^ExecStart=$t/root/target/release/brisk-hostd $t/root/web/dist$" "$U" || fail "unit ExecStart: $(grep ExecStart "$U")"
grep -q "^Environment=BRISK_BRIDGE_BIN=$t/root/target/release/brisk-bridge$" "$U" || fail "unit bridge path"
grep -q '^systemctl --user daemon-reload' "$L" && grep -q '^systemctl --user enable --now brisk-hostd.service' "$L" || fail "no daemon-reload + enable --now"
grep -q sudo "$L" && fail "install used sudo"
out=$("$B" install 2>&1) || fail "second install failed"; grep -q 'unit unchanged' <<<"$out" || fail "second install rewrote the unit"
mkdir -p "$t/run/brisk"; sleep 30 & p=$!; echo $p > "$t/run/brisk/bridge.pid"
"$B" install >/dev/null 2>&1 && fail "install beside a brisk-up hostd"; kill $p; rm -f "$t/run/brisk/bridge.pid"

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
grep -h '^tailscale serve' "$L" | grep -v -e 'status --json' -e '--https=8443' && fail "a serve call outside :8443"

# sunshine-permanent: refuses on a busy host, else install + enable through sunshine-brisk.sh.
: > "$L"; STATE=SUNSHINE_SERVER_BUSY GAME=1 "$B" sunshine-permanent >/dev/null 2>&1 && fail "permanent on a busy host"
grep -q '^sb ' "$L" && fail "sunshine-brisk ran on a busy host"
: > "$L"; "$B" sunshine-permanent >/dev/null 2>&1 || fail "sunshine-permanent failed"
grep -q '^sb install' "$L" && grep -q '^sb enable' "$L" || fail "no sb install + enable"
: > "$t/d/brisk.conf"; : > "$L"; EXEC=/x/sunshine-brisk/bin/sunshine "$B" sunshine-permanent >/dev/null 2>&1
grep -q '^sb ' "$L" && fail "sunshine-permanent not idempotent"

# update: capped build, web build only when sources changed, no restart over a live stream.
touch -d '2 days ago' "$t/root/web/src/a.ts" "$t/root/web/package-lock.json"; touch "$t/root/web/dist/index.html" "$t/root/web/node_modules/.package-lock.json"
: > "$L"; SVC_ACTIVE=0 STREAM=live "$B" update >/dev/null 2>&1 && fail "update restarted over a live stream"
grep -q 'ionice -c3 cargo build --release -q -p brisk-hostd -p brisk-bridge' "$L" || fail "build not capped"
grep -q '^npm' "$L" && fail "npm ran with web/dist up to date"
grep -q 'restart' "$L" && fail "restart over a live stream"
: > "$L"; SVC_ACTIVE=0 "$B" update >/dev/null 2>&1 || fail "update failed"
grep -q '^systemctl --user restart brisk-hostd.service' "$L" || fail "update did not restart"
touch "$t/root/web/src/a.ts"; touch -d '1 day ago' "$t/root/web/dist/index.html"
: > "$L"; "$B" update >/dev/null 2>&1; grep -q '^npm run build' "$L" || fail "web not rebuilt after a source change"
grep -q '^npm ci' "$L" && fail "npm ci with node_modules up to date"

# doctor: all good, then each broken check fails it.
: > "$t/home/.config/brisk/tls/bridge.pem"
dr() { env SVC_ACTIVE=0 SVC_ENABLED=0 TS_MAPPED=1 EXEC=/x/sunshine-brisk/bin/sunshine "$@" "$B" doctor 2>&1; }
out=$(dr) || fail "doctor failed on a good setup: $out"
[ -n "${VERBOSE:-}" ] && echo "$out"
for e in SS_ADDR=0.0.0.0:8443 CODE_NOAUTH=200 CODE_AUTH=403 LINGER=no TS_MAPPED= SVC_ACTIVE=3; do
  out=$(dr "$e") && fail "doctor passed with $e"
  grep -q '^FAIL' <<<"$out" || fail "no FAIL line with $e"
done
grep -q '0123456789abcdef0123' "$L" && fail "the secret appeared on a command line"

# link, logs, uninstall.
"$B" link | grep -qx $'page\thttps://bawn.tail0.ts.net:8443/?s=0123456789abcdef0123456789abcdef' || fail "link"
: > "$L"; "$B" logs -f >/dev/null; grep -qx 'journalctl --user -u brisk-hostd.service --no-pager -f' "$L" || fail "logs"
: > "$L"; "$B" uninstall >/dev/null 2>&1 || fail "uninstall failed"
[ -f "$U" ] && fail "unit left after uninstall"; grep -q 'disable --now brisk-hostd.service' "$L" || fail "not disabled"
"$B" uninstall >/dev/null 2>&1 || fail "uninstall not idempotent"

if command -v shellcheck >/dev/null; then shellcheck -x "$s/brisk-service.sh" "$s/brisk-up.sh" "$s/brisk-down.sh" || fail shellcheck; fi
[ $bad = 0 ] && echo "ok: brisk-service"
exit $bad
