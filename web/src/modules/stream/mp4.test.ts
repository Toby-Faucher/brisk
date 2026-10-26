import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { OPUS_PRESKIP, muxClip, opusHead } from './mp4';
import type { Clip } from './replay';

describe('opusHead', () => {
  it('is_rfc7845_stereo_48k', () => {
    const h = opusHead();
    expect(new TextDecoder().decode(h.subarray(0, 8))).toBe('OpusHead');
    const v = new DataView(h.buffer);
    expect([h[8], h[9], v.getUint16(10, true), v.getUint32(12, true), h[18]]).toEqual([1, 2, OPUS_PRESKIP, 48000, 0]);
    expect(new DataView(opusHead(312).buffer).getUint16(10, true)).toBe(312);
  });
});

describe('muxClip', () => {
  it('video_only_clip_is_a_non_empty_mp4', async () => {
    const es = new Uint8Array(readFileSync(join(__dirname, 'fixtures', 'h264.h264')));
    const aud: number[] = []; // access units start at each AUD (NAL type 9)
    for (let i = 0; i + 4 < es.length && aud.length < 31; i++) if (es[i] === 0 && es[i + 1] === 0 && es[i + 2] === 0 && es[i + 3] === 1 && (es[i + 4] & 0x1f) === 9) aud.push(i);
    const video = aud.slice(0, 30).map((s, k) => ({ data: es.slice(s, aud[k + 1]), key: k === 0, t: k / 60, d: 1 / 60 }));
    const clip: Clip = { kind: 'avc', codec: 'avc1.640014', video, audio: [], seconds: 0.5, bytes: 0 };
    const blob = await muxClip(clip, 320, 180);
    expect(blob.type).toBe('video/mp4');
    expect(blob.size).toBeGreaterThan(1000);
    expect(new TextDecoder().decode(new Uint8Array(await blob.slice(4, 8).arrayBuffer()))).toBe('ftyp');
  });
});
