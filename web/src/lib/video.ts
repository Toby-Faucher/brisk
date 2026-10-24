// Brisk video: brisk-bridge frame streams -> WebCodecs -> canvas. Framework-free.
//
// Each frame arrives on its own WebTransport uni stream: a 21-byte little-endian header
// {frame_idx u32, flags u8, capture_ns u64, bridge_ns u64}, then Annex B H.264, then FIN.
// capture_ns and bridge_ns are both CLOCK_MONOTONIC on the bridge host, so their difference is
// meaningful here; neither is compared with this machine's clock.

export const HEADER_LEN = 21;
export const FLAG_IDR = 1;
/// Bridge message: "my decoder needs a key frame".
export const MSG_WANT_KEY = 1;
/// Largest frame accepted (the tap's own cap is 4 MiB).
const MAX_FRAME = HEADER_LEN + (4 << 20);
const US_PER_FRAME = 1e6 / 60;
const KEY_ASK_EVERY_MS = 100;

export interface FrameHeader {
  idx: number;
  idr: boolean;
  captureNs: bigint;
  bridgeNs: bigint;
}

export function parseHeader(b: Uint8Array): FrameHeader {
  if (b.length < HEADER_LEN) throw new Error(`frame of ${b.length} bytes is shorter than its header`);
  const v = new DataView(b.buffer, b.byteOffset, HEADER_LEN);
  return {
    idx: v.getUint32(0, true),
    idr: (v.getUint8(4) & FLAG_IDR) !== 0,
    captureNs: v.getBigUint64(5, true),
    bridgeNs: v.getBigUint64(13, true),
  };
}

/// The first SPS NAL unit (header byte included) of an Annex B access unit, or null.
export function findSps(annexb: Uint8Array): Uint8Array | null {
  const starts: number[] = [];
  for (let i = 0; i + 2 < annexb.length; i++) {
    if (annexb[i] === 0 && annexb[i + 1] === 0 && annexb[i + 2] === 1) {
      starts.push(i + 3);
      i += 2;
    }
  }
  for (let k = 0; k < starts.length; k++) {
    const s = starts[k];
    if ((annexb[s] & 0x1f) !== 7) continue;
    let end = k + 1 < starts.length ? starts[k + 1] - 3 : annexb.length;
    if (end > s && annexb[end - 1] === 0) end--; // 4-byte start code of the next NAL
    return annexb.subarray(s, end);
  }
  return null;
}

/// RFC 6381 `avc1.PPCCLL` from an SPS: profile_idc, the constraint-flags byte, level_idc.
export function codecFromSps(sps: Uint8Array): string {
  if (sps.length < 4 || (sps[0] & 0x1f) !== 7 || sps[1] === 0) {
    throw new Error('not an H.264 SPS');
  }
  const hex = (x: number) => x.toString(16).padStart(2, '0');
  return `avc1.${hex(sps[1])}${hex(sps[2])}${hex(sps[3])}`;
}

/// Which frames may go to the decoder. It starts waiting for a key frame (WebCodecs needs one
/// after configure), takes deltas only in index order with no gap, and goes back to waiting
/// for a key frame on a gap, a reset, or a decode queue longer than 2.
export class KeyGate {
  needKey = true;
  private last = -1;

  /// `askKey`: the gate just started waiting, so the bridge should be asked for an IDR.
  accept(idx: number, idr: boolean, decodeQueueSize: number): { decode: boolean; askKey: boolean } {
    let askKey = false;
    if (decodeQueueSize > 2 && !this.needKey) {
      this.needKey = true;
      askKey = true;
    }
    if (idr) {
      if (this.last >= 0 && !after(idx, this.last)) return { decode: false, askKey }; // stale
      if (decodeQueueSize > 2) return { decode: false, askKey };
      this.needKey = false;
      this.last = idx;
      return { decode: true, askKey: false };
    }
    if (this.needKey) return { decode: false, askKey };
    if (((idx - this.last) >>> 0) !== 1) {
      this.needKey = true; // gap or out of order: the reference chain is broken
      return { decode: false, askKey: true };
    }
    this.last = idx;
    return { decode: true, askKey: false };
  }

  reset(): void {
    this.needKey = true;
  }
}

/// `a` is newer than `b` in u32 index space (wraps after 2^32 frames).
function after(a: number, b: number): boolean {
  const d = (a - b) >>> 0;
  return d !== 0 && d < 0x8000_0000;
}

/// The last `n` samples, for percentiles (bounded memory).
export class Window {
  private v: number[] = [];
  constructor(private n = 600) {}
  push(x: number): void {
    this.v.push(x);
    if (this.v.length > this.n) this.v.shift();
  }
  pct(p: number): number | null {
    if (this.v.length === 0) return null;
    const s = [...this.v].sort((a, b) => a - b);
    return s[Math.max(0, Math.ceil((p / 100) * s.length) - 1)];
  }
  min(): number | null {
    return this.v.length ? Math.min(...this.v) : null;
  }
}

export interface VideoStats {
  codec: string | null;
  /// isConfigSupported result, with the hardwareAcceleration that was accepted.
  supported: boolean | null;
  hardwareAcceleration: string | null;
  width: number;
  height: number;
  framesReceived: number;
  framesDecoded: number;
  framesPresented: number;
  /// Received but not decoded (waiting for a key frame, gaps, stale).
  framesDropped: number;
  /// Decoded but replaced by a newer frame before a paint.
  framesSkipped: number;
  /// Bridge reset the stream before it was complete.
  streamsReset: number;
  decodeErrors: number;
  keyRequests: number;
  decodeQueueSize: number;
  lastError: string | null;
  /// Windows of the last 600 frames, ms.
  bridgeCaptureMs: Window;
  decodeMs: Window;
  presentMs: Window;
  /// Arrival interval minus bridge-write interval (clock-sync free).
  jitterMs: Window;
  /// (arrival - bridge_ns) minus its minimum over the window: delay above the best frame.
  queueMs: Window;
}

export interface Video {
  stats: VideoStats;
  /// Test hook: stop reading new streams for `ms` (simulates a stalled tab).
  stall(ms: number): void;
  done: Promise<void>;
}

/// Plays the bridge's video from `transport` onto `canvas`.
export function startVideo(transport: WebTransport, canvas: HTMLCanvasElement): Video {
  const stats: VideoStats = {
    codec: null, supported: null, hardwareAcceleration: null, width: 0, height: 0,
    framesReceived: 0, framesDecoded: 0, framesPresented: 0, framesDropped: 0, framesSkipped: 0,
    streamsReset: 0, decodeErrors: 0, keyRequests: 0, decodeQueueSize: 0, lastError: null,
    bridgeCaptureMs: new Window(), decodeMs: new Window(), presentMs: new Window(),
    jitterMs: new Window(), queueMs: new Window(),
  };
  const ctx = canvas.getContext('2d')!;
  const gate = new KeyGate();
  const decodeStart = new Map<number, number>();
  let config: VideoDecoderConfig | null = null;
  let decoder: VideoDecoder | null = null;
  let configuring = false;
  let pending: { frame: VideoFrame; at: number } | null = null;
  let prev: { arrival: number; bridgeMs: number } | null = null;
  let lastAsk = -Infinity;
  let stallUntil = 0;

  const askKey = () => {
    const now = performance.now();
    if (now - lastAsk < KEY_ASK_EVERY_MS) return;
    lastAsk = now;
    stats.keyRequests++;
    transport.createUnidirectionalStream().then(async (s) => {
      const w = s.getWriter();
      await w.write(new Uint8Array([MSG_WANT_KEY]));
      await w.close();
    }).catch((e) => {
      stats.lastError = `key request: ${e}`;
    });
  };

  const newDecoder = () => {
    decoder = new VideoDecoder({
      output: (frame) => {
        const now = performance.now();
        const t = decodeStart.get(frame.timestamp);
        if (t !== undefined) {
          stats.decodeMs.push(now - t);
          decodeStart.delete(frame.timestamp);
        }
        stats.framesDecoded++;
        if (pending) {
          pending.frame.close();
          stats.framesSkipped++;
        }
        pending = { frame, at: now };
      },
      error: (e) => {
        stats.decodeErrors++;
        stats.lastError = String(e);
        decodeStart.clear();
        gate.reset();
        if (config) {
          newDecoder();
          decoder!.configure(config);
        }
        askKey();
      },
    });
  };

  const configure = async (codec: string) => {
    configuring = true;
    stats.codec = codec;
    for (const hardwareAcceleration of ['prefer-hardware', 'no-preference'] as const) {
      const c: VideoDecoderConfig = { codec, optimizeForLatency: true, hardwareAcceleration };
      try {
        const r = await VideoDecoder.isConfigSupported(c);
        if (r.supported) {
          stats.supported = true;
          stats.hardwareAcceleration = hardwareAcceleration;
          config = c;
          newDecoder();
          decoder!.configure(c);
          gate.reset(); // the IDR that carried the SPS went by while we waited
          askKey();
          configuring = false;
          return;
        }
      } catch (e) {
        stats.lastError = String(e);
      }
    }
    stats.supported = false;
    stats.lastError = `decoder does not support ${codec}`;
    // configuring stays true: nothing to decode with
  };

  const onFrame = (b: Uint8Array, arrival: number) => {
    const h = parseHeader(b);
    stats.framesReceived++;
    const bridgeMs = Number(h.bridgeNs) / 1e6;
    if (h.captureNs !== 0n) stats.bridgeCaptureMs.push(Number(h.bridgeNs - h.captureNs) / 1e6);
    const q = arrival - bridgeMs;
    stats.queueMs.push(q);
    if (prev) stats.jitterMs.push(Math.abs(arrival - prev.arrival - (bridgeMs - prev.bridgeMs)));
    prev = { arrival, bridgeMs };
    const annexb = b.subarray(HEADER_LEN);
    if (!config) {
      if (h.idr && !configuring) {
        const sps = findSps(annexb);
        if (sps) configure(codecFromSps(sps)).catch((e) => (stats.lastError = String(e)));
      }
      stats.framesDropped++;
      return;
    }
    stats.decodeQueueSize = decoder!.decodeQueueSize;
    const g = gate.accept(h.idx, h.idr, decoder!.decodeQueueSize);
    if (g.askKey) askKey();
    if (!g.decode) {
      stats.framesDropped++;
      return;
    }
    const timestamp = Math.round(h.idx * US_PER_FRAME);
    decodeStart.set(timestamp, performance.now());
    if (decodeStart.size > 64) decodeStart.delete(decodeStart.keys().next().value!);
    try {
      decoder!.decode(new EncodedVideoChunk({ type: h.idr ? 'key' : 'delta', timestamp, data: annexb }));
    } catch (e) {
      stats.decodeErrors++;
      stats.lastError = String(e);
      gate.reset();
      askKey();
    }
  };

  const paint = () => {
    if (pending) {
      const { frame, at } = pending;
      pending = null;
      if (canvas.width !== frame.displayWidth || canvas.height !== frame.displayHeight) {
        canvas.width = stats.width = frame.displayWidth;
        canvas.height = stats.height = frame.displayHeight;
      }
      ctx.drawImage(frame, 0, 0);
      frame.close();
      stats.framesPresented++;
      stats.presentMs.push(performance.now() - at);
    }
    requestAnimationFrame(paint);
  };
  requestAnimationFrame(paint);

  const readStream = async (s: ReadableStream<Uint8Array>) => {
    const parts: Uint8Array[] = [];
    let n = 0;
    try {
      for await (const c of s as unknown as AsyncIterable<Uint8Array>) {
        n += c.length;
        if (n > MAX_FRAME) throw new Error(`frame over ${MAX_FRAME} bytes`);
        parts.push(c);
      }
    } catch {
      stats.streamsReset++;
      return;
    }
    const arrival = performance.now();
    const b = parts.length === 1 ? parts[0] : new Uint8Array(n);
    if (parts.length !== 1) {
      let o = 0;
      for (const p of parts) {
        b.set(p, o);
        o += p.length;
      }
    }
    try {
      onFrame(b, arrival);
    } catch (e) {
      stats.lastError = String(e);
    }
  };

  const done = (async () => {
    const r = transport.incomingUnidirectionalStreams.getReader();
    for (;;) {
      const wait = stallUntil - performance.now();
      if (wait > 0) await new Promise((res) => setTimeout(res, wait));
      const { value, done } = await r.read();
      if (done) return;
      readStream(value);
    }
  })();

  return {
    stats,
    stall: (ms) => {
      stallUntil = performance.now() + ms;
    },
    done,
  };
}
