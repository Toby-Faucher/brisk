// Instant replay: a time- and byte-capped ring of the encoded video access units (Annex B, as the
// bridge sends them) and Opus packets this page already received, kept as references (never
// copied), and the pure choice of what goes into a clip. Muxing is mp4.ts (in a worker).
import { codecFromAccessUnit, nals } from '../../lib/video';
import type { AudioHeader } from '../../lib/audio';

/** Video + audio kept at most (Crisp 40 Mbps for 30 s is about 150 MB). */
export const REPLAY_CAP_BYTES = 160 << 20;
/** Counted per entry on top of its bytes, so 200 tiny audio packets a second count too. */
const ENTRY_BYTES = 64;
/** Sunshine sends no periodic IDR: while replay is on the page asks for one when none came for this long. */
export const KEY_EVERY_MS = 5000;
/** One Sunshine Opus packet: 5 ms at 48 kHz. */
export const OPUS_FRAMES = 240;
const RATE = 48000;

type V = { data: Uint8Array; key: boolean; ns: number; at: number };
type A = { data: Uint8Array; ts: number; at: number };

export type ClipVideo = { data: Uint8Array; key: boolean; t: number; d: number };
export type ClipAudio = { data: Uint8Array; t: number };
/** Times in seconds from the clip's first (key) frame. */
export type Clip = { kind: 'avc' | 'hevc'; codec: string; video: ClipVideo[]; audio: ClipAudio[]; seconds: number; bytes: number };

export class Replay {
  private v: V[] = [];
  private a: A[] = [];
  private vh = 0; // index of the oldest live entry (entries before it are evicted, compacted later)
  private ah = 0;
  bytes = 0;
  /** Arrival of the newest key frame (performance.now() ms). */
  lastKeyAt = -Infinity;

  /** `seconds` 0 = off: nothing is kept. */
  constructor(readonly seconds: number, private cap = REPLAY_CAP_BYTES, private now = () => performance.now()) {}

  /** A video access unit that went to the decoder (so the ring always starts at a key frame). */
  video(data: Uint8Array, key: boolean, bridgeNs: bigint, at: number): void {
    if (!this.seconds || (!key && this.vh === this.v.length)) return; // a delta without its key frame is useless
    if (key) this.lastKeyAt = at;
    this.v.push({ data, key, ns: Number(bridgeNs), at });
    this.bytes += data.byteLength + ENTRY_BYTES;
    this.evict(at);
  }

  audio(h: AudioHeader, at = this.now()): void {
    if (!this.seconds) return;
    this.a.push({ data: h.opus, ts: h.ts48, at });
    this.bytes += h.opus.byteLength + ENTRY_BYTES;
    this.evict(at);
  }

  /** True when the page should ask the bridge for a key frame now. */
  wantKey(now = this.now()): boolean {
    return !!this.seconds && now - this.lastKeyAt >= KEY_EVERY_MS;
  }

  get frames(): number {
    return this.v.length - this.vh;
  }

  clear(): void {
    this.v = [];
    this.a = [];
    this.vh = this.ah = 0;
    this.bytes = 0;
  }

  private dropVideo(): void {
    const f = this.v[this.vh++];
    this.bytes -= f.data.byteLength + ENTRY_BYTES;
    // Deltas without their key frame are useless: drop through to the next key frame.
    while (this.vh < this.v.length && !this.v[this.vh].key) {
      this.bytes -= this.v[this.vh++].data.byteLength + ENTRY_BYTES;
    }
  }

  private dropAudio(): void {
    this.bytes -= this.a[this.ah++].data.byteLength + ENTRY_BYTES;
  }

  /** Oldest first, by time (older than `seconds`) and by bytes (over the cap). */
  private evict(now: number): void {
    const cutoff = now - this.seconds * 1000;
    while (this.vh < this.v.length && this.v[this.vh].at < cutoff) this.dropVideo();
    while (this.ah < this.a.length && this.a[this.ah].at < cutoff) this.dropAudio();
    while (this.bytes > this.cap) {
      const va = this.vh < this.v.length ? this.v[this.vh].at : Infinity;
      const aa = this.ah < this.a.length ? this.a[this.ah].at : Infinity;
      if (va === Infinity && aa === Infinity) break;
      if (va <= aa) this.dropVideo();
      else this.dropAudio();
    }
    if (this.vh > 1024 && this.vh * 2 > this.v.length) (this.v = this.v.slice(this.vh)), (this.vh = 0);
    if (this.ah > 4096 && this.ah * 2 > this.a.length) (this.a = this.a.slice(this.ah)), (this.ah = 0);
  }

  /** The clip of the last `seconds`, or null when there is no key frame yet. References only. */
  clip(now = this.now()): Clip | null {
    if (!this.seconds) return null;
    this.evict(now);
    return selectClip(this.v.slice(this.vh), this.a.slice(this.ah), now - this.seconds * 1000);
  }
}

const isHevc = (key: Uint8Array) => !codecFromAccessUnit(key)?.startsWith('avc1');
/** The parameter-set NAL units of a key frame (H.264 SPS/PPS, HEVC VPS/SPS/PPS). They lead the
 *  access unit, so only its first 1 KiB is scanned (an IDR at Crisp is hundreds of KB). */
function paramSets(au: Uint8Array, hevc: boolean): Uint8Array[] {
  return nals(au.subarray(0, 1024)).filter((n) => n.length && (hevc ? [32, 33, 34].includes((n[0] >> 1) & 0x3f) : [7, 8].includes(n[0] & 0x1f)));
}
const sameBytes = (a: Uint8Array, b: Uint8Array) => a.length === b.length && a.every((x, i) => x === b[i]);
const sameSets = (a: Uint8Array[], b: Uint8Array[]) => a.length === b.length && a.every((x, i) => sameBytes(x, b[i]));

/** Pure clip choice. `v`, `a`: in arrival order; `from`: now - N s on the arrival clock (ms).
 *  Video: from the first key frame at or after `from` (else the earliest) whose parameter sets
 *  equal the newest key frame's, so a resolution change mid-buffer clips only the last config.
 *  Audio: the packets overlapping that video, placed by their own timestamps (no drift); gaps are
 *  filled with 1-byte Opus packets (TOC only = a lost frame: the decoder conceals, then silence). */
export function selectClip(v: V[], a: A[], from: number): Clip | null {
  let last = -1;
  for (let i = v.length - 1; i >= 0; i--) if (v[i].key) (last = i), (i = -1);
  if (last < 0) return null;
  const codec = codecFromAccessUnit(v[last].data);
  if (!codec) return null;
  const hevc = isHevc(v[last].data);
  const sets = paramSets(v[last].data, hevc);
  let start = last;
  let atOrAfter = v[last].at >= from ? last : -1;
  for (let i = last - 1; i >= 0; i--) {
    if (!v[i].key) continue;
    if (!sameSets(paramSets(v[i].data, hevc), sets)) break;
    start = i;
    if (v[i].at >= from) atOrAfter = i;
  }
  if (atOrAfter >= 0) start = atOrAfter;
  const sel = v.slice(start);

  // Video times: the bridge's write clock (bawn CLOCK_MONOTONIC), strictly increasing.
  const t0 = sel[0].ns;
  const video: ClipVideo[] = [];
  let prev = -1;
  for (const f of sel) {
    const t = Math.max((f.ns - t0) / 1e9, prev + 1e-4);
    if (video.length) video[video.length - 1].d = t - prev;
    video.push({ data: f.data, key: f.key, t, d: 1 / 60 });
    prev = t;
  }
  if (video.length > 1) video[video.length - 1].d = video[video.length - 2].d;
  const end = prev + video[video.length - 1].d;

  // Audio clock (Sunshine's RTP ts) to video clock: the smallest arrival - send of each (the
  // least-queued packet / frame), both on this page's clock.
  const audio: ClipAudio[] = [];
  if (a.length) {
    const base = a[0].ts;
    const pk = a.map((p) => ({ data: p.data, d: (p.ts - base) | 0, at: p.at })); // signed: u32 wrap and reordering
    let offA = Infinity;
    for (const p of pk) offA = Math.min(offA, p.at - p.d / 48);
    let offV = Infinity;
    for (const f of sel) offV = Math.min(offV, f.at - f.ns / 1e6);
    const rel = (d: number) => (d / 48 + offA - offV - t0 / 1e6) / 1000; // s from the clip start
    pk.sort((x, y) => x.d - y.d);
    let next = 0; // expected d of the next packet
    let first = -1;
    let t0a = 0;
    for (const p of pk) {
      const r = rel(p.d);
      if (r < 0 || r >= end) continue;
      if (first < 0) {
        first = p.d;
        next = p.d;
        t0a = Math.round(r * RATE) / RATE;
      }
      if (p.d < next) continue; // duplicate or overlap
      const lost = p.data.length ? Uint8Array.of(p.data[0] & 0xfc) : null;
      for (; p.d - next >= OPUS_FRAMES; next += OPUS_FRAMES) if (lost) audio.push({ data: lost, t: t0a + (next - first) / RATE });
      audio.push({ data: p.data, t: t0a + (p.d - first) / RATE });
      next = p.d + OPUS_FRAMES;
    }
  }
  const bytes = video.reduce((s, f) => s + f.data.byteLength, 0) + audio.reduce((s, p) => s + p.data.byteLength, 0);
  return { kind: hevc ? 'hevc' : 'avc', codec, video, audio, seconds: end, bytes };
}
