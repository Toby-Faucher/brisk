#!/bin/sh
# Purity guard: fails if a C/asm crypto or build-time C crate is in the product's dependency
# graph (normal + build edges, every native target). Dev-only crates do not count.
# Targets are listed instead of `--target all`: quinn-proto 0.11 depends on ring unconditionally
# for wasm32-unknown-unknown only (browser builds, which Brisk never makes); every native target
# is checked.
# Run from anywhere: scripts/check-pure-rust.sh   (needs cargo; exit 0 = pure, 1 = not)
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
exit $bad
