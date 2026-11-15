import { describe, expect, it } from 'vitest';
import { MIC_FRAMES, MIC_PACKET_LEN, MicPacker, packMic } from './mic';

const frames = (n: number, from = 0) => Float32Array.from({ length: n }, (_, i) => (from + i) / 100000);

describe('packMic', () => {
  it('writes the header and clamps', () => {
    const f = new Float32Array(MIC_FRAMES);
    f.set([0, 1, -1, 2]);
    const p = packMic(0x10203, f);
    expect(p.length).toBe(MIC_PACKET_LEN);
    expect([...p.subarray(0, 3)]).toEqual([0x30, 0x03, 0x02]);
    const v = new DataView(p.buffer);
    expect(v.getInt16(3 + 2, true)).toBe(32767);
    expect(v.getInt16(3 + 4, true)).toBe(-32767);
    expect(v.getInt16(3 + 6, true)).toBe(32767);
  });
});

describe('MicPacker', () => {
  it('makes 10 ms packets from 128-frame blocks', () => {
    const m = new MicPacker();
    const out: Uint8Array[] = [];
    for (let i = 0; i < 4; i++) out.push(...m.push(frames(128, i * 128)));
    expect(out.length).toBe(1);
    expect(new DataView(out[0].buffer).getUint16(1, true)).toBe(0);
    for (let i = 4; i < 8; i++) out.push(...m.push(frames(128, i * 128)));
    expect(out.length).toBe(2);
    expect(new DataView(out[1].buffer).getUint16(1, true)).toBe(1);
    expect(new DataView(out[1].buffer).getInt16(3, true)).toBe(Math.round((480 / 100000) * 32767));
  });
  it('seq wraps', () => {
    const m = new MicPacker();
    for (let i = 0; i < 65536; i++) m.push(new Float32Array(MIC_FRAMES));
    const p = m.push(new Float32Array(MIC_FRAMES))[0];
    expect(new DataView(p.buffer).getUint16(1, true)).toBe(0);
  });
});
