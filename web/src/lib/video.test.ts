import { describe, expect, it } from 'vitest';
import { codecFromSps, findSps, KeyGate, parseHeader, Window, HEADER_LEN } from './video';

const hex = (s: string) => Uint8Array.from(s.match(/../g)!.map((b) => parseInt(b, 16)));

// The first SPS of the live tap run (Task 3, Sunshine v2026.914.233613, VAAPI H.264 1080p60).
const SUNSHINE_SPS = hex('67640c2aac2b403c0113f2cd40404050000003001000000788f08042a0');

describe('codecFromSps', () => {
  it('reads profile_idc, constraint flags and level_idc of the real Sunshine SPS', () => {
    expect(codecFromSps(SUNSHINE_SPS)).toBe('avc1.640c2a');
  });
  it('pads each byte to two hex digits', () => {
    expect(codecFromSps(hex('6742e00a'))).toBe('avc1.42e00a');
  });
  it('rejects what is not an SPS', () => {
    expect(() => codecFromSps(hex('68ce3c80'))).toThrow(); // PPS
    expect(() => codecFromSps(hex('6764'))).toThrow();
    expect(() => codecFromSps(hex('67000000'))).toThrow(); // profile 0
  });
});

describe('findSps', () => {
  const pps = hex('68ce3c80');
  const idr = hex('6588840021');
  const au = (sc: string) => new Uint8Array([...hex(sc), ...SUNSHINE_SPS, ...hex(sc), ...pps, ...hex(sc), ...idr]);
  it('finds the SPS between 4-byte start codes', () => {
    expect(findSps(au('00000001'))).toEqual(SUNSHINE_SPS);
  });
  it('finds the SPS between 3-byte start codes', () => {
    expect(findSps(au('000001'))).toEqual(SUNSHINE_SPS);
  });
  it('finds an SPS that is not the first NAL, or none', () => {
    const b = new Uint8Array([...hex('00000001'), ...hex('09f0'), ...hex('00000001'), ...SUNSHINE_SPS]);
    expect(findSps(b)).toEqual(SUNSHINE_SPS);
    expect(findSps(new Uint8Array([...hex('00000001'), ...idr]))).toBeNull();
  });
});

describe('parseHeader', () => {
  it('reads the little-endian 21-byte header', () => {
    const b = new Uint8Array(HEADER_LEN + 2);
    const v = new DataView(b.buffer);
    v.setUint32(0, 0xfffffffe, true);
    b[4] = 1;
    v.setBigUint64(5, 123456789012345n, true);
    v.setBigUint64(13, 123456795012345n, true);
    const h = parseHeader(b);
    expect(h).toEqual({ idx: 0xfffffffe, idr: true, captureNs: 123456789012345n, bridgeNs: 123456795012345n });
    b[4] = 2;
    expect(parseHeader(b).idr).toBe(false);
  });
  it('rejects a short frame', () => {
    expect(() => parseHeader(new Uint8Array(20))).toThrow();
  });
});

describe('KeyGate (drop until key)', () => {
  it('waits for a key frame at start, then takes deltas in order', () => {
    const g = new KeyGate();
    expect(g.accept(5, false, 0).decode).toBe(false);
    expect(g.accept(6, true, 0)).toEqual({ decode: true, askKey: false });
    expect(g.accept(7, false, 0).decode).toBe(true);
    expect(g.accept(8, false, 0).decode).toBe(true);
  });
  it('a gap breaks the chain: drop and ask until the next key frame', () => {
    const g = new KeyGate();
    g.accept(0, true, 0);
    expect(g.accept(2, false, 0)).toEqual({ decode: false, askKey: true });
    expect(g.accept(3, false, 0).decode).toBe(false);
    expect(g.accept(1, false, 0).decode).toBe(false); // late frame after the gap
    expect(g.accept(4, true, 0).decode).toBe(true);
    expect(g.accept(5, false, 0).decode).toBe(true);
  });
  it('drops out-of-order and stale frames', () => {
    const g = new KeyGate();
    g.accept(10, true, 0);
    g.accept(11, false, 0);
    expect(g.accept(11, false, 0).decode).toBe(false); // duplicate
    expect(g.accept(9, true, 0).decode).toBe(false); // an older key frame
  });
  it('a decode queue over 2 waits for the next key frame', () => {
    const g = new KeyGate();
    g.accept(0, true, 0);
    expect(g.accept(1, false, 2).decode).toBe(true);
    expect(g.accept(2, false, 3)).toEqual({ decode: false, askKey: true });
    expect(g.accept(3, false, 0)).toEqual({ decode: false, askKey: false });
    expect(g.accept(4, true, 3).decode).toBe(false); // still backed up
    expect(g.accept(5, true, 0).decode).toBe(true);
  });
  it('reset (decoder error) waits for a key frame', () => {
    const g = new KeyGate();
    g.accept(0, true, 0);
    g.reset();
    expect(g.accept(1, false, 0).decode).toBe(false);
    expect(g.accept(2, true, 0).decode).toBe(true);
  });
  it('index wraps at 2^32', () => {
    const g = new KeyGate();
    g.accept(0xffffffff, true, 0);
    expect(g.accept(0, false, 0).decode).toBe(true);
    expect(g.accept(1, true, 0).decode).toBe(true);
  });
});

describe('Window', () => {
  it('keeps the last n samples for nearest-rank percentiles', () => {
    const w = new Window(4);
    expect(w.pct(50)).toBeNull();
    [9, 1, 2, 3, 4].forEach((x) => w.push(x)); // 9 falls out
    expect(w.pct(50)).toBe(2);
    expect(w.pct(100)).toBe(4);
    expect(w.min()).toBe(1);
  });
});
