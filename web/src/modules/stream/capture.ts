// Screenshot and replay clip, all in this browser: files download to this machine (a[download]),
// the screenshot is also copied to its clipboard when Chrome allows it.
import type { Clip } from './replay';

/** Ctrl+Alt+S / Ctrl+Alt+R: reserved, never forwarded to the game. Shift or Meta held = not ours. */
export function hotkey(e: { code: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }): 'shot' | 'clip' | null {
  if (!e.ctrlKey || !e.altKey || e.shiftKey || e.metaKey) return null;
  return e.code === 'KeyS' ? 'shot' : e.code === 'KeyR' ? 'clip' : null;
}

/** `brisk-<game or desktop>-<YYYYMMDD-HHMMSS>.<ext>`, local time. */
export function fileName(game: string | null, ext: 'png' | 'mp4', d = new Date()): string {
  const slug = (game ?? '').toLowerCase().normalize('NFKD').replace(/[\u0300-\u036f]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 60) || 'desktop';
  const p = (n: number) => String(n).padStart(2, '0');
  return `brisk-${slug}-${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}.${ext}`;
}

export function download(blob: Blob, name: string): void {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 60_000);
}

/** The canvas at its own (the stream's) resolution as PNG. Call inside the key or click handler:
 *  the clipboard write needs that gesture, and gets a promise so it starts before the encode ends. */
export async function screenshot(canvas: HTMLCanvasElement): Promise<{ blob: Blob; copied: boolean }> {
  const png = new Promise<Blob>((res, rej) => canvas.toBlob((b) => (b ? res(b) : rej(new Error('nothing on screen yet'))), 'image/png'));
  let copied: Promise<boolean> = Promise.resolve(false);
  try {
    copied = navigator.clipboard.write([new ClipboardItem({ 'image/png': png })]).then(() => true, () => false);
  } catch {}
  return { blob: await png, copied: await copied };
}

/** Bytes per message to the worker: each batch is one main-thread copy of about 2 ms. */
const BATCH_BYTES = 8 << 20;

/** Muxes `clip` in a worker. The ring keeps its references: each packet is copied once
 *  (slice, then transferred), in batches with a yield between them so the stream keeps going. */
export async function muxInWorker(clip: Clip, width: number, height: number, worker = () => new Worker(new URL('./clip-worker.ts', import.meta.url), { type: 'module' })): Promise<Blob> {
  const w = worker();
  try {
    const done = new Promise<Blob>((res, rej) => {
      w.onmessage = (e) => (e.data.blob ? res(e.data.blob) : rej(new Error(e.data.error)));
      w.onerror = (e) => rej(new Error(e.message || 'clip worker failed'));
    });
    const { video, audio, ...begin } = clip;
    w.postMessage({ begin, width, height });
    for (const [key, list] of [['video', video], ['audio', audio]] as const) {
      for (let i = 0; i < list.length; ) {
        const batch: unknown[] = [];
        const bufs: ArrayBuffer[] = [];
        for (let n = 0; i < list.length && n < BATCH_BYTES; i++) {
          const data = list[i].data.slice();
          n += data.byteLength;
          bufs.push(data.buffer);
          batch.push({ ...list[i], data });
        }
        w.postMessage({ [key]: batch }, bufs);
        await new Promise((r) => setTimeout(r, 0));
      }
    }
    w.postMessage({ end: true });
    return await done;
  } finally {
    w.terminate();
  }
}
