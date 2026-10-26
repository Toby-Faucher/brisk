import { describe, expect, it, vi } from 'vitest';
import { codecFromAccessUnit, codecFromHevcSps, codecFromSps, findSps, hevcSupported, KeyGate, parseHeader, startVideo, Window, HEADER_LEN } from './video';

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

// x265-style Main profile 1080p SPS (decodes to 1920x1080, 4:2:0, level 4.0); the two 0x000003
// emulation-prevention bytes sit inside the compatibility and constraint flags.
const HEVC_MAIN_SPS = hex('420101016000000300b00000030000030078a003c08010e596666924cae010000003001000000301e080');
const HEVC_VPS = hex('40010c01ffff016000000300b0000003000003007895');
const HEVC_PPS = hex('4401c172b46240');
const HEVC_IDR = hex('2601af0880');

describe('codecFromHevcSps (ISO 14496-15 E.3)', () => {
  it('builds hvc1.1.6.L120.B0 for Main profile level 4.0, through emulation-prevention bytes', () => {
    expect(codecFromHevcSps(HEVC_MAIN_SPS)).toBe('hvc1.1.6.L120.B0');
  });
  it('Main 10 level 5.1: profile 2, compatibility flag 2 reversed = 4', () => {
    expect(codecFromHevcSps(hex('420101022000000300900000030000030099a001e020021c59'))).toBe('hvc1.2.4.L153.90');
  });
  it('profile space, high tier, 3 sub-layers, and only trailing zero constraint bytes dropped', () => {
    // vps 0, max_sub_layers_minus1 2, nesting 1 = 0x05; space 1, tier 1, profile 1 = 0x61
    expect(codecFromHevcSps(hex('420105616000000300800000030000030196'))).toBe('hvc1.A1.6.H150.80.0.0.0.0.1');
  });
  it('rejects what is not an HEVC SPS', () => {
    expect(() => codecFromHevcSps(HEVC_VPS)).toThrow();
    expect(() => codecFromHevcSps(SUNSHINE_SPS)).toThrow();
    expect(() => codecFromHevcSps(hex('42010101600000'))).toThrow(); // cut short
  });
});

describe('codecFromAccessUnit', () => {
  const sc = hex('00000001');
  const join = (...n: Uint8Array[]) => new Uint8Array(n.flatMap((x) => [...sc, ...x]));
  it('H.264: the SPS decides (avc1)', () => {
    expect(codecFromAccessUnit(join(hex('09f0'), SUNSHINE_SPS, hex('68ce3c80'), hex('6588840021')))).toBe('avc1.640c2a');
  });
  it('HEVC: VPS first, then the SPS gives hvc1', () => {
    expect(codecFromAccessUnit(join(HEVC_VPS, HEVC_MAIN_SPS, HEVC_PPS, HEVC_IDR))).toBe('hvc1.1.6.L120.B0');
    expect(codecFromAccessUnit(join(hex('4601'), hex('4e01'), HEVC_VPS, HEVC_MAIN_SPS, HEVC_PPS, HEVC_IDR))).toBe('hvc1.1.6.L120.B0'); // AUD, SEI first
  });
  it('null without parameter sets, or HEVC without its SPS', () => {
    expect(codecFromAccessUnit(join(hex('6588840021')))).toBeNull();
    expect(codecFromAccessUnit(join(HEVC_IDR))).toBeNull();
    expect(codecFromAccessUnit(join(HEVC_VPS, HEVC_PPS, HEVC_IDR))).toBeNull();
  });
});

describe('hevcSupported', () => {
  it('asks isConfigSupported for hvc1 and is false when it says no or throws', async () => {
    const calls: VideoDecoderConfig[] = [];
    const mk = (r: boolean | Error) => ({ isConfigSupported: async (c: VideoDecoderConfig) => (calls.push(c), r instanceof Error ? Promise.reject(r) : { supported: r }) });
    expect(await hevcSupported(mk(true))).toBe(true);
    expect(calls[0].codec).toBe('hvc1.1.6.L120.B0');
    expect(await hevcSupported(mk(false))).toBe(false);
    expect(await hevcSupported(mk(new Error('x')))).toBe(false);
    expect(await hevcSupported(undefined)).toBe(false);
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
    expect(g.accept(3, false, 0)).toEqual({ decode: false, askKey: true }); // keeps asking
    expect(g.accept(4, true, 3).decode).toBe(false); // still backed up
    expect(g.accept(5, true, 0).decode).toBe(true);
  });
  it('(a) an IDR dropped for a backed-up queue does not end the asking', () => {
    const g = new KeyGate();
    g.accept(0, true, 0);
    expect(g.accept(1, false, 3).askKey).toBe(true); // starts waiting
    expect(g.accept(2, true, 3).decode).toBe(false); // the IDR it asked for, dropped
    // Sunshine sends no periodic IDR: every frame dropped while waiting must ask again
    // (askKey() throttles to one request per 100 ms).
    expect(g.accept(3, false, 0)).toEqual({ decode: false, askKey: true });
    expect(g.accept(4, false, 0)).toEqual({ decode: false, askKey: true });
  });
  it('(b) an ask swallowed by the 100 ms throttle is repeated by the next dropped frame', () => {
    const g = new KeyGate();
    g.accept(0, true, 0);
    g.reset(); // decoder error: its ask may land inside the throttle window
    expect(g.accept(1, false, 0)).toEqual({ decode: false, askKey: true });
    expect(g.accept(3, false, 0)).toEqual({ decode: false, askKey: true });
    expect(g.accept(4, true, 0)).toEqual({ decode: true, askKey: false });
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

describe('startVideo', () => {
  it('closes the decoder when the transport closes', async () => {
    const decoder = { state: 'configured', decodeQueueSize: 0, configure() {}, decode() {}, close: vi.fn(() => { decoder.state = 'closed'; }) };
    vi.stubGlobal('VideoDecoder', Object.assign(function () { return decoder; }, { isConfigSupported: async () => ({ supported: true }) }));
    vi.stubGlobal('requestAnimationFrame', () => 0);
    let closeTransport!: () => void;
    const closed = new Promise<void>((r) => (closeTransport = r));
    const frame = new Uint8Array([...new Uint8Array(HEADER_LEN), 0, 0, 0, 1, ...SUNSHINE_SPS]);
    frame[4] = 1;
    const incoming = new ReadableStream({
      start(c) {
        c.enqueue(new ReadableStream({ start(s) { s.enqueue(frame); s.close(); } }));
      },
    });
    const transport = {
      closed,
      incomingUnidirectionalStreams: incoming,
      createUnidirectionalStream: async () => ({ getWriter: () => ({ write: async () => {}, close: async () => {} }) }),
    } as unknown as WebTransport;
    const canvas = { getContext: () => ({}) } as unknown as HTMLCanvasElement;
    startVideo(transport, canvas);
    await new Promise((r) => setTimeout(r, 20));
    closeTransport();
    await vi.waitFor(() => expect(decoder.close).toHaveBeenCalledTimes(1));
    vi.unstubAllGlobals();
  });
});
