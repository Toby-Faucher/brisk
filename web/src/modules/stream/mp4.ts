// Muxes a replay Clip (replay.ts) into an MP4 with mediabunny (MPL-2.0). Given Annex B packets
// and no `description`, mediabunny builds the avcC / hvcC from the key frame's in-band SPS/PPS
// (+VPS) and rewrites every access unit to 4-byte length-prefixed NAL units (AVCC / HVCC).
import { EncodedAudioPacketSource, EncodedPacket, EncodedVideoPacketSource, Mp4OutputFormat, Output, StreamTarget } from 'mediabunny';
import type { Clip } from './replay';

/** Encoder lookahead of libopus in RESTRICTED_LOWDELAY (Sunshine's mode): Fs/400 = 2.5 ms. */
export const OPUS_PRESKIP = 120;

/** RFC 7845 OpusHead for Sunshine's stereo 48 kHz stream; mediabunny turns it into the dOps box. */
export function opusHead(preSkip = OPUS_PRESKIP): Uint8Array {
  const b = new Uint8Array(19);
  const v = new DataView(b.buffer);
  b.set([0x4f, 0x70, 0x75, 0x73, 0x48, 0x65, 0x61, 0x64, 1, 2]); // "OpusHead", version 1, 2 channels
  v.setUint16(10, preSkip, true);
  v.setUint32(12, 48000, true);
  return b; // output gain 0, channel mapping family 0
}

/** Collects the muxer's writes as Blob parts: appends, plus in-place patches (the mdat size). */
function parts() {
  const list: { pos: number; data: Uint8Array }[] = [];
  let end = 0;
  const stream = new WritableStream<{ type: 'write'; data: Uint8Array; position: number }>({
    write({ data, position }) {
      if (position === end) {
        list.push({ pos: position, data });
        end += data.byteLength;
        return;
      }
      if (position + data.byteLength > end) throw new Error('mp4: write past the end');
      for (const p of list) {
        const a = Math.max(position, p.pos);
        const b = Math.min(position + data.byteLength, p.pos + p.data.byteLength);
        if (a < b) p.data.set(data.subarray(a - position, b - position), a - p.pos);
      }
    },
  });
  return { stream, blob: () => new Blob(list.map((p) => p.data as BlobPart), { type: 'video/mp4' }) };
}

/** Muxes `c` (its packets are dropped as they are written, so memory peaks near one clip, not
 *  two); moov at the end (no second in-memory copy for fast start). */
export async function muxClip(c: Clip, width: number, height: number): Promise<Blob> {
  const sink = parts();
  const out = new Output({ format: new Mp4OutputFormat({ fastStart: false }), target: new StreamTarget(sink.stream) });
  const vs = new EncodedVideoPacketSource(c.kind);
  out.addVideoTrack(vs);
  const as = c.audio.length ? new EncodedAudioPacketSource('opus') : null;
  if (as) out.addAudioTrack(as);
  await out.start();
  const vmeta = { decoderConfig: { codec: c.codec, codedWidth: width, codedHeight: height } };
  const ameta = { decoderConfig: { codec: 'opus', numberOfChannels: 2, sampleRate: 48000, description: opusHead() } };
  // Interleaved by time, as an MP4 muxer expects.
  let i = 0;
  let j = 0;
  while (i < c.video.length || j < c.audio.length) {
    if (j >= c.audio.length || (i < c.video.length && c.video[i].t <= c.audio[j].t)) {
      const f = c.video[i];
      c.video[i++] = null!;
      await vs.add(new EncodedPacket(f.data, f.key ? 'key' : 'delta', f.t, f.d), i === 1 ? vmeta : undefined);
    } else {
      const p = c.audio[j];
      c.audio[j++] = null!;
      await as!.add(new EncodedPacket(p.data, 'key', p.t, 240 / 48000), j === 1 ? ameta : undefined);
    }
  }
  await out.finalize();
  return sink.blob();
}
