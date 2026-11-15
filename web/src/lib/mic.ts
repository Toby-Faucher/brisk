// Brisk mic: getUserMedia -> AudioWorklet (mic-worklet.ts) -> 10 ms PCM datagrams to the bridge.
// Packet: [0x30][seq u16 LE][480 x s16 LE], mono, 48 kHz. Datagrams are unreliable, which is
// fine for a live mic; the bridge drops stale seqs.

export const KIND_MIC = 0x30;
export const MIC_FRAMES = 480;
export const MIC_PACKET_LEN = 3 + MIC_FRAMES * 2;

export function packMic(seq: number, pcm: Float32Array): Uint8Array {
  const out = new Uint8Array(MIC_PACKET_LEN);
  const v = new DataView(out.buffer);
  out[0] = KIND_MIC;
  v.setUint16(1, seq & 0xffff, true);
  for (let i = 0; i < MIC_FRAMES; i++) {
    v.setInt16(3 + i * 2, Math.round(Math.max(-1, Math.min(1, pcm[i])) * 32767), true);
  }
  return out;
}

/// Collects worklet blocks (128 frames) into 480-frame packets.
export class MicPacker {
  private buf = new Float32Array(MIC_FRAMES);
  private n = 0;
  private seq = 0;

  push(block: Float32Array): Uint8Array[] {
    const out: Uint8Array[] = [];
    let i = 0;
    while (i < block.length) {
      const k = Math.min(MIC_FRAMES - this.n, block.length - i);
      this.buf.set(block.subarray(i, i + k), this.n);
      this.n += k;
      i += k;
      if (this.n === MIC_FRAMES) {
        out.push(packMic(this.seq, this.buf));
        this.seq = (this.seq + 1) & 0xffff;
        this.n = 0;
      }
    }
    return out;
  }
}

export type Mic = { stop(): void };

/// Call from the click handler: the AudioContext is made before the first await so it keeps the
/// click's activation. `workletUrl`: the bundled mic-worklet.ts.
export async function startMic(t: WebTransport, workletUrl: string): Promise<Mic> {
  const ctx = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' });
  let stream: MediaStream | null = null;
  let node: AudioWorkletNode | null = null;
  let writer: WritableStreamDefaultWriter<Uint8Array> | null = null;
  let stopped = false;
  const stop = () => {
    if (stopped) return;
    stopped = true;
    stream?.getTracks().forEach((tr) => tr.stop());
    if (node) node.port.onmessage = null;
    node?.disconnect();
    void ctx.close().catch(() => {});
    try {
      writer?.releaseLock();
    } catch {}
  };
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    });
    await ctx.audioWorklet.addModule(workletUrl);
    const src = ctx.createMediaStreamSource(stream);
    node = new AudioWorkletNode(ctx, 'brisk-mic', { numberOfOutputs: 1, outputChannelCount: [1] });
    // The output stays silent; the connection makes Chrome pull the node.
    src.connect(node).connect(ctx.destination);
    writer = t.datagrams.writable.getWriter();
    const w = writer;
    const packer = new MicPacker();
    node.port.onmessage = (e: MessageEvent<Float32Array>) => {
      for (const p of packer.push(e.data)) w.write(p).catch(() => {});
    };
  } catch (e) {
    stop();
    throw e;
  }
  return { stop };
}
