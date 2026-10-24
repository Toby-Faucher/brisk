# nanors cross-check vectors

`vectors.bin` holds 256 Reed-Solomon cases (6 edge schemes + 250 random ones; data shards
1..254, parity 1..254, data+parity <= 255, shard size 1..1040, random loss of 0..parity shards)
encoded and decoded by nanors itself. `crates/brisk-proto/tests/fec.rs` checks that the pure-Rust
`brisk_proto::fec` gives the same parity and the same recovered data. The format is
described at the top of `gen.c`. Output is deterministic (fixed xorshift seed).

`gen.c` is dev-only. It is never built by cargo and never linked into brisk.

## Regenerating

Use the nanors copy vendored in moonlight-common-c (`nanors/`, submodule commit `b1e3c22`), and
build in a temporary directory, not inside the source checkout:

```sh
NANORS=/path/to/moonlight-common-c/nanors
B=$(mktemp -d)
cp -r "$NANORS"/rs.c "$NANORS"/rs.h "$NANORS"/deps tools/nanors-vectors/gen.c "$B"/
(cd "$B" && gcc -O2 -std=c11 -Wall -I. -Ideps/obl gen.c rs.c deps/obl/oblas_common.c deps/obl/oblas_lite.c -o gen)
"$B"/gen tools/nanors-vectors/vectors.bin
sha256sum tools/nanors-vectors/vectors.bin
# 573f91f2290faade9ff10234f54b77affcf5ae236700f4dffd4c29f63797bec3 (gcc 16.2.1, x86_64)
```

`gen` exits non-zero if nanors fails to encode or decode a case, or recovers the wrong data.
