// The upload engine: sends game files to a machine in slices that can be resumed (docs/plans/2026-10-28-stage2-upload.md).
// Per file: create the upload, PUT one slice at a time with its offset, finish. A dropped connection or a 409 asks the
// machine where it is and carries on from there. At most two files at once; one slice per file is ever cut from the File.
import { ApiError, type ScanResult, type UploadApi, type UploadDone } from './api';

export type ItemState = 'queued' | 'sending' | 'finishing' | 'done' | 'failed' | 'cancelled';
export type Item = {
  key: number;
  name: string;
  size: number;
  state: ItemState;
  /** Bytes the machine has confirmed. */
  offset: number;
  /** Bytes per second, smoothed; 0 until the first slice is through. */
  speed: number;
  etaS: number | null;
  error?: string;
  /** Extra line after a finished file ("Stored, but no emulator can play it yet."). */
  note?: string;
  result?: UploadDone;
};

const DROPPED = 'The connection keeps dropping. Check the network and try again.';

/** Plain words for what went wrong. */
export function uploadError(e: unknown): string {
  if (!(e instanceof ApiError) || transient(e)) return DROPPED;
  switch (e.status) {
    case 507:
      return 'This machine keeps 5 GB free, so there is no room for this file.';
    case 415:
      return 'This file type is not supported.';
    case 422:
      return 'This file looks damaged.';
    case 429:
      return 'This machine is already taking two uploads. Try again in a moment.';
    case 404:
      return 'The machine lost track of this upload. Try again.';
    case 401:
    case 403:
      return 'Brisk is not signed in to this machine.';
    default:
      return e.message || `The machine answered ${e.status}.`;
  }
}

/** Worth waiting and asking again: no answer at all, the hub could not reach the machine, or a timeout. */
function transient(e: unknown): boolean {
  if (e instanceof DOMException && e.name === 'AbortError') return false;
  return !(e instanceof ApiError) || [0, 502, 503, 504].includes(e.status);
}

const label = (n: number, one: string) => [one, n] as [string, number];
/** The counts of a scan for display: Added always, the rest only when not zero. */
export function scanRows(r: ScanResult): [string, number][] {
  const rows = [
    label(r.added.length, 'Added'),
    label(r.aliased + r.duplicates, 'Already there'),
    label(r.removed, 'Skipped, you removed them'),
    label(r.unplayable, 'Stored, no emulator yet'),
    label(r.unknown, 'Not recognised'),
    label(r.broken, 'Damaged'),
  ];
  return rows.filter(([l, n]) => n > 0 || l === 'Added');
}

type Job = { item: Item; file: File; api: UploadApi; id?: string; ctl?: AbortController; cancelled?: boolean };

export type Options = {
  /** Largest slice; the machine's own chunk size (from create) caps it too. */
  sliceSize?: number;
  concurrency?: number;
  /** Network failures in a row before a file fails. */
  retries?: number;
  sleep?: (ms: number) => Promise<void>;
  now?: () => number;
  onChange?: (items: Item[]) => void;
  /** A file reached the machine (not called for failed or cancelled ones). */
  onDone?: (item: Item) => void;
};

export function createUploader(o: Options = {}) {
  const slice = o.sliceSize ?? 8 << 20;
  const limit = o.concurrency ?? 2;
  const retries = o.retries ?? 5;
  const sleep = o.sleep ?? ((ms: number) => new Promise<void>((r) => setTimeout(r, ms)));
  const now = o.now ?? (() => performance.now());
  let seq = 0;
  let jobs: Job[] = [];
  let running = 0;
  let idle: (() => void)[] = [];

  const snapshot = () => jobs.map((j) => ({ ...j.item }));
  const emit = () => o.onChange?.(snapshot());
  const set = (j: Job, p: Partial<Item>) => (Object.assign(j.item, p), emit());

  async function attempt(j: Job): Promise<void> {
    const { file, api } = j;
    let chunk = slice;
    let misses = 0; // network failures in a row
    // Carry on with an upload that already exists (a retry), else create one.
    const start = async () => {
      if (j.id) {
        try {
          return (await api.status(j.id)).offset;
        } catch (e) {
          if (!(e instanceof ApiError && e.status === 404)) throw e;
        }
      }
      const c = await api.create(file.name, file.size);
      j.id = c.id;
      chunk = Math.min(slice, c.chunk);
      return c.offset;
    };
    const wait = async () => {
      if (++misses > retries) throw new TypeError('gave up');
      await sleep(Math.min(500 * 2 ** (misses - 1), 8000));
    };
    let offset = 0;
    for (;;) {
      try {
        offset = await start();
        break;
      } catch (e) {
        if (j.cancelled) return;
        if (!transient(e)) throw e;
        await wait();
      }
    }
    set(j, { state: 'sending', offset });
    const id = j.id as string;
    const stamp = { t: now(), at: offset };
    const finishing = async () => {
      set(j, { state: 'finishing', etaS: null });
      for (;;) {
        try {
          return await api.finish(id);
        } catch (e) {
          if (j.cancelled) return null;
          if (e instanceof ApiError && e.status === 409) {
            offset = ((e.body as { offset?: number } | undefined)?.offset ?? 0);
            return 'again' as const;
          }
          if (!transient(e)) throw e;
          await wait();
          // The finish may have gone through before the answer was lost: the upload is gone then.
          try {
            await api.status(id);
          } catch (s) {
            if (s instanceof ApiError && s.status === 404) return 'unsure' as const;
          }
        }
      }
    };
    for (;;) {
      while (offset < file.size) {
        if (j.cancelled) return;
        const end = Math.min(offset + chunk, file.size);
        j.ctl = new AbortController();
        try {
          offset = (await api.put(id, offset, file.slice(offset, end), j.ctl.signal)).offset;
          misses = 0;
          const t = now();
          const inst = ((offset - stamp.at) / Math.max(t - stamp.t, 1)) * 1000;
          const speed = j.item.speed ? j.item.speed * 0.6 + inst * 0.4 : inst;
          Object.assign(stamp, { t, at: offset });
          set(j, { offset, speed, etaS: offset < file.size && speed > 0 ? (file.size - offset) / speed : null });
        } catch (e) {
          if (j.cancelled) return;
          if (e instanceof ApiError && e.status === 409) {
            if (++misses > retries) throw e;
            offset = (e.body as { offset?: number } | undefined)?.offset ?? (await api.status(id)).offset;
          } else if (transient(e)) {
            await wait();
            try {
              offset = (await api.status(id)).offset;
            } catch (s) {
              if (!transient(s)) throw s;
            }
          } else throw e;
          set(j, { offset });
        }
      }
      const r = await finishing();
      if (j.cancelled || r === null) return;
      if (r === 'again') {
        set(j, { state: 'sending', offset });
        continue;
      }
      if (r === 'unsure') return void set(j, { state: 'done', offset: file.size, etaS: null, note: 'Sent. Scan the games folder to be sure it is listed.' });
      const note = !r.registered ? 'Stored, but no emulator can play it yet.' : r.duplicate ? 'This game was already in the library.' : undefined;
      return void set(j, { state: 'done', offset: file.size, etaS: null, result: r, note });
    }
  }

  async function run(j: Job) {
    running++;
    set(j, { state: 'sending', error: undefined, note: undefined, speed: 0, etaS: null });
    try {
      await attempt(j);
    } catch (e) {
      if (!j.cancelled) {
        set(j, { state: 'failed', error: uploadError(e), etaS: null });
        // A refusal frees the machine's upload slot; no room (507) and a dropped line keep it for a retry.
        if (j.id && !transient(e) && !(e instanceof ApiError && e.status === 507)) (void j.api.abort(j.id).catch(() => {}), (j.id = undefined));
      }
    }
    if (j.cancelled) {
      set(j, { state: 'cancelled', etaS: null });
      if (j.id) void j.api.abort(j.id).catch(() => {});
    } else if (j.item.state === 'done') o.onDone?.({ ...j.item });
    running--;
    pump();
  }

  function pump() {
    for (const j of jobs) {
      if (running >= limit) break;
      if (j.item.state === 'queued' && !j.cancelled) void run(j);
    }
    if (!running && !jobs.some((j) => j.item.state === 'queued')) (idle.forEach((f) => f()), (idle = []));
  }

  return {
    /** Queues files for one machine's api. */
    add(files: File[], api: UploadApi) {
      for (const file of files) jobs.push({ file, api, item: { key: ++seq, name: file.name, size: file.size, state: 'queued', offset: 0, speed: 0, etaS: null } });
      emit();
      pump();
    },
    /** A queued file never starts; a running or failed one stops and its upload is deleted on the machine. */
    cancel(key: number) {
      const j = jobs.find((x) => x.item.key === key);
      if (!j || ['done', 'cancelled'].includes(j.item.state)) return;
      if (j.item.state === 'failed') return void (set(j, { state: 'cancelled' }), j.id && void j.api.abort(j.id).catch(() => {}));
      j.cancelled = true;
      j.ctl?.abort();
      if (j.item.state === 'queued') set(j, { state: 'cancelled' });
    },
    /** A failed file carries on with the same upload where the machine has it. */
    retry(key: number) {
      const j = jobs.find((x) => x.item.key === key);
      if (!j || j.item.state !== 'failed') return;
      set(j, { state: 'queued', error: undefined });
      pump();
    },
    /** Drops finished, failed and cancelled rows. */
    clear() {
      jobs = jobs.filter((j) => ['queued', 'sending', 'finishing'].includes(j.item.state));
      emit();
    },
    items: snapshot,
    /** Resolves when nothing is running or waiting. */
    settled: () => (running || jobs.some((j) => j.item.state === 'queued') ? new Promise<void>((r) => idle.push(r)) : Promise.resolve()),
  };
}
