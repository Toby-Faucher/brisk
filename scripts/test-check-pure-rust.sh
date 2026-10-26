#!/bin/bash
# Stubbed test of check-pure-rust.sh: a fake `cargo` serves a canned `cargo tree`; no build happens.
set -u
s=$(cd "$(dirname "$0")" && pwd)/check-pure-rust.sh
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT
mkdir "$t/bin"
# `-i <pkg>` (the per-package checks) finds nothing; the plain listing prints $TREE.
cat > "$t/bin/cargo" <<'S'
#!/bin/bash
for a in "$@"; do [ "$a" = -i ] && { echo "error: package ID specification did not match any packages" >&2; exit 101; }; done
cat "$TREE"
S
chmod +x "$t/bin/cargo"
export PATH=$t/bin:$PATH
printf 'brisk-proto v0.0.0\nserde v1.0.1\nlibc v0.2.1\n' > "$t/clean"
printf 'brisk-proto v0.0.0\nfoo-sys v0.1.0\n' > "$t/sys"
printf 'brisk-proto v0.0.0\nwayland_sys v0.1.0\n' > "$t/sys2"
printf 'brisk-proto v0.0.0\ncc v1.0.0\n' > "$t/cc"
: > "$t/none"
printf '# comment\nfoo-sys | vendored zlib, no pure alternative | 2026-10-26\n' > "$t/allow"
printf 'foo-sys | no date\n' > "$t/badallow"
fail=0
expect() { # name want-rc tree allow [text that must appear]
    out=$(TREE=$3 PURE_ALLOW_FILE=$4 "$s" 2>&1); rc=$?
    { [ "$rc" = "$2" ] && { [ -z "${5:-}" ] || echo "$out" | grep -qF -- "$5"; }; } \
        || { echo "FAIL: $1 (rc=$rc)"; echo "$out"; fail=1; }
}
expect "clean graph passes" 0 "$t/clean" "$t/none"
expect "foo-sys fails" 1 "$t/sys" "$t/none" "NOT PURE: foo-sys"
expect "wayland_sys fails" 1 "$t/sys2" "$t/none" "NOT PURE: wayland_sys"
expect "cc fails" 1 "$t/cc" "$t/none" "NOT PURE: cc"
expect "allowlisted foo-sys passes" 0 "$t/sys" "$t/allow" "allowed"
expect "allowlist is per crate" 1 "$t/cc" "$t/allow" "NOT PURE: cc"
expect "allowlist line without date is an error" 1 "$t/sys" "$t/badallow" "must be"
[ $fail = 0 ] && echo "all passed"
exit $fail
