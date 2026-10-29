#!/bin/bash
# Test of setup-retroarch.sh against a temp dir; touches nothing real.
set -u
s=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
bad=0
fail() { echo "FAIL: $*"; bad=1; }
run() { "$s/setup-retroarch.sh" --dir "$t/c" --data "$t/d" "$@" >/dev/null || fail "setup-retroarch.sh $* exited non-zero"; }
snap() { cat "$t"/c/retroarch.cfg "$t"/c/autoconfig/* 2>/dev/null | cksum; }
cfg=$t/c/retroarch.cfg

run
cmp -s "$s/../deploy/retroarch/Sunshine_X-Box_Series.cfg" "$t/c/autoconfig/Sunshine_X-Box_Series.cfg" || fail "fresh: profile not written"
for l in 'confirm_quit = "false"' 'video_fullscreen = "true"' "savefile_directory = \"$t/d/saves\"" "system_directory = \"$t/d/system\"" "joypad_autoconfig_dir = \"$t/c/autoconfig\""; do
  grep -qxF "$l" "$cfg" || fail "fresh: retroarch.cfg lacks: $l"
done
! grep -q '@' "$cfg" || fail "an @ placeholder was left in retroarch.cfg"
for d in saves states system; do [ -d "$t/d/$d" ] || fail "folder $d not created"; done
a=$(snap); run; [ "$a" = "$(snap)" ] || fail "second run changed something"

sed -i 's/^confirm_quit = .*/confirm_quit = "true"/' "$cfg"; echo "# mine" >> "$t/c/autoconfig/Sunshine_X-Box_Series.cfg"
run
grep -qx 'confirm_quit = "true"' "$cfg" || fail "an existing key was rewritten without --force"
[ ! -e "$t/c/autoconfig/Sunshine_X-Box_Series.cfg.before-brisk" ] || fail "backup made without --force"
run --force
grep -qx 'confirm_quit = "false"' "$cfg" || fail "--force did not set the key"
[ "$(grep -c '^confirm_quit' "$cfg")" = 1 ] || fail "--force duplicated a key"
grep -q '^# mine' "$t/c/autoconfig/Sunshine_X-Box_Series.cfg.before-brisk" || fail "--force kept no backup"

printf 'foo = "1"\nvideo_fullscreen = "false"\n' > "$cfg"
run
grep -qxF 'foo = "1"' "$cfg" && grep -qx 'video_fullscreen = "false"' "$cfg" || fail "user keys lost or changed"
grep -qx 'pause_nonactive = "false"' "$cfg" || fail "missing keys not added to an existing file"

[ $bad = 0 ] && echo "ok: setup-retroarch"
exit $bad
