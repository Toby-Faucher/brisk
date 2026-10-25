// AudioWorklet side of brisk audio: the Playout ring (audio.ts) fed with decoded packets from
// the main thread, pulled once per render quantum. Posts its counters about 4 times a second.
import { Playout } from './audio';

// AudioWorkletGlobalScope (not in lib.dom).
declare class AudioWorkletProcessor {
  readonly port: MessagePort;
}
declare function registerProcessor(name: string, ctor: new () => AudioWorkletProcessor): void;
declare const sampleRate: number;

class BriskPlayout extends AudioWorkletProcessor {
  private p = new Playout();
  private quanta = 0;

  constructor() {
    super();
    this.port.onmessage = (e: MessageEvent<{ seq: number; l: Float32Array; r: Float32Array }>) => {
      this.p.push(e.data.seq, e.data.l, e.data.r);
    };
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const [l, r] = outputs[0];
    this.p.pull(l, r ?? new Float32Array(l.length));
    if (++this.quanta % Math.round(sampleRate / 128 / 4) === 0) this.port.postMessage(this.p.stats);
    return true;
  }
}

registerProcessor('brisk-playout', BriskPlayout);
