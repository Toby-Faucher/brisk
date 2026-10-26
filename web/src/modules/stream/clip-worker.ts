// Muxes a replay clip off the main thread. Messages: {begin: Clip without packets, width, height},
// then batches {video?, audio?}, then {end: true}; answers {blob} or {error}.
import { muxClip } from './mp4';
import type { Clip } from './replay';

let clip: Clip | null = null;
let size = { width: 0, height: 0 };

self.onmessage = async (e: MessageEvent) => {
  const m = e.data;
  if (m.begin) {
    clip = { ...m.begin, video: [], audio: [] };
    size = { width: m.width, height: m.height };
  } else if (clip && !m.end) {
    if (m.video) clip.video.push(...m.video);
    if (m.audio) clip.audio.push(...m.audio);
  } else if (clip) {
    try {
      const buf = await muxClip(clip, size.width, size.height);
      self.postMessage({ blob: new Blob([buf], { type: 'video/mp4' }) });
    } catch (err) {
      self.postMessage({ error: String((err as Error)?.message ?? err) });
    }
    clip = null;
  }
};
