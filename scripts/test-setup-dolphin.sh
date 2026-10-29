#!/bin/bash
# shellcheck disable=SC2015 # A && B || fail asserts both
# Test of setup-dolphin.sh against a temp dir; touches nothing real.
set -u
s=$(cd "$(dirname "$0")" && pwd)
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
bad=0
fail() { echo "FAIL: $*"; bad=1; }
run() { "$s/setup-dolphin.sh" --dir "$t/c" "$@" >/dev/null || fail "setup-dolphin.sh $* exited non-zero"; }
snap() { cat "$t"/c/* 2>/dev/null | cksum; }

run
cmp -s "$s/../deploy/dolphin/WiimoteNew.ini" "$t/c/WiimoteNew.ini" || fail "fresh: profile not written"
grep -qx 'BackgroundInput = True' "$t/c/Dolphin.ini" || fail "fresh: Dolphin.ini lacks the key"
grep -qx 'ConfirmStop = False' "$t/c/Dolphin.ini" || fail "fresh: Dolphin.ini lacks ConfirmStop"
a=$(snap); run; [ "$a" = "$(snap)" ] || fail "second run changed something"

echo "# mine" >> "$t/c/WiimoteNew.ini"; a=$(snap)
run; [ "$a" = "$(snap)" ] || fail "edited file touched without --force"
[ ! -e "$t/c/WiimoteNew.ini.before-brisk" ] || fail "backup made without --force"
run --force
grep -q '^# mine' "$t/c/WiimoteNew.ini.before-brisk" || fail "--force kept no backup of the edit"
cmp -s "$s/../deploy/dolphin/WiimoteNew.ini" "$t/c/WiimoteNew.ini" || fail "--force did not restore the profile"

printf '[Core]\nFoo = 1\n[Input]\nBar = 2\n[Other]\nBaz = 3\n' > "$t/c/Dolphin.ini"
run
for l in '[Core]' 'Foo = 1' 'Bar = 2' '[Other]' 'Baz = 3' 'BackgroundInput = True'; do
  grep -qxF "$l" "$t/c/Dolphin.ini" || fail "Dolphin.ini lost or missed: $l"
done
printf '[Core]\nFoo = 1\n' > "$t/c/Dolphin.ini"
run
grep -qxF 'Foo = 1' "$t/c/Dolphin.ini" && grep -qx 'BackgroundInput = True' "$t/c/Dolphin.ini" || fail "no [Input] section case"

printf '[Interface]\nConfirmStop = True\nX = 1\n' > "$t/c/Dolphin.ini"
run
grep -qx 'ConfirmStop = True' "$t/c/Dolphin.ini" && ! grep -q 'ConfirmStop = False' "$t/c/Dolphin.ini" || fail "an existing ConfirmStop was rewritten"

[ $bad = 0 ] && echo "ok: setup-dolphin"
exit $bad
