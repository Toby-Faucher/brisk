#!/bin/sh
# Purity guard: fails if a C/asm crypto or build-time C crate is in the product's dependency
# graph (normal + build edges, every native target). Dev-only crates do not count.
# Targets are listed instead of `--target all`: quinn-proto 0.11 depends on ring unconditionally
# for wasm32-unknown-unknown only (browser builds, which Brisk never makes); every native target
# is checked.
# Second check: the real target (x86_64-unknown-linux-gnu, default features) may not contain ANY
# `*-sys` / `*_sys` crate or build-time C toolchain crate (cc cmake bindgen pkg-config vcpkg
# cxx-build) unless it is listed in scripts/pure-rust-allow.txt (crate | reason | date).
# Run from anywhere: scripts/check-pure-rust.sh   (needs cargo; exit 0 = pure, 1 = not)
# Test: scripts/test-check-pure-rust.sh (stub cargo). PURE_ALLOW_FILE overrides the allowlist path.
set -u
cd "$(dirname "$0")/.." || exit 2
err=$(mktemp) || exit 2
trap 'rm -f "$err"' EXIT
bad=0
targets=""
for t in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin \
    x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do targets="$targets --target $t"; done
for p in ring aws-lc-sys openssl-sys cc; do
    out=$(cargo tree --workspace -e normal,build $targets -i "$p" 2>"$err")
    rc=$?
    if [ -n "$out" ]; then
        echo "NOT PURE: $p is a dependency:"; echo "$out"; bad=1
    elif [ $rc -ne 0 ] && ! grep -q "did not match any packages" "$err"; then
        echo "ERROR checking $p:"; cat "$err"; bad=1
    else
        echo "ok: $p not in the graph"
    fi
done

# --- stricter scan: every -sys crate and C build tool, minus the allowlist ---
allow=${PURE_ALLOW_FILE:-scripts/pure-rust-allow.txt}
tree=$(cargo tree --workspace -e normal,build --target x86_64-unknown-linux-gnu --prefix none 2>"$err")
if [ $? -ne 0 ] || [ -z "$tree" ]; then
    echo "ERROR listing the dependency graph:"; cat "$err"; exit 1
fi
entries=$(grep -v '^[[:space:]]*\(#\|$\)' "$allow" 2>/dev/null)
if [ -n "$entries" ] && ! echo "$entries" | awk -F'|' 'NF<3 || $1 ~ /^[[:space:]]*$/ || $2 ~ /^[[:space:]]*$/ || $3 ~ /^[[:space:]]*$/ {exit 1}'; then
    echo "ERROR: lines in $allow must be 'crate | reason | date'"; exit 1
fi
allowed=$(echo "$entries" | awk -F'|' '{gsub(/[[:space:]]/,"",$1); print $1}')
for c in $(echo "$tree" | awk '{print $1}' | sort -u); do
    case $c in
        *-sys|*_sys|cc|cmake|bindgen|pkg-config|vcpkg|cxx-build) ;;
        *) continue ;;
    esac
    if echo "$allowed" | grep -qx -- "$c"; then
        echo "allowed (see $allow): $c"
    else
        echo "NOT PURE: $c is a -sys / C build-tool crate and is not in $allow"; bad=1
    fi
done
[ $bad -eq 0 ] && echo "ok: no unlisted -sys or C build-tool crates"
exit $bad
