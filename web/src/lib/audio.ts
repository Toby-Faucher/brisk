// Brisk audio: brisk-bridge datagrams -> WebCodecs AudioDecoder -> AudioWorklet playout ring.
// Framework-free.
//
// Each WebTransport datagram is one 5 ms Opus packet exactly as Sunshine encoded it (stereo,
// CBR, RESTRICTED_LOWDELAY), after the bridge's FEC recovery and decryption: a 6-byte
// little-endian header {seq u16, ts_48k u32}, then the Opus bytes. Datagrams are never
// retransmitted, so a lost one shows up as a seq gap.
//
// The pure part (Playout, concealment, driftStep) also runs inside the worklet
// (audio-worklet.ts); startAudio is the main-thread glue.

export const RATE = 48000;
export const AUDIO_HEADER_LEN = 6;
/// 5 ms at 48 kHz (Sunshine's packetDuration 5).
export const PACKET_FRAMES = 240;
/// Playback starts (and restarts after a starvation) at 20 ms buffered.
export const TARGET_FRAMES = 960;
/// The ring never holds more than 40 ms; a burst beyond that drops the oldest audio.
export const CAPACITY_FRAMES = 1920;
/// Drift correction acts only when the average fill is more than 5 ms off the target.
export const DEADBAND_FRAMES = 240;
/// A starvation this long that ends with no seq gap is a silent host (Sunshine stops sending
/// while the host is quiet unless continuous_audio is set), not a glitch.
export const PAUSE_FRAMES = 9600;
/// Fade-in after concealment or starvation, so resumed audio does not click.
export const FADE_FRAMES = 96;
/// Underrun durations kept in the stats.
export const UNDERRUN_LOG = 20;
/// Decoder errors in a row (no packet decoded in between) before audio gives up.
export const MAX_DECODER_RESTARTS = 5;
/// Weight of each render quantum in the fill average (about a 1.4 s time constant).
const AVG_ALPHA = 1 / 512;

export interface AudioHeader {
  seq: number;
  ts48: number;
  opus: Uint8Array;
}

export function parseAudioHeader(b: Uint8Array): AudioHeader {
  if (b.length <= AUDIO_HEADER_LEN) throw new Error(`audio datagram of ${b.length} bytes`);
  const v = new DataView(b.buffer, b.byteOffset, AUDIO_HEADER_LEN);
  return { seq: v.getUint16(0, true), ts48: v.getUint32(2, true), opus: b.subarray(AUDIO_HEADER_LEN) };
}

/// `seq - expected` as a signed distance in u16 space.
export function seqDelta(seq: number, expected: number): number {
  return (((seq - expected) & 0xffff) << 16) >> 16;
}

/// What fills a gap of `packets` lost packets: the last packet once, fading from full to zero,
/// then silence; at most 40 ms.
export function concealment(l: Float32Array, r: Float32Array, packets: number): [Float32Array, Float32Array] {
  const n = Math.min(packets * PACKET_FRAMES, CAPACITY_FRAMES);
  const cl = new Float32Array(n);
  const cr = new Float32Array(n);
  const m = Math.min(l.length, n);
  for (let i = 0; i < m; i++) {
    const g = 1 - i / m;
    cl[i] = l[i] * g;
    cr[i] = r[i] * g;
  }
  return [cl, cr];
}

/// Frames to consume beyond one quantum: 1 = drop one (the host clock is fast), -1 = repeat one
/// (it is slow), 0 = within the deadband.
// shortcut: at most one frame per 128-frame quantum (0.8 %), with no interpolation; enough for
// crystal drift (~100 ppm), audible only if the clocks were wildly off. Resample if that shows.
export function driftStep(avgFill: number, target: number, deadband = DEADBAND_FRAMES): -1 | 0 | 1 {
  if (avgFill > target + deadband) return 1;
  if (avgFill < target - deadband) return -1;
  return 0;
}

export interface PlayoutStats {
  /// Starvations that were glitches: short ones, or ones that ended with a seq gap.
  underruns: number;
  /// How long each of the last UNDERRUN_LOG underruns lasted, ms: ring empty until the next
  /// packet (playback then waits for the 20 ms target on top of that).
  underrunMs: number[];
  /// Packets missing from the seq sequence (concealed when playing).
  lost: number;
  /// Packets older than one already played (reordered or duplicated): dropped.
  late: number;
  /// Starvations of at least PAUSE_FRAMES with no gap: the host went quiet. Not glitches.
  pauses: number;
  /// Packets that pushed the ring past 40 ms (the oldest audio was dropped).
  overflows: number;
  /// Frames dropped / repeated for clock drift.
  dropped: number;
  repeated: number;
  bufferMs: number;
}

/// The playout ring (planar stereo) and its loss, starvation and drift rules. Sans-io: `push`
/// one decoded packet, `pull` one render quantum.
export class Playout {
  stats: PlayoutStats = {
    underruns: 0,
    underrunMs: [],
    lost: 0,
    late: 0,
    pauses: 0,
    overflows: 0,
    dropped: 0,
    repeated: 0,
    bufferMs: 0,
  };
  avgFill = TARGET_FRAMES;
  private l = new Float32Array(CAPACITY_FRAMES);
  private r = new Float32Array(CAPACITY_FRAMES);
  private rd = 0;
  private n = 0;
  private expected: number | null = null;
  private playing = false;
  /// Frame clock when the current starvation began (null: not starved).
  private starvedAt: number | null = null;
  private clock = 0;
  private last: [Float32Array, Float32Array] | null = null;
  private fadeLeft = 0;

  get fill(): number {
    return this.n;
  }

  push(seq: number, l: Float32Array, r: Float32Array): void {
    const gap = this.expected === null ? 0 : seqDelta(seq, this.expected);
    if (gap < 0) {
      this.stats.late++;
      return;
    }
    this.stats.lost += gap;
    if (this.starvedAt !== null) {
      const dry = this.clock - this.starvedAt;
      if (gap > 0 || dry < PAUSE_FRAMES) {
        this.stats.underruns++;
        this.stats.underrunMs.push(Math.round(dry / (RATE / 1000)));
        if (this.stats.underrunMs.length > UNDERRUN_LOG) this.stats.underrunMs.shift();
      } else this.stats.pauses++;
      this.starvedAt = null;
      this.fadeLeft = FADE_FRAMES;
    } else if (gap > 0 && this.last) {
      this.write(...concealment(this.last[0], this.last[1], gap));
      this.fadeLeft = FADE_FRAMES;
    }
    this.expected = (seq + 1) & 0xffff;
    this.last = [l, r];
    this.write(l, r);
  }

  pull(outL: Float32Array, outR: Float32Array): void {
    const q = outL.length;
    this.clock += q;
    if (!this.playing) {
      if (this.n < TARGET_FRAMES) {
        outL.fill(0);
        outR.fill(0);
        this.update();
        return;
      }
      this.playing = true;
    }
    const step = driftStep(this.avgFill, TARGET_FRAMES);
    if (step === 1 && this.n > q) {
      this.skip(1);
      this.stats.dropped++;
    }
    const take = Math.min(step === -1 ? q - 1 : q, this.n);
    for (let i = 0; i < take; i++) {
      const j = (this.rd + i) % CAPACITY_FRAMES;
      outL[i] = this.l[j];
      outR[i] = this.r[j];
    }
    this.skip(take);
    if (take === q - 1 && step === -1) {
      outL[q - 1] = outL[q - 2];
      outR[q - 1] = outR[q - 2];
      this.stats.repeated++;
    } else if (take < q) {
      outL.fill(0, take);
      outR.fill(0, take);
      this.playing = false;
      this.starvedAt = this.clock - q + take;
    }
    this.avgFill += (this.n - this.avgFill) * AVG_ALPHA;
    this.update();
  }

  private write(l: Float32Array, r: Float32Array): void {
    const k = l.length;
    const over = this.n + k - CAPACITY_FRAMES;
    if (over > 0) {
      this.skip(Math.min(over, this.n));
      this.stats.overflows++;
    }
    const from = Math.max(0, k - CAPACITY_FRAMES);
    for (let i = from; i < k; i++) {
      const g = this.fadeLeft > 0 ? (FADE_FRAMES - this.fadeLeft--) / FADE_FRAMES : 1;
      const j = (this.rd + this.n) % CAPACITY_FRAMES;
      this.l[j] = l[i] * g;
      this.r[j] = r[i] * g;
      this.n++;
    }
    this.update();
  }

  private skip(k: number): void {
    this.rd = (this.rd + k) % CAPACITY_FRAMES;
    this.n -= k;
  }

  private update(): void {
    this.stats.bufferMs = this.n / (RATE / 1000);
  }
}

/// Counts decoder failures with no successful decode in between: a decoder that keeps failing
/// (a bad config, an unsupported codec) is not re-created forever.
export class Restarts {
  private row = 0;
  constructor(private max = MAX_DECODER_RESTARTS) {}
  /// A failure; true while another restart is allowed.
  failed(): boolean {
    return ++this.row <= this.max;
  }
  ok(): void {
    this.row = 0;
  }
}

export interface AudioStats extends PlayoutStats {
  /// Datagrams received / Opus packets decoded.
  packets: number;
  decoded: number;
  /// Decoded packets with any sample above -60 dBFS (proves real audio, not digital silence).
  nonSilent: number;
  decodeErrors: number;
  contextState: string;
  /// AudioContext baseLatency + outputLatency, ms (what the device adds after the ring).
  outputMs: number;
  lastError: string | null;
}

export interface Audio {
  stats: AudioStats;
  done: Promise<void>;
}

const DECODER_CONFIG = { codec: 'opus', sampleRate: RATE, numberOfChannels: 2 };

/// Plays the bridge's audio datagrams from `transport` through `ctx`. Create `ctx` in a user
/// gesture (the connect click), or Chrome's autoplay policy keeps it suspended.
/// `workletUrl`: the bundled audio-worklet.ts. `out`: where the playout connects (a volume GainNode).
/// `onPacket` (replay buffer) gets each parsed packet, by reference.
export async function startAudio(
  transport: WebTransport,
  ctx: AudioContext,
  workletUrl: string,
  out: AudioNode = ctx.destination,
  onPacket?: (h: AudioHeader) => void,
): Promise<Audio> {
  const stats: AudioStats = {
    underruns: 0,
    underrunMs: [],
    lost: 0,
    late: 0,
    pauses: 0,
    overflows: 0,
    dropped: 0,
    repeated: 0,
    bufferMs: 0,
    packets: 0,
    decoded: 0,
    nonSilent: 0,
    decodeErrors: 0,
    contextState: ctx.state,
    outputMs: 0,
    lastError: null,
  };
  await ctx.audioWorklet.addModule(workletUrl);
  const node = new AudioWorkletNode(ctx, 'brisk-playout', { numberOfInputs: 0, outputChannelCount: [2] });
  node.connect(out);
  node.port.onmessage = (e: MessageEvent<PlayoutStats>) => {
    Object.assign(stats, e.data);
    stats.contextState = ctx.state;
    stats.outputMs = ((ctx.baseLatency ?? 0) + (ctx.outputLatency ?? 0)) * 1000;
  };
  // AudioData timestamps are rewritten by Chrome's decoder (contiguous from the first one), so
  // the seq of each output comes from this FIFO: Opus gives one output per chunk, in order.
  let seqs: number[] = [];
  let decoder = null as AudioDecoder | null; // `as`: assigned in closures, so TS must not narrow it to null
  const restarts = new Restarts();
  const fail = (e: unknown) => {
    stats.decodeErrors++;
    stats.lastError = String(e);
    seqs = [];
    if (restarts.failed()) return newDecoder();
    decoder = null;
    stats.lastError = `audio stopped: decoder failed ${MAX_DECODER_RESTARTS + 1} times in a row (${e})`;
  };
  const newDecoder = () => {
    try {
      decoder = new AudioDecoder({
        output: (a) => {
          const seq = seqs.shift();
          const l = new Float32Array(a.numberOfFrames);
          const r = new Float32Array(a.numberOfFrames);
          a.copyTo(l, { planeIndex: 0, format: 'f32-planar' });
          a.copyTo(r, { planeIndex: a.numberOfChannels > 1 ? 1 : 0, format: 'f32-planar' });
          a.close();
          stats.decoded++;
          restarts.ok();
          if (l.some((x) => Math.abs(x) > 1e-3)) stats.nonSilent++;
          if (seq !== undefined) node.port.postMessage({ seq, l, r }, [l.buffer, r.buffer]);
        },
        error: fail,
      });
      decoder.configure(DECODER_CONFIG);
    } catch (e) {
      fail(e); // a config the browser rejects outright
    }
  };
  newDecoder();
  let timestamp = 0;
  const done = (async () => {
    const reader = transport.datagrams.readable.getReader();
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      stats.packets++;
      if (!decoder) continue; // gave up (stats.lastError says why)
      let h: AudioHeader;
      try {
        h = parseAudioHeader(value);
      } catch (e) {
        stats.lastError = String(e);
        continue;
      }
      onPacket?.(h);
      seqs.push(h.seq);
      try {
        // Timestamps only need to increase; 5 ms per packet.
        decoder.decode(new EncodedAudioChunk({ type: 'key', timestamp, data: h.opus }));
        timestamp += 5000;
      } catch (e) {
        seqs.pop();
        stats.decodeErrors++;
        stats.lastError = String(e);
      }
    }
  })();
  return { stats, done };
}
