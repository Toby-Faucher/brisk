// AudioWorklet side of the mic: posts each render quantum of the mono input to the main thread.

// AudioWorkletGlobalScope (not in lib.dom).
declare class AudioWorkletProcessor {
  readonly port: MessagePort;
}
declare function registerProcessor(name: string, ctor: new () => AudioWorkletProcessor): void;

class BriskMic extends AudioWorkletProcessor {
  process(inputs: Float32Array[][]): boolean {
    const ch = inputs[0]?.[0];
    // A copy: Chrome reuses the input buffers between quanta.
    if (ch) this.port.postMessage(ch.slice());
    return true;
  }
}

registerProcessor('brisk-mic', BriskMic);
