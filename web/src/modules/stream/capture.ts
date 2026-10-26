// Screenshot and replay clip, all in this browser: files download to this machine (a[download]),
// the screenshot is also copied to its clipboard when Chrome allows it.
import type { Snapshot } from './replay';

type Keys = { code: string; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean; getModifierState?(k: string): boolean };

/** Ctrl+Alt+S / Ctrl+Alt+R: reserved, never forwarded to the game. Shift, Meta or AltGr held = not ours. */
export function hotkey(e: Keys): 'shot' | 'clip' | null {
  if (!e.ctrlKey || !e.altKey || e.shiftKey || e.metaKey || e.getModifierState?.('AltGraph')) return null;
  return e.code === 'KeyS' ? 'shot' : e.code === 'KeyR' ? 'clip' : null;
}

/** Catches the hotkeys on `win` in the capture phase, before input.ts (window, bubble phase) sees
 *  them; their keyup is swallowed too. `active`: only while streaming. Returns the remover. */
export function reserveHotkeys(win: EventTarget, active: () => boolean, on: (k: 'shot' | 'clip') => void): () => void {
  const swallowed = new Set<string>();
  const f = (e: Event) => {
    const ke = e as KeyboardEvent;
    if (!active()) return;
    const k = ke.type === 'keydown' ? hotkey(ke) : null;
    if (!k && !(ke.type === 'keyup' && swallowed.delete(ke.code))) return;
    ke.preventDefault();
    ke.stopImmediatePropagation();
    if (!k) return;
    swallowed.add(ke.code);
    if (!ke.repeat) on(k);
  };
  win.addEventListener('keydown', f, true);
  win.addEventListener('keyup', f, true);
  return () => {
    win.removeEventListener('keydown', f, true);
    win.removeEventListener('keyup', f, true);
  };
}

/** `brisk-<game or desktop>-<YYYYMMDD-HHMMSS>.<ext>`, local time. */
export function fileName(game: string | null, ext: 'png' | 'mp4', d = new Date()): string {
  const slug = (game ?? '').toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 60) || 'desktop';
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

/** The canvas at its own (the stream's) resolution as PNG, and its clipboard copy. Call inside the
 *  key or click handler: the clipboard write needs that gesture and gets the PNG as a promise. */
export function screenshot(canvas: HTMLCanvasElement): { png: Promise<Blob>; copied: Promise<boolean> } {
  const png = new Promise<Blob>((res, rej) => canvas.toBlob((b) => (b ? res(b) : rej(new Error('nothing on screen yet'))), 'image/png'));
  let copied: Promise<boolean> = Promise.resolve(false);
  try {
    copied = navigator.clipboard.write([new ClipboardItem({ 'image/png': png })]).then(() => true, () => false);
  } catch {}
  return { png, copied };
}

/** Bytes per message to the worker: each batch is one main-thread copy well under 1 ms. */
const BATCH_BYTES = 2 << 20;
export const CLIP_TIMEOUT_MS = 30_000;

/** Chooses and muxes the clip in a worker. The ring keeps its references: each packet is copied
 *  once (slice, then transferred), in batches with a yield between them so the stream keeps going. */
export async function clipInWorker(snap: Snapshot, width: number, height: number, worker = () => new Worker(new URL('./clip-worker.ts', import.meta.url), { type: 'module' })): Promise<{ blob: Blob; seconds: number }> {
  const w = worker();
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const done = new Promise<{ blob: Blob; seconds: number }>((res, rej) => {
      w.onmessage = (e) => (e.data.blob ? res(e.data) : rej(new Error(e.data.error)));
      w.onerror = (e) => rej(new Error(e.message || 'clip worker failed'));
      w.onmessageerror = () => rej(new Error('clip worker message failed'));
      timer = setTimeout(() => rej(new Error('timed out')), CLIP_TIMEOUT_MS);
    });
    done.catch(() => {}); // awaited below; a failure while still posting is not "unhandled"
    w.postMessage({ begin: { from: snap.from, width, height } });
    for (const [key, list] of [['v', snap.v], ['a', snap.a]] as const) {
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
    clearTimeout(timer);
    w.terminate();
  }
}
