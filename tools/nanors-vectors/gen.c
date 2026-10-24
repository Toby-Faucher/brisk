/* Dev-only nanors cross-check vector generator (never linked into brisk). See README.md.
 *
 * Output (all integers little-endian):
 *   "NRSV0001" u32 case_count
 *   per case: u8 ds, u8 ps, u16 bs, u8 nlost, u8 lost[nlost] (shard indices, any of ds+ps),
 *             data[ds*bs], parity[ps*bs] (reed_solomon_encode),
 *             recovered[ds*bs] (reed_solomon_decode after erasing `lost`; must equal data)
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "rs.h"

static uint64_t st = 0x6272697366656321ull; /* fixed seed: output is reproducible */
static uint32_t rnd(void)
{
    st ^= st << 13;
    st ^= st >> 7;
    st ^= st << 17;
    return (uint32_t)(st >> 16);
}
static int range(int lo, int hi) { return lo + (int)(rnd() % (uint32_t)(hi - lo + 1)); }

static void put(FILE *f, const void *p, size_t n)
{
    if (fwrite(p, 1, n, f) != n) {
        perror("write");
        exit(1);
    }
}

static void one(FILE *f, int ds, int ps, int bs)
{
    int ts = ds + ps;
    uint8_t **sh = calloc(ts, sizeof *sh), *orig = malloc((size_t)ds * bs), marks[DATA_SHARDS_MAX] = {0};
    for (int i = 0; i < ts; i++)
        sh[i] = malloc(bs);
    for (int i = 0; i < ds; i++)
        for (int b = 0; b < bs; b++)
            sh[i][b] = (uint8_t)rnd();
    reed_solomon *rs = reed_solomon_new(ds, ps);
    if (!rs || reed_solomon_encode(rs, sh, ts, bs)) {
        fprintf(stderr, "encode failed ds=%d ps=%d\n", ds, ps);
        exit(1);
    }
    for (int i = 0; i < ds; i++)
        memcpy(orig + (size_t)i * bs, sh[i], bs);

    uint8_t nlost = (uint8_t)range(0, ps), lost[DATA_SHARDS_MAX];
    for (int n = 0; n < nlost;) {
        int idx = range(0, ts - 1);
        if (!marks[idx]) {
            marks[idx] = 1;
            lost[n++] = (uint8_t)idx;
        }
    }
    uint8_t h[5] = {(uint8_t)ds, (uint8_t)ps, (uint8_t)bs, (uint8_t)(bs >> 8), nlost};
    put(f, h, 5);
    put(f, lost, nlost);
    for (int i = 0; i < ts; i++)
        put(f, sh[i], bs);

    for (int i = 0; i < nlost; i++)
        memset(sh[lost[i]], 0xAA, bs); /* garbage in erased slots */
    if (reed_solomon_decode(rs, sh, marks, ts, bs)) {
        fprintf(stderr, "decode failed ds=%d ps=%d\n", ds, ps);
        exit(1);
    }
    for (int i = 0; i < ds; i++) {
        if (memcmp(sh[i], orig + (size_t)i * bs, bs)) {
            fprintf(stderr, "nanors mis-recovered ds=%d ps=%d\n", ds, ps);
            exit(1);
        }
        put(f, sh[i], bs);
    }
    reed_solomon_release(rs);
    for (int i = 0; i < ts; i++)
        free(sh[i]);
    free(sh);
    free(orig);
}

int main(int argc, char **argv)
{
    if (argc != 2) {
        fprintf(stderr, "usage: %s OUT\n", argv[0]);
        return 2;
    }
    reed_solomon_init();
    FILE *f = fopen(argv[1], "wb");
    if (!f) {
        perror("open");
        return 1;
    }
    /* edge schemes first, then random ones */
    static const int fixed[][3] = {{1, 1, 1}, {1, 254, 2}, {254, 1, 3}, {200, 55, 4}, {128, 127, 2}, {2, 2, 1040}};
    const uint32_t nfixed = sizeof fixed / sizeof fixed[0], nrand = 250, count = nfixed + nrand;
    put(f, "NRSV0001", 8);
    uint8_t c[4] = {(uint8_t)count, (uint8_t)(count >> 8), (uint8_t)(count >> 16), (uint8_t)(count >> 24)};
    put(f, c, 4);
    for (uint32_t i = 0; i < nfixed; i++)
        one(f, fixed[i][0], fixed[i][1], fixed[i][2]);
    for (uint32_t i = 0; i < nrand; i++) {
        int ds = range(1, 80);
        int ps = range(1, (255 - ds) < 40 ? (255 - ds) : 40);
        one(f, ds, ps, range(1, 48));
    }
    fclose(f);
    return 0;
}
