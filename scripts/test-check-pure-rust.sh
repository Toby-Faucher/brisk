#!/bin/bash
# Stubbed test of check-pure-rust.sh: a fake `cargo` serves canned `cargo tree` / `cargo metadata`;
# no build happens. Env knobs for the stub: TREE/META (files), TREE_RC (tree exit code), HIT=1
# (every `-i` per-package check finds a dependency).
set -u
s=$(cd "$(dirname "$0")" && pwd)/check-pure-rust.sh
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
mkdir "$t/bin"
cat > "$t/bin/cargo" <<'S'
#!/bin/bash
case $1 in
metadata) cat "$META"; exit 0;;
esac
for a in "$@"; do
  [ "$a" = -i ] || continue
  [ -n "${HIT:-}" ] && { echo "ring v0.17.14"; exit 0; }
  echo "error: package ID specification did not match any packages" >&2; exit 101
done
cat "$TREE"; exit "${TREE_RC:-0}"
S
chmod +x "$t/bin/cargo"
export PATH=$t/bin:$PATH
tree() { printf 'brisk-hostd v0.0.0\nbrisk-proto v0.0.0\nserde v1.0.1\nlibc v0.2.1\n%s' "$1"; }
tree '' > "$t/clean"
tree $'foo-sys v0.1.0\n' > "$t/sys"
tree $'wayland_sys v0.1.0\n' > "$t/sys2"
tree $'cc v1.0.0\n' > "$t/cc"
tree $'zlib-native v1.0.0\n' > "$t/zl"
printf 'brisk-proto v0.0.0\n' > "$t/badfmt"   # no brisk-hostd line
: > "$t/empty"
# metadata: brisk-hostd -> z (normal); z declares links = "z" when $2 is set
meta() { cat <<J
{"packages":[{"id":"h","name":"brisk-hostd","links":null},{"id":"z","name":"zlib-native","links":$1}],
 "workspace_members":["h"],
 "resolve":{"nodes":[{"id":"h","deps":[{"pkg":"z","dep_kinds":[{"kind":null}]}]},{"id":"z","deps":[]}]}}
J
}
meta null > "$t/nolinks"; meta '"z"' > "$t/links"
: > "$t/none"
printf '# comment\nfoo-sys | vendored zlib, no pure alternative | 2026-10-26\nzlib-native | links libz, pinned | 2026-10-26\n' > "$t/allow"
printf 'foo-sys | no date\n' > "$t/badallow"
fail=0
expect() { # name want-rc tree meta allow [text that must appear]  (env vars pass through)
    out=$(TREE=$3 META=$4 PURE_ALLOW_FILE=$5 "$s" 2>&1); rc=$?
    { [ "$rc" = "$2" ] && { [ -z "${6:-}" ] || echo "$out" | grep -qF -- "$6"; }; } \
        || { echo "FAIL: $1 (rc=$rc)"; echo "$out"; fail=1; }
}
expect "clean graph passes" 0 "$t/clean" "$t/nolinks" "$t/none"
expect "foo-sys fails" 1 "$t/sys" "$t/nolinks" "$t/none" "NOT PURE: foo-sys"
expect "wayland_sys fails" 1 "$t/sys2" "$t/nolinks" "$t/none" "NOT PURE: wayland_sys"
expect "cc fails" 1 "$t/cc" "$t/nolinks" "$t/none" "NOT PURE: cc"
expect "allowlisted foo-sys passes" 0 "$t/sys" "$t/nolinks" "$t/allow" "allowed"
expect "allowlist is per crate" 1 "$t/cc" "$t/nolinks" "$t/allow" "NOT PURE: cc"
expect "allowlist line without date is an error" 1 "$t/sys" "$t/nolinks" "$t/badallow" "must be"
expect "links crate not allowlisted fails" 1 "$t/zl" "$t/links" "$t/none" "NOT PURE: zlib-native"
expect "allowlisted links crate passes" 0 "$t/zl" "$t/links" "$t/allow" "allowed (see"
expect "a name that merely contains an allowed one still fails" 1 "$t/zl" "$t/links" <(printf 'zlib | x | 2026-10-26\n') "NOT PURE: zlib-native"
expect "a links crate absent from the tree is ignored (metadata unifies dev features)" 0 "$t/clean" "$t/links" "$t/none"
TREE_RC=1 expect "cargo tree failing fails closed" 1 "$t/clean" "$t/nolinks" "$t/none" "unexpected cargo tree"
expect "empty tree fails closed" 1 "$t/empty" "$t/nolinks" "$t/none" "unexpected cargo tree"
expect "unexpected tree format fails closed" 1 "$t/badfmt" "$t/nolinks" "$t/none" "unexpected cargo tree"
HIT=1 expect "hit on the first per-package check fails" 1 "$t/clean" "$t/nolinks" "$t/none" "NOT PURE: ring"
[ $fail = 0 ] && echo "all passed"
exit $fail
