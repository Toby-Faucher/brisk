// Brisk video: brisk-bridge frame streams -> WebCodecs -> canvas. Framework-free.
//
// Each frame arrives on its own WebTransport uni stream: a 21-byte little-endian header
// {frame_idx u32, flags u8, capture_ns u64, bridge_ns u64}, then Annex B H.264 or HEVC, then FIN.
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

/// The NAL units (header included, start code not) of an Annex B access unit.
function nals(annexb: Uint8Array): Uint8Array[] {
  const starts: number[] = [];
  for (let i = 0; i + 2 < annexb.length; i++) {
    if (annexb[i] === 0 && annexb[i + 1] === 0 && annexb[i + 2] === 1) {
      starts.push(i + 3);
      i += 2;
    }
  }
  return starts.map((s, k) => {
    let end = k + 1 < starts.length ? starts[k + 1] - 3 : annexb.length;
    if (end > s && annexb[end - 1] === 0) end--; // 4-byte start code of the next NAL
    return annexb.subarray(s, end);
  });
}

/// The first SPS NAL unit (header byte included) of an Annex B access unit, or null.
export function findSps(annexb: Uint8Array): Uint8Array | null {
  return nals(annexb).find((n) => (n[0] & 0x1f) === 7) ?? null;
}

/// RFC 6381 `avc1.PPCCLL` from an SPS: profile_idc, the constraint-flags byte, level_idc.
export function codecFromSps(sps: Uint8Array): string {
  if (sps.length < 4 || (sps[0] & 0x1f) !== 7 || sps[1] === 0) {
    throw new Error('not an H.264 SPS');
  }
  const hex = (x: number) => x.toString(16).padStart(2, '0');
  return `avc1.${hex(sps[1])}${hex(sps[2])}${hex(sps[3])}`;
}

const hevcType = (b: number) => (b >> 1) & 0x3f;

/// ISO 14496-15 E.3 `hvc1.<space><profile>.<compat reversed>.<L|H><level>.<constraint bytes>`
/// from an HEVC SPS (NAL type 33). The general profile_tier_level fields come first and are byte
/// aligned whatever sps_max_sub_layers is (sub-layer fields follow general_level_idc).
export function codecFromHevcSps(sps: Uint8Array): string {
  if (sps.length < 3 || hevcType(sps[0]) !== 33) throw new Error('not an HEVC SPS');
  // RBSP after the 2-byte NAL header, emulation-prevention bytes (00 00 03) removed.
  const r: number[] = [];
  for (let i = 2, zeros = 0; i < sps.length && r.length < 13; i++) {
    if (zeros >= 2 && sps[i] === 3) {
      zeros = 0;
      continue;
    }
    r.push(sps[i]);
    zeros = sps[i] === 0 ? zeros + 1 : 0;
  }
  if (r.length < 13) throw new Error('HEVC SPS too short');
  // r[0]: vps id, max_sub_layers_minus1, temporal nesting; r[1]: space(2) tier(1) profile_idc(5).
  const space = ['', 'A', 'B', 'C'][r[1] >> 6];
  const tier = r[1] & 0x20 ? 'H' : 'L';
  const compat = ((r[2] << 24) | (r[3] << 16) | (r[4] << 8) | r[5]) >>> 0;
  let rev = 0;
  for (let j = 0; j < 32; j++) if (compat & (1 << j)) rev |= 1 << (31 - j);
  const cons = r.slice(6, 12);
  while (cons.length && cons[cons.length - 1] === 0) cons.pop();
  const parts = [`hvc1`, `${space}${r[1] & 0x1f}`, (rev >>> 0).toString(16).toUpperCase(), `${tier}${r[12]}`];
  return [...parts, ...cons.map((b) => b.toString(16).toUpperCase())].join('.');
}

/// The decoder codec string of a key frame's access unit: an H.264 SPS (type 7) gives avc1, as
/// on main; else an HEVC SPS (type 33) gives hvc1. HEVC headers with nuh_layer_id 0 are even bytes,
/// so they never read as H.264 type 7. Null without either.
export function codecFromAccessUnit(annexb: Uint8Array): string | null {
  const avc = findSps(annexb);
  if (avc) return codecFromSps(avc);
  const sps = nals(annexb).find((n) => n.length && hevcType(n[0]) === 33);
  return sps ? codecFromHevcSps(sps) : null;
}

/// Whether this browser can decode HEVC (Main, level 4.0), to offer it at all. Annex B in-band
/// parameter sets, so no `description` (WebCodecs HEVC registration).
export async function hevcSupported(vd: Pick<typeof VideoDecoder, 'isConfigSupported'> | undefined = globalThis.VideoDecoder): Promise<boolean> {
  try {
    return !!vd && !!(await vd.isConfigSupported({ codec: 'hvc1.1.6.L120.B0' })).supported;
  } catch {
    return false;
  }
}

/// Which frames may go to the decoder. It starts waiting for a key frame (WebCodecs needs one
/// after configure), takes deltas only in index order with no gap, and goes back to waiting
/// for a key frame on a gap, a reset, or a decode queue longer than 2.
export class KeyGate {
  needKey = true;
  private last = -1;

  /// `askKey`: ask the bridge for an IDR. True for every frame dropped while waiting, because an
  /// ask can be lost (throttled, or its IDR dropped for a backed-up queue) and Sunshine sends no
  /// periodic IDR; the caller's 100 ms throttle keeps this to 10 requests a second.
  accept(idx: number, idr: boolean, decodeQueueSize: number): { decode: boolean; askKey: boolean } {
    if (decodeQueueSize > 2) this.needKey = true;
    if (this.last >= 0 && !after(idx, this.last)) return { decode: false, askKey: this.needKey }; // stale
    if (idr && decodeQueueSize <= 2) {
      this.needKey = false;
      this.last = idx;
      return { decode: true, askKey: false };
    }
    if (!idr && !this.needKey) {
      if (((idx - this.last) >>> 0) === 1) {
        this.last = idx;
        return { decode: true, askKey: false };
      }
      this.needKey = true; // gap or out of order: the reference chain is broken
    }
    return { decode: false, askKey: this.needKey };
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
  /// Video payload + header bytes received (Chrome's transport getStats() reports 0 for this).
  bytesReceived: number;
  framesDecoded: number;
  framesPresented: number;
  /// Received but not decoded (waiting for a key frame, gaps, stale).
  framesDropped: number;
  /// Unused since frames are drawn on arrival (was: replaced before a paint); stays 0.
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
  /// Gap between requestAnimationFrame callbacks, and drawImage cost, ms.
  rafMs: Window;
  /// Last 6 gaps over 250 ms: source (bawn write clock), arrival (page), raf.
  stalls: string[];
  /// Frame sizes: bytes of the last frame, and the current / longest run of tiny frames (< 300 B = unchanged picture).
  lastBytes: number;
  tinyRun: number;
  tinyMax: number;
  drawMs: Window;
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
    framesReceived: 0, bytesReceived: 0, framesDecoded: 0, framesPresented: 0, framesDropped: 0, framesSkipped: 0,
    streamsReset: 0, decodeErrors: 0, keyRequests: 0, decodeQueueSize: 0, lastError: null,
    bridgeCaptureMs: new Window(), decodeMs: new Window(), presentMs: new Window(),
    rafMs: new Window(), drawMs: new Window(), stalls: [], lastBytes: 0, tinyRun: 0, tinyMax: 0,
    jitterMs: new Window(), queueMs: new Window(),
  };
  // desynchronized: draw straight to the front buffer, no wait for the compositor's next frame.
  const ctx = canvas.getContext('2d', { alpha: false, desynchronized: true })!;
  const gate = new KeyGate();
  const decodeStart = new Map<number, number>();
  let config: VideoDecoderConfig | null = null;
  let decoder: VideoDecoder | null = null;
  let configuring = false;
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
        // Drawn on arrival (no rAF wait, so none are skipped); the display shows the latest at its next refresh.
        if (canvas.width !== frame.displayWidth || canvas.height !== frame.displayHeight) {
          canvas.width = stats.width = frame.displayWidth;
          canvas.height = stats.height = frame.displayHeight;
        }
        const d0 = performance.now();
        ctx.drawImage(frame, 0, 0);
        const d1 = performance.now();
        frame.close();
        stats.drawMs.push(d1 - d0);
        stats.framesPresented++;
        stats.presentMs.push(d1 - now);
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
    stats.bytesReceived += b.length;
    stats.lastBytes = b.length - HEADER_LEN;
    if (stats.lastBytes < 300) {
      if (++stats.tinyRun > stats.tinyMax) stats.tinyMax = stats.tinyRun;
    } else {
      if (stats.tinyRun >= 20) {
        stats.stalls.push(`t=${(performance.now() / 1000).toFixed(1)}s SAME PICTURE for ${stats.tinyRun} frames (tiny frames from bawn) end_epoch=${Date.now()}`);
        if (stats.stalls.length > 6) stats.stalls.shift();
      }
      stats.tinyRun = 0;
    }
    const bridgeMs = Number(h.bridgeNs) / 1e6;
    if (h.captureNs !== 0n) stats.bridgeCaptureMs.push(Number(h.bridgeNs - h.captureNs) / 1e6);
    const q = arrival - bridgeMs;
    stats.queueMs.push(q);
    if (prev && arrival - prev.arrival > 250) {
      stats.stalls.push(`t=${(arrival / 1000).toFixed(1)}s src gap ${(bridgeMs - prev.bridgeMs).toFixed(0)} ms, arrival gap ${(arrival - prev.arrival).toFixed(0)} ms`);
      if (stats.stalls.length > 6) stats.stalls.shift();
    }
    if (prev) stats.jitterMs.push(Math.abs(arrival - prev.arrival - (bridgeMs - prev.bridgeMs)));
    prev = { arrival, bridgeMs };
    const annexb = b.subarray(HEADER_LEN);
    if (!config) {
      if (h.idr && !configuring) {
        const codec = codecFromAccessUnit(annexb);
        if (codec) configure(codec).catch((e) => (stats.lastError = String(e)));
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

  let lastRaf = 0;
  const paint = (t: number) => {
    if (lastRaf) stats.rafMs.push(t - lastRaf);
    if (lastRaf && t - lastRaf > 250) {
      stats.stalls.push(`t=${(t / 1000).toFixed(1)}s page raf gap ${(t - lastRaf).toFixed(0)} ms vis=${document.visibilityState} focus=${document.hasFocus()} lock=${document.pointerLockElement ? 1 : 0}`);
      if (stats.stalls.length > 6) stats.stalls.shift();
    }
    lastRaf = t;
    if (!ended) requestAnimationFrame(paint);
  };
  // The rAF probe stops with the session, so a closed stream costs nothing per frame.
  let ended = false;
  const onClosed = () => {
    ended = true;
    if (decoder && decoder.state !== 'closed') decoder.close();
  };
  transport.closed.then(onClosed, onClosed);
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
