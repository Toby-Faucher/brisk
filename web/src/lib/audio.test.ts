import { describe, expect, it } from 'vitest';
import {
  CAPACITY_FRAMES, DEADBAND_FRAMES, FADE_FRAMES, MAX_DECODER_RESTARTS, PACKET_FRAMES, PAUSE_FRAMES, Playout,
  Restarts, TARGET_FRAMES, UNDERRUN_LOG, concealment, driftStep, parseAudioHeader, seqDelta,
} from './audio';

const QUANTUM = 128;

/// One 5 ms stereo packet: left = `v`, right = -`v`.
const pkt = (v: number, n = PACKET_FRAMES): [Float32Array, Float32Array] => [
  new Float32Array(n).fill(v),
  new Float32Array(n).fill(-v),
];

const pull = (p: Playout, n = QUANTUM) => {
  const l = new Float32Array(n);
  const r = new Float32Array(n);
  p.pull(l, r);
  return [l, r];
};

/// Pushes packets seq0.. until playback has started and settled.
function playing(seq0 = 0): { p: Playout; seq: number } {
  const p = new Playout();
  let seq = seq0;
  for (let i = 0; i < TARGET_FRAMES / PACKET_FRAMES; i++) p.push(seq++ & 0xffff, ...pkt(0.5));
  pull(p);
  return { p, seq };
}

describe('parseAudioHeader', () => {
  it('reads seq u16 and ts_48k u32 little-endian, then the Opus bytes', () => {
    const b = Uint8Array.from([0x02, 0x01, 0x30, 0x00, 0x00, 0x01, 9, 8, 7]);
    const h = parseAudioHeader(b);
    expect(h.seq).toBe(0x0102);
    expect(h.ts48).toBe(0x0100_0030);
    expect([...h.opus]).toEqual([9, 8, 7]);
  });
  it('rejects a datagram without payload', () => {
    expect(() => parseAudioHeader(new Uint8Array(6))).toThrow();
  });
});

describe('seqDelta', () => {
  it('is the signed u16 distance', () => {
    expect(seqDelta(5, 5)).toBe(0);
    expect(seqDelta(7, 5)).toBe(2);
    expect(seqDelta(4, 5)).toBe(-1);
    expect(seqDelta(1, 0xffff)).toBe(2);
    expect(seqDelta(0xfffe, 0)).toBe(-2);
  });
});

describe('Playout', () => {
  it('waits for the 20 ms target before playing, and counts nothing at start', () => {
    const p = new Playout();
    p.push(0, ...pkt(0.5));
    const [l] = pull(p);
    expect(l.every((x) => x === 0)).toBe(true);
    for (let s = 1; s < 4; s++) p.push(s, ...pkt(0.5));
    const [l2, r2] = pull(p);
    expect(l2[0]).toBe(0.5);
    expect(r2[0]).toBe(-0.5);
    expect(p.stats).toMatchObject({ underruns: 0, lost: 0, late: 0, pauses: 0, overflows: 0 });
  });

  it('counts a gap as lost and fills it with a faded repeat of the last packet', () => {
    const { p, seq } = playing();
    const before = p.fill;
    p.push(seq + 2, ...pkt(0.25)); // seq+0 and seq+1 lost
    expect(p.stats.lost).toBe(2);
    expect(p.stats.underruns).toBe(0);
    // Two packets of concealment plus the packet itself keep the playout time.
    expect(p.fill).toBe(before + 3 * PACKET_FRAMES);
  });

  it('a late or duplicate packet is counted and dropped', () => {
    const { p, seq } = playing();
    const before = p.fill;
    p.push(seq - 1, ...pkt(0.5));
    p.push(seq - 3, ...pkt(0.5));
    expect(p.stats.late).toBe(2);
    expect(p.fill).toBe(before);
  });

  it('wraps seq at 2^16', () => {
    const { p, seq } = playing(0xfffe);
    expect(seq & 0xffff).toBe(2);
    p.push(2, ...pkt(0.5));
    expect(p.stats).toMatchObject({ lost: 0, late: 0 });
  });

  it('starvation with no loss (a silent host stops sending) counts nothing', () => {
    let { p, seq } = playing();
    // The host goes quiet for a second: the ring runs dry and stays dry.
    for (let i = 0; i < 48000 / QUANTUM; i++) pull(p);
    expect(p.fill).toBe(0);
    p.push(seq++, ...pkt(0.5)); // it speaks again, no seq gap
    expect(p.stats).toMatchObject({ underruns: 0, lost: 0, pauses: 1 });
    // It restarts at the 20 ms target, fading in from silence.
    for (let i = 0; i < 3; i++) p.push(seq++, ...pkt(0.5));
    const [l] = pull(p);
    expect(l[0]).toBe(0);
    expect(l[FADE_FRAMES / 2]).toBeCloseTo(0.25, 6);
    expect(l[FADE_FRAMES]).toBe(0.5);
  });

  it('a short starvation with no loss is an underrun (late network, not a silent host)', () => {
    let { p, seq } = playing();
    const quanta = Math.ceil((p.fill + PACKET_FRAMES) / QUANTUM); // dry for about 5 ms
    for (let i = 0; i < quanta; i++) pull(p);
    expect(p.fill).toBe(0);
    p.push(seq++, ...pkt(0.5));
    expect(p.stats).toMatchObject({ underruns: 1, lost: 0, pauses: 0 });
    // Dry from the quantum that ran out until this packet: 2 quanta here.
    expect(p.stats.underrunMs).toEqual([Math.round((quanta * QUANTUM - 832) / 48)]);
  });

  it('keeps the durations of the last UNDERRUN_LOG underruns only', () => {
    let { p, seq } = playing();
    for (let k = 0; k < UNDERRUN_LOG + 5; k++) {
      for (let i = 0; i < 8 + k; i++) pull(p);
      for (let i = 0; i < 4; i++) p.push(seq++ & 0xffff, ...pkt(0.5));
    }
    expect(p.stats.underruns).toBe(UNDERRUN_LOG + 5);
    expect(p.stats.underrunMs.length).toBe(UNDERRUN_LOG);
    expect(p.stats.underrunMs[UNDERRUN_LOG - 1]).toBeGreaterThan(p.stats.underrunMs[0]); // newest last
  });

  it('a long starvation that ends with a seq gap is loss, so an underrun', () => {
    let { p, seq } = playing();
    for (let i = 0; i < (2 * PAUSE_FRAMES) / QUANTUM; i++) pull(p);
    p.push(seq + 5, ...pkt(0.5));
    expect(p.stats).toMatchObject({ underruns: 1, lost: 5, pauses: 0 });
    // No stale concealment after the silence: only the new packet is buffered.
    expect(p.fill).toBe(PACKET_FRAMES);
  });

  it('the underrun is counted once per starvation', () => {
    let { p, seq } = playing();
    for (let i = 0; i < 20; i++) pull(p);
    p.push(seq++, ...pkt(0.5));
    for (let i = 0; i < 3; i++) p.push(seq++, ...pkt(0.5));
    for (let i = 0; i < 20; i++) pull(p);
    expect(p.stats.underruns).toBe(1);
  });

  it('never holds more than 40 ms: a burst drops the oldest audio and counts it', () => {
    const p = new Playout();
    for (let s = 0; s < 20; s++) p.push(s, ...pkt(s / 100));
    expect(p.fill).toBe(CAPACITY_FRAMES);
    expect(p.stats.overflows).toBe(20 - CAPACITY_FRAMES / PACKET_FRAMES);
    const [l] = pull(p);
    expect(l[0]).toBeCloseTo((20 - CAPACITY_FRAMES / PACKET_FRAMES) / 100, 6); // the oldest kept
    expect(p.stats.bufferMs).toBeCloseTo((CAPACITY_FRAMES - QUANTUM) / 48, 6);
  });

  it('fades in after concealment, so the resumed audio does not click', () => {
    const { p, seq } = playing();
    p.push(seq + 1, ...pkt(0.8)); // one lost
    // Drain the 4 good packets and the concealed one to reach the resumed packet.
    const out: number[] = [];
    while (out.length < 6 * PACKET_FRAMES) out.push(...pull(p)[0]);
    const resumed = out.slice(5 * PACKET_FRAMES - QUANTUM);
    expect(resumed[0]).toBeCloseTo(0, 6);
    expect(resumed[FADE_FRAMES / 2]).toBeCloseTo(0.4, 2);
    expect(resumed[FADE_FRAMES]).toBeCloseTo(0.8, 6);
  });
});

describe('concealment', () => {
  it('repeats the last packet fading from full to zero, then silence', () => {
    const [l, r] = pkt(1);
    const [cl, cr] = concealment(l, r, 3);
    expect(cl.length).toBe(3 * PACKET_FRAMES);
    expect(cl[0]).toBeCloseTo(1, 6);
    expect(cl[PACKET_FRAMES / 2]).toBeCloseTo(0.5, 2);
    expect(cr[PACKET_FRAMES / 2]).toBeCloseTo(-0.5, 2);
    expect(Math.abs(cl[PACKET_FRAMES - 1])).toBeLessThan(0.01);
    expect(cl.subarray(PACKET_FRAMES).every((x) => x === 0)).toBe(true);
  });
  it('fills at most 40 ms however long the gap', () => {
    const [l, r] = pkt(1);
    expect(concealment(l, r, 1000)[0].length).toBe(CAPACITY_FRAMES);
  });
});

describe('drift', () => {
  it('driftStep: drop a frame when fuller than target + 5 ms, repeat one when emptier, else nothing', () => {
    expect(driftStep(TARGET_FRAMES, TARGET_FRAMES)).toBe(0);
    expect(driftStep(TARGET_FRAMES + DEADBAND_FRAMES, TARGET_FRAMES)).toBe(0);
    expect(driftStep(TARGET_FRAMES + DEADBAND_FRAMES + 1, TARGET_FRAMES)).toBe(1);
    expect(driftStep(TARGET_FRAMES - DEADBAND_FRAMES - 1, TARGET_FRAMES)).toBe(-1);
  });

  // Host clock 0.1 % fast: 48048 frames a second arrive for 48000 played.
  const run = (hostRatio: number, secs: number) => {
    const p = new Playout();
    let seq = 0;
    let owed = 0;
    for (let q = 0; q < (secs * 48000) / QUANTUM; q++) {
      owed += QUANTUM * hostRatio;
      while (owed >= PACKET_FRAMES) {
        p.push(seq++ & 0xffff, ...pkt(0.5));
        owed -= PACKET_FRAMES;
      }
      pull(p);
    }
    return p;
  };

  it('a fast host clock is absorbed without overflow, the buffer stays near the target', () => {
    const p = run(1.001, 60);
    expect(p.stats.overflows).toBe(0);
    expect(p.stats.dropped).toBeGreaterThan(0.8 * 48 * 60);
    expect(Math.abs(p.avgFill - TARGET_FRAMES)).toBeLessThanOrEqual(DEADBAND_FRAMES + PACKET_FRAMES);
  });

  it('a slow host clock is absorbed without underruns', () => {
    const p = run(0.999, 60);
    expect(p.stats.underruns).toBe(0);
    expect(p.stats.repeated).toBeGreaterThan(0.8 * 48 * 60);
  });

  it('matched clocks never adjust', () => {
    const p = run(1, 60);
    expect(p.stats).toMatchObject({ dropped: 0, repeated: 0, underruns: 0, overflows: 0 });
  });
});

describe('Restarts', () => {
  it('allows MAX_DECODER_RESTARTS failures in a row, then stops', () => {
    const r = new Restarts();
    for (let i = 0; i < MAX_DECODER_RESTARTS; i++) expect(r.failed()).toBe(true);
    expect(r.failed()).toBe(false);
  });
  it('a decoded packet resets the count', () => {
    const r = new Restarts(2);
    expect(r.failed()).toBe(true);
    expect(r.failed()).toBe(true);
    r.ok();
    expect(r.failed()).toBe(true);
    expect(r.failed()).toBe(true);
    expect(r.failed()).toBe(false);
  });
});
