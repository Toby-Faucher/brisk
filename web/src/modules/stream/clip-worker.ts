// Chooses and muxes a replay clip off the main thread. Messages: {begin: {from, width, height}},
// then batches {v?, a?} of ring entries (replay.ts V / A), then {end: true}; answers
// {blob, seconds} or {error}.
import { muxClip } from './mp4';
import { selectClip, type A, type V } from './replay';

let v: V[] = [];
let a: A[] = [];
let begin: { from: number; width: number; height: number } | null = null;

self.onmessage = async (e: MessageEvent) => {
  const m = e.data;
  if (m.begin) {
    begin = m.begin;
    v = [];
    a = [];
  } else if (!m.end) {
    if (m.v) v.push(...m.v);
    if (m.a) a.push(...m.a);
  } else if (begin) {
    const clip = selectClip(v, a, begin.from);
    v = a = []; // the clip holds what it uses; the rest can go
    if (!clip) return void self.postMessage({ error: 'nothing to save yet' });
    const seconds = clip.seconds;
    try {
      self.postMessage({ blob: await muxClip(clip, begin.width, begin.height), seconds });
    } catch (err) {
      self.postMessage({ error: String((err as Error)?.message ?? err) });
    }
  }
};
