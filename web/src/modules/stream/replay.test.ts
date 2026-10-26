import { describe, expect, it } from 'vitest';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { nals } from '../../lib/video';
import { KEY_EVERY_MS, OPUS_FRAMES, Replay, selectClip } from './replay';
import { muxClip, opusHead } from './mp4';

const fx = (n: string) => new Uint8Array(readFileSync(join(__dirname, 'fixtures', n)));

/** Access units of an Annex B stream, split at each AUD (H.264 type 9, HEVC type 35), start codes kept. */
function accessUnits(es: Uint8Array, hevc: boolean): { data: Uint8Array; key: boolean }[] {
  const aud: number[] = [];
  for (let i = 0; i + 4 < es.length; i++) {
    if (es[i] === 0 && es[i + 1] === 0 && es[i + 2] === 0 && es[i + 3] === 1) {
      const t = hevc ? (es[i + 4] >> 1) & 0x3f : es[i + 4] & 0x1f;
      if (t === (hevc ? 35 : 9)) aud.push(i);
    }
  }
  return aud.map((s, k) => {
    const data = es.slice(s, aud[k + 1] ?? es.length);
    const types = nals(data).map((n) => (hevc ? (n[0] >> 1) & 0x3f : n[0] & 0x1f));
    return { data, key: types.some((t) => (hevc ? t === 19 || t === 20 : t === 5)) };
  });
}

/** Opus packets of an Ogg file (the OpusHead and OpusTags packets skipped). */
function oggPackets(b: Uint8Array): Uint8Array[] {
  const out: Uint8Array[] = [];
  let cur: number[] = [];
  for (let o = 0; o < b.length; ) {
    const nseg = b[o + 26];
    let p = o + 27 + nseg;
    for (let s = 0; s < nseg; s++) {
      const len = b[o + 27 + s];
      for (let k = 0; k < len; k++) cur.push(b[p + k]);
      p += len;
      if (len < 255) (out.push(Uint8Array.from(cur)), (cur = []));
    }
    o = p;
  }
  return out.slice(2);
}

const FRAME_MS = 1000 / 60;
/** Feeds access units at 60 fps (bridge clock = arrival + 7 ms) and the Opus packets (5 ms, RTP base 4800). */
function feed(r: Replay, aus: { data: Uint8Array; key: boolean }[], opus: Uint8Array[] = [], skip = new Set<number>()) {
  let k = 0;
  for (let i = 0; i < aus.length; i++) {
    const at = i * FRAME_MS;
    for (; k < opus.length && k * 5 <= at; k++) if (!skip.has(k)) r.audio({ seq: k & 0xffff, ts48: (4800 + k * OPUS_FRAMES) >>> 0, opus: opus[k] }, k * 5 + 1);
    r.video(aus[i].data, aus[i].key, BigInt(Math.round((at - 7) * 1e6)), at);
  }
  return (aus.length - 1) * FRAME_MS;
}

const h264 = accessUnits(fx('h264.h264'), false); // 180 frames 320x180, then 60 frames 256x144
const hevc = accessUnits(fx('hevc.hevc'), true);
const high = accessUnits(fx('high.h264'), false); // High profile, 3 s
const opus = oggPackets(fx('opus.ogg'));

describe('fixtures', () => {
  it('split_like_the_bridge_sends_them', () => {
    expect(h264.length).toBe(240);
    expect(hevc.length).toBe(180);
    expect(h264.filter((f) => f.key).length).toBe(4);
    expect(hevc.filter((f) => f.key).length).toBe(3);
    expect(opus.length).toBe(601);
    expect(new Set(opus.map((p) => p[0] >> 3))).toEqual(new Set([29])); // CELT FB 5 ms, like Sunshine
  });
});

describe('Replay ring', () => {
  it('disabled_keeps_nothing_and_never_asks_for_a_key_frame', () => {
    const r = new Replay(0);
    feed(r, h264, opus);
    expect(r.bytes).toBe(0);
    expect(r.frames).toBe(0);
    expect(r.clip()).toBeNull();
    expect(r.refreshDue(1e9)).toBe(false);
  });

  it('evicts_by_time_but_keeps_the_last_key_frame_at_or_before_now_minus_n', () => {
    const r = new Replay(1);
    const now = feed(r, h264.slice(0, 180), opus);
    // keys at 0, 1, 2 s; now 2.98 s, cutoff 1.98 s: the GOP at 0 goes, the one at 1 s stays
    expect(r.frames).toBe(120);
    const c = r.clip(now)!;
    expect(c.video[0].key).toBe(true);
    expect(c.video.length).toBe(120);
    expect(c.seconds).toBeGreaterThanOrEqual(1); // at least N s
    expect(c.seconds).toBeCloseTo(2, 1);
  });

  it('never_evicts_the_only_key_frame', () => {
    // A refresh that never came: one key frame, then 4 s of deltas with N = 1 s.
    const r = new Replay(1);
    r.video(h264[0].data, true, 0n, 0);
    for (let i = 1; i < 240; i++) r.video(h264[(i % 59) + 1].data, false, BigInt(Math.round(i * FRAME_MS * 1e6)), i * FRAME_MS);
    expect(r.frames).toBe(240);
    expect(r.clip(240 * FRAME_MS)!.video.length).toBe(240);
  });

  it('evicts_by_bytes_counting_whole_buffers_and_never_exceeds_the_cap', () => {
    const cap = 60_000; // about 1.4 GOPs of the fixture
    const r = new Replay(60, cap);
    let max = 0;
    for (let i = 0; i < 180; i++) {
      r.video(h264[i].data, h264[i].key, BigInt(i * 16_666_667), i * FRAME_MS);
      max = Math.max(max, r.bytes);
    }
    expect(max).toBeLessThanOrEqual(cap);
    expect(r.frames).toBeGreaterThan(0);
    expect(r.clip(180 * FRAME_MS)!.video[0].key).toBe(true);
    // A small view of a big buffer counts as the buffer it keeps alive.
    const big = new Replay(60, 1 << 20);
    big.video(new Uint8Array(new ArrayBuffer(4 << 20), 0, 100).fill(0), true, 0n, 0);
    expect(big.bytes).toBeGreaterThan(4 << 20);
  });

  it('one_gop_over_the_cap_pauses_until_the_next_key_frame', () => {
    const r = new Replay(60, 30_000); // under one GOP (~42 KB)
    for (let i = 0; i < 61; i++) r.video(h264[i].data, h264[i].key, BigInt(i * 16_666_667), i * FRAME_MS);
    expect(r.bytes).toBeLessThanOrEqual(30_000);
    const c = r.clip(61 * FRAME_MS)!;
    expect(c.video[0].key).toBe(true); // the newest key frame (frame 60) on its own, or the cut GOP
  });

  it('clear_drops_memory_and_kills_the_ring', () => {
    const r = new Replay(30);
    feed(r, h264, opus);
    expect(r.bytes).toBeGreaterThan(100_000);
    r.clear();
    expect(r.bytes).toBe(0);
    expect(r.clip()).toBeNull();
    r.video(h264[0].data, true, 0n, 0); // a late frame of the closed connection
    expect(r.frames).toBe(0);
    expect(r.refreshDue(1e9)).toBe(false);
  });

  it('nothing_to_save_without_a_key_frame', () => {
    expect(selectClip([], [], 0)).toBeNull();
    const deltas = h264.slice(1, 30).map((f, i) => ({ data: f.data, key: false, ns: i * 1e7, at: i }));
    expect(selectClip(deltas, [], 0)).toBeNull();
    const r = new Replay(30);
    for (const f of h264.slice(1, 30)) r.video(f.data, false, 0n, 0);
    expect(r.snapshot(0)).toBeNull();
  });

  it('asks_for_a_refresh_every_10_s_while_on', () => {
    expect(KEY_EVERY_MS).toBe(10_000);
    const r = new Replay(30);
    expect(r.refreshDue(0)).toBe(true); // nothing yet
    expect(r.refreshDue(KEY_EVERY_MS - 1)).toBe(false); // asked: not again for 10 s
    r.video(h264[0].data, true, 0n, 5000);
    expect(r.refreshDue(5000 + KEY_EVERY_MS - 1)).toBe(false);
    expect(r.refreshDue(5000 + KEY_EVERY_MS)).toBe(true);
  });
});

describe('selectClip', () => {
  const ring = (aus: { data: Uint8Array; key: boolean }[]) => aus.map((f, i) => ({ data: f.data, key: f.key, ns: (i * FRAME_MS - 7) * 1e6, at: i * FRAME_MS }));

  it('last_key_frame_at_or_before_now_minus_n', () => {
    expect(selectClip(ring(h264.slice(0, 180)), [], 1.5 * 1000)!.video.length).toBe(120); // keys at 0, 1, 2 s: picks 1 s
    expect(selectClip(ring(h264.slice(0, 180)), [], 1.01 * 1000)!.video.length).toBe(120);
    const c = selectClip(ring(h264.slice(0, 180)), [], 0.5 * 1000)!;
    expect(c.video.length).toBe(180);
    expect(c.video[0].t).toBe(0);
  });

  it('else_the_earliest_key_frame', () => {
    const c = selectClip(ring(h264.slice(0, 150)), [], -100)!; // every key is after now - N
    expect(c.video.length).toBe(150);
  });

  it('clips_only_the_last_config_after_a_resolution_change', () => {
    const c = selectClip(ring(h264), [], -1e9)!;
    expect(c.video.length).toBe(60); // the 256x144 tail
    expect(c.codec).toMatch(/^avc1\./);
  });

  it('audio_gaps_filled_by_timestamp_without_drift_and_ts_wrap', () => {
    const v = ring(h264.slice(0, 120));
    // RTP ts wraps mid-clip; packets 100..139 lost (200 ms); 2 reordered.
    const base = 0xffffffff - 100 * OPUS_FRAMES;
    const a = opus.slice(0, 400).map((p, k) => ({ data: p, ts: (base + k * OPUS_FRAMES) >>> 0, at: k * 5 + 1 })).filter((_, k) => k < 100 || k >= 140);
    [a[50], a[51]] = [a[51], a[50]];
    const c = selectClip(v, a, -1e9)!;
    expect(c.audio.length).toBe(400);
    for (let k = 1; k < c.audio.length; k++) expect(c.audio[k].t - c.audio[k - 1].t).toBeCloseTo(0.005, 9);
    expect(c.audio.filter((p) => p.data.length === 1).length).toBe(40);
    // Each clock mapped by its least-queued item: packet 0 (arrived at 1 ms) lands 1 ms after frame 0 (arrived at 0).
    expect(c.audio[0].t).toBeCloseTo(0.001, 9);
  });
});

const ff = existsSync('/usr/bin/ffprobe') && existsSync('/usr/bin/ffmpeg');
describe.skipIf(!ff)('muxClip (ffprobe / ffmpeg)', () => {
  const dir = mkdtempSync(join(tmpdir(), 'brisk-clip-'));
  for (const [name, aus, w, h] of [['h264', h264.slice(0, 180), 320, 180], ['high', high, 320, 180], ['hevc', hevc, 320, 180]] as const) {
    it(`${name}_with_opus_probes_and_decodes_clean`, async () => {
      const r = new Replay(60);
      const now = feed(r, aus, opus, new Set([300, 301, 302]));
      const c = r.clip(now)!;
      const mp4 = new Uint8Array(await (await muxClip(c, w, h)).arrayBuffer());
      const f = join(dir, `${name}.mp4`);
      writeFileSync(f, mp4);
      if (process.env.BRISK_CLIP_OUT) writeFileSync(join(process.env.BRISK_CLIP_OUT, `${name}.mp4`), mp4); // for scripts/clip-check.mjs
      const probe = JSON.parse(execFileSync('ffprobe', ['-v', 'error', '-show_streams', '-show_format', '-of', 'json', f]).toString());
      const [vs, as] = probe.streams;
      expect(vs.codec_name).toBe(name === 'high' ? 'h264' : name);
      expect([vs.width, vs.height]).toEqual([w, h]);
      expect(Number(vs.nb_frames)).toBe(aus.length);
      expect(as.codec_name).toBe('opus');
      expect(as.channels).toBe(2);
      expect(Number(probe.format.duration)).toBeCloseTo(3, 1);
      const dec = spawnSync('ffmpeg', ['-v', 'error', '-i', f, '-f', 'null', '-']);
      expect([dec.status, dec.stderr.toString()]).toEqual([0, '']);
      if (name === 'high') {
        // avcC of a High (100) profile stream carries the ISO 14496-15 extension: chroma_format, bit depths.
        const b = mp4.subarray(Buffer.from(mp4).indexOf('avcC') + 4);
        expect(b[1]).toBe(100);
        let o = 6;
        const nSps = b[5] & 0x1f;
        for (let k = 0; k < nSps; k++) o += 2 + ((b[o] << 8) | b[o + 1]);
        const nPps = b[o++];
        for (let k = 0; k < nPps; k++) o += 2 + ((b[o] << 8) | b[o + 1]);
        expect(o + 4).toBeLessThanOrEqual((mp4[Buffer.from(mp4).indexOf('avcC') - 2] << 8 | mp4[Buffer.from(mp4).indexOf('avcC') - 1]) - 8);
        expect([b[o] & 0xfc, b[o] & 3, b[o + 1] & 0xf8, b[o + 1] & 7, b[o + 2] & 0xf8, b[o + 2] & 7]).toEqual([0xfc, 1, 0xf8, 0, 0xf8, 0]); // 4:2:0, 8-bit
        expect(vs.profile).toBe('High');
      }
      // pre-skip 120 in dOps
      const i = Buffer.from(mp4).indexOf('dOps');
      expect(mp4[i + 6] << 8 | mp4[i + 7]).toBe(120);
    });
  }
});

describe('opusHead', () => {
  it('is_rfc7845', () => {
    const h = opusHead();
    expect(new TextDecoder().decode(h.subarray(0, 8))).toBe('OpusHead');
    expect([h[9], h[10] | (h[11] << 8), new DataView(h.buffer).getUint32(12, true), h[18]]).toEqual([2, 120, 48000, 0]);
  });
});
