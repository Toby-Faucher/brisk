#!/bin/sh
# Purity guard: fails if a C/asm crypto or build-time C crate is in the product's dependency
# graph (normal + build edges, every native target). Dev-only crates do not count.
# Targets are listed instead of `--target all`: quinn-proto 0.11 depends on ring unconditionally
# for wasm32-unknown-unknown only (browser builds, which Brisk never makes); every native target
# is checked.
# Second check: the Linux targets (x86_64 and aarch64 gnu, default features) may not contain ANY
# `*-sys` / `*_sys` crate, build-time C toolchain crate (cc cmake bindgen pkg-config vcpkg
# cxx-build) or crate with a `links` key (binds a native library) unless it is listed in
# scripts/pure-rust-allow.txt (crate | reason | date). Fails closed: an unparsable or empty
# `cargo tree` is an error. Windows/macOS are covered only by the per-package checks above
# (windows-sys and friends exist only there).
# Run from anywhere: scripts/check-pure-rust.sh   (needs cargo, python3; exit 0 = pure, 1 = not)
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

# --- stricter scan: every -sys crate, C build tool and `links` crate, minus the allowlist ---
allow=${PURE_ALLOW_FILE:-scripts/pure-rust-allow.txt}
entries=$(grep -v '^[[:space:]]*\(#\|$\)' "$allow" 2>/dev/null)
if [ -n "$entries" ] && ! echo "$entries" | awk -F'|' 'NF<3 || $1 ~ /^[[:space:]]*$/ || $2 ~ /^[[:space:]]*$/ || $3 ~ /^[[:space:]]*$/ {exit 1}'; then
    echo "ERROR: lines in $allow must be 'crate | reason | date'"; exit 1
fi
allowed=$(echo "$entries" | awk -F'|' '{gsub(/[[:space:]]/,"",$1); print $1}')
# Names of normal/build-reachable packages that declare `links`, from `cargo metadata` JSON on stdin.
links_py='
import json, sys
m = json.load(sys.stdin)
pk = {p["id"]: p for p in m["packages"]}
nodes = {n["id"]: n for n in m["resolve"]["nodes"]}
seen, todo = set(), list(m["workspace_members"])
while todo:
    i = todo.pop()
    if i in seen:
        continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        if any(k["kind"] in (None, "build") for k in d["dep_kinds"]):
            todo.append(d["pkg"])
for i in sorted(seen):
    if pk[i].get("links"):
        print(pk[i]["name"])
'
for tgt in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
    tree=$(cargo tree --workspace -e normal,build --target "$tgt" --prefix none 2>"$err")
    if [ $? -ne 0 ] || ! echo "$tree" | grep -q '^brisk-hostd v'; then
        echo "ERROR: unexpected cargo tree output for $tgt (failed, empty or not the expected format):"; cat "$err"; exit 1
    fi
    meta=$(cargo metadata --format-version 1 --filter-platform "$tgt" 2>"$err")
    names=$(echo "$meta" | python3 -c "$links_py" 2>>"$err")
    if [ $? -ne 0 ]; then echo "ERROR reading cargo metadata for $tgt:"; cat "$err"; exit 1; fi
    # `cargo metadata` unifies features across dev-deps too (it lists ring via rustls, which the
    # normal+build tree does not), so a links crate only counts if the tree has it as well.
    treenames=$(echo "$tree" | awk '{print $1}' | sort -u)
    names=$(echo "$names" | while read -r n; do [ -n "$n" ] && echo "$treenames" | grep -qxF -- "$n" && echo "$n"; done)
    for c in $treenames; do
        case $c in
            *-sys|*_sys|cc|cmake|bindgen|pkg-config|vcpkg|cxx-build) ;;
            *) echo "$names" | grep -qxF -- "$c" || continue ;;
        esac
        if echo "$allowed" | grep -qxF -- "$c"; then
            echo "allowed (see $allow): $c [$tgt]"
        else
            echo "NOT PURE: $c is a -sys / links / C build-tool crate and is not in $allow [$tgt]"; bad=1
        fi
    done
done
[ $bad -eq 0 ] && echo "ok: no unlisted -sys, links or C build-tool crates"
exit $bad
