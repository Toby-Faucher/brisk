// Muxes a replay Clip (replay.ts) into an MP4 with mediabunny (MPL-2.0). Given Annex B packets
// and no `description`, mediabunny builds the avcC / hvcC from the key frame's in-band SPS/PPS
// (+VPS) and rewrites every access unit to 4-byte length-prefixed NAL units (AVCC / HVCC).
import { BufferTarget, EncodedAudioPacketSource, EncodedPacket, EncodedVideoPacketSource, Mp4OutputFormat, Output } from 'mediabunny';
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

export async function muxClip(c: Clip, width: number, height: number): Promise<ArrayBuffer> {
  const target = new BufferTarget();
  const out = new Output({ format: new Mp4OutputFormat({ fastStart: 'in-memory' }), target });
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
      const f = c.video[i++];
      await vs.add(new EncodedPacket(f.data, f.key ? 'key' : 'delta', f.t, f.d), i === 1 ? vmeta : undefined);
    } else {
      const p = c.audio[j++];
      await as!.add(new EncodedPacket(p.data, 'key', p.t, 240 / 48000), j === 1 ? ameta : undefined);
    }
  }
  await out.finalize();
  return target.buffer!;
}
