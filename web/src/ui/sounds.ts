import { get } from 'svelte/store';
import { settings } from '../state/settings';

// Short synthesized tones: [frequency Hz, duration s] played back to back.
const TONES = {
  click: [[1200, 0.03]],
  connect: [[660, 0.08], [880, 0.08], [1320, 0.18]],
  error: [[330, 0.1], [220, 0.2]],
} as const;

let ctx: AudioContext | null = null;

export function play(name: keyof typeof TONES) {
  if (!get(settings).sounds || typeof AudioContext === 'undefined') return;
  try {
    ctx ??= new AudioContext();
    if (ctx.state === 'suspended') void ctx.resume();
    let t = ctx.currentTime + 0.01;
    for (const [f, d] of TONES[name]) {
      const o = ctx.createOscillator();
      const g = ctx.createGain();
      o.type = 'sine';
      o.frequency.value = f;
      g.gain.setValueAtTime(0, t);
      g.gain.linearRampToValueAtTime(0.08, t + 0.005);
      g.gain.exponentialRampToValueAtTime(0.0001, t + d);
      o.connect(g).connect(ctx.destination);
      o.start(t);
      o.stop(t + d + 0.02);
      t += d * 0.85;
    }
  } catch {}
}
