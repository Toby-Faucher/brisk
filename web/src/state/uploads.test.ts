import { describe, expect, it } from 'vitest';
import { ApiError, type UploadApi, type UploadDone } from './api';
import { createUploader, fmtEta, fmtSpeed, scanRows, uploadError, type Item } from './uploads';

const MB = 1 << 20;
const done: UploadDone = { system: 'wii', registered: true, duplicate: false, game: null };

/** A file that counts its slices; `live` is how many slices were cut and not yet sent. */
function file(name: string, size: number) {
  const f = {
    name,
    size,
    live: 0,
    maxLive: 0,
    slices: [] as [number, number][],
    slice(a: number, b: number) {
      f.live++;
      f.maxLive = Math.max(f.maxLive, f.live);
      f.slices.push([a, b]);
      return { size: b - a, a, b } as unknown as Blob;
    },
  };
  return f;
}
type F = ReturnType<typeof file>;

/** An in-memory machine. `script` can fail a PUT call by number (1-based). */
function fake(
  opts: {
    chunk?: number;
    putFail?: Record<number, unknown | (() => Promise<void>)>;
    createFail?: unknown;
    finish?: UploadDone | unknown;
  } = {},
) {
  const s = {
    api: null as unknown as UploadApi,
    ups: new Map<string, { size: number; offset: number }>(),
    puts: 0,
    inFlight: 0,
    maxInFlight: 0,
    creates: [] as string[],
    aborted: [] as string[],
    statuses: 0,
    offsets: [] as string[],
    finished: [] as string[],
  };
  let n = 0;
  s.api = {
    async create(name, size) {
      s.creates.push(name);
      s.inFlight++;
      s.maxInFlight = Math.max(s.maxInFlight, s.inFlight);
      await Promise.resolve();
      s.inFlight--;
      if (opts.createFail) throw opts.createFail;
      const id = `u${++n}`;
      s.ups.set(id, { size, offset: 0 });
      return { id, chunk: opts.chunk ?? 32 * MB, offset: 0 };
    },
    async put(id, offset, blob, signal) {
      const call = ++s.puts;
      const b = blob as unknown as { size: number; b: number };
      s.offsets.push(`${id}:${offset}`);
      s.inFlight++;
      s.maxInFlight = Math.max(s.maxInFlight, s.inFlight);
      try {
        await Promise.resolve();
        await new Promise((r) => setTimeout(r, 1));
        if (signal?.aborted) throw new DOMException('aborted', 'AbortError');
        const fail = opts.putFail?.[call];
        if (fail) throw fail;
        const u = s.ups.get(id)!;
        if (offset !== u.offset) throw new ApiError('wrong offset', 409, { offset: u.offset });
        u.offset += b.size;
        return { offset: u.offset };
      } finally {
        s.inFlight--;
      }
    },
    async status(id) {
      s.statuses++;
      const u = s.ups.get(id);
      if (!u) throw new ApiError('unknown upload', 404);
      return { id, name: '', offset: u.offset, size: u.size, state: 'open' };
    },
    async finish(id) {
      s.finished.push(id);
      if (opts.finish instanceof Error) throw opts.finish;
      s.ups.delete(id);
      return (opts.finish as UploadDone) ?? done;
    },
    async abort(id) {
      s.aborted.push(id);
      s.ups.delete(id);
    },
  };
  return s;
}

function rig(s: ReturnType<typeof fake>, extra = {}) {
  const log: Item[][] = [];
  const sleeps: number[] = [];
  const u = createUploader({
    sliceSize: 8 * MB,
    sleep: async (ms) => void sleeps.push(ms),
    onChange: (items) => log.push(items),
    ...extra,
  });
  return { u, log, sleeps, add: (fs: F[]) => u.add(fs as unknown as File[], s.api), settled: () => u.settled() };
}
const states = (u: ReturnType<typeof createUploader>) => u.items().map((i) => i.state);

describe('upload engine', () => {
  it('sends the file in slices, one at a time, then finishes', async () => {
    const s = fake();
    const r = rig(s);
    const f = file('a.iso', 20 * MB);
    r.add([f]);
    await r.settled();
    expect(f.slices).toEqual([
      [0, 8 * MB],
      [8 * MB, 16 * MB],
      [16 * MB, 20 * MB],
    ]);
    expect(s.finished).toEqual(['u1']);
    const it = r.u.items()[0];
    expect([it.state, it.offset, it.result?.registered]).toEqual(['done', 20 * MB, true]);
  });

  it('never cuts the next slice before the last one was sent', async () => {
    const s = fake();
    const r = rig(s);
    const f = file('a.iso', 40 * MB);
    // the fake api "drops" a slice once it is sent
    const put = s.api.put;
    s.api.put = async (...a) => {
      try {
        return await put(...a);
      } finally {
        f.live--;
      }
    };
    r.add([f]);
    await r.settled();
    expect(f.maxLive).toBe(1);
  });

  it('uses the smaller of the slice size and the machine chunk', async () => {
    const s = fake({ chunk: 4 * MB });
    const r = rig(s);
    const f = file('a.iso', 9 * MB);
    r.add([f]);
    await r.settled();
    expect(f.slices.map(([a, b]) => b - a)).toEqual([4 * MB, 4 * MB, 1 * MB]);
  });

  it('resumes after a dropped connection from the offset the machine reports', async () => {
    const s = fake({ putFail: { 2: new TypeError('Failed to fetch') } });
    const r = rig(s);
    const f = file('a.iso', 24 * MB);
    r.add([f]);
    await r.settled();
    expect(r.u.items()[0].state).toBe('done');
    expect(s.statuses).toBe(1);
    expect(s.offsets).toEqual(['u1:0', 'u1:8388608', 'u1:8388608', 'u1:16777216']);
    expect(r.sleeps.length).toBe(1);
  });

  it('realigns on a 409 and goes on from the offset it names', async () => {
    const s = fake();
    const r = rig(s);
    const f = file('a.iso', 24 * MB);
    // the machine already has 8 MiB the client does not know about
    const put = s.api.put;
    let first = true;
    s.api.put = async (id, off, b, sig) => {
      if (first) {
        first = false;
        s.ups.get(id)!.offset = 8 * MB;
      }
      return put(id, off, b, sig);
    };
    r.add([f]);
    await r.settled();
    expect(r.u.items()[0].state).toBe('done');
    expect(s.offsets).toEqual(['u1:0', 'u1:8388608', 'u1:16777216']);
  });

  it('gives up after bounded retries with a plain message and keeps the upload to retry', async () => {
    const down = new TypeError('Failed to fetch');
    const s = fake({ putFail: { 1: down, 2: down, 3: down, 4: down, 5: down, 6: down, 7: down, 8: down } });
    const r = rig(s, { retries: 3 });
    r.add([file('a.iso', 10 * MB)]);
    await r.settled();
    const it = r.u.items()[0];
    expect(it.state).toBe('failed');
    expect(it.error).toBe('The connection keeps dropping. Check the network and try again.');
    expect(s.puts).toBe(4);
    expect(r.sleeps).toEqual([500, 1000, 2000]);
    // retry carries on with the same upload
    r.u.retry(it.key);
    await r.settled();
    expect(s.creates.length).toBe(1);
    expect(r.u.items()[0].state).toBe('failed');
  });

  it('a retry after the network is back finishes the same upload', async () => {
    const down = new TypeError('Failed to fetch');
    const s = fake({ putFail: { 1: down, 2: down } });
    const r = rig(s, { retries: 1 });
    r.add([file('a.iso', 10 * MB)]);
    await r.settled();
    expect(r.u.items()[0].state).toBe('failed');
    r.u.retry(r.u.items()[0].key);
    await r.settled();
    expect(r.u.items()[0].state).toBe('done');
    expect(s.creates.length).toBe(1);
  });

  it('runs two files at a time and no more', async () => {
    const s = fake();
    const r = rig(s);
    const fs = [file('a.iso', 20 * MB), file('b.iso', 20 * MB), file('c.iso', 20 * MB), file('d.iso', 20 * MB)];
    r.add(fs);
    expect(states(r.u)).toEqual(['sending', 'sending', 'queued', 'queued']);
    await r.settled();
    expect(s.maxInFlight).toBeLessThanOrEqual(2);
    expect(states(r.u)).toEqual(['done', 'done', 'done', 'done']);
    expect(Math.max(...r.log.map((l) => l.filter((i) => i.state === 'sending' || i.state === 'finishing').length))).toBe(2);
  });

  it('cancel deletes the upload and starts the next file', async () => {
    const s = fake();
    const r = rig(s);
    r.add([file('a.iso', 400 * MB), file('b.iso', 8 * MB), file('c.iso', 8 * MB)]);
    await new Promise((x) => setTimeout(x, 5));
    r.u.cancel(r.u.items()[0].key);
    await r.settled();
    expect(states(r.u)).toEqual(['cancelled', 'done', 'done']);
    expect(s.aborted).toEqual(['u1']);
    expect(s.finished).not.toContain('u1');
  });

  it('cancelling a queued file never creates it', async () => {
    const s = fake();
    const r = rig(s, { concurrency: 1 });
    r.add([file('a.iso', 8 * MB), file('b.iso', 8 * MB)]);
    r.u.cancel(r.u.items()[1].key);
    await r.settled();
    expect(s.creates).toEqual(['a.iso']);
    expect(states(r.u)).toEqual(['done', 'cancelled']);
  });

  it('fails at once on a refusal that retrying cannot fix', async () => {
    const full = new ApiError('x', 507);
    const s = fake({ createFail: full });
    const r = rig(s);
    r.add([file('a.iso', 8 * MB)]);
    await r.settled();
    const it = r.u.items()[0];
    expect([it.state, it.error]).toEqual(['failed', 'This machine keeps 5 GB free, so there is no room for this file.']);
    expect(r.sleeps).toEqual([]);
  });

  it('frees the slot after a refusal, but keeps the upload when there was no room', async () => {
    const s = fake({ finish: new ApiError('bad', 422) });
    const r = rig(s);
    r.add([file('a.iso', 8 * MB)]);
    await r.settled();
    expect(s.aborted).toEqual(['u1']);
    const s2 = fake({ putFail: { 1: new ApiError('full', 507) } });
    const r2 = rig(s2);
    r2.add([file('a.iso', 8 * MB)]);
    await r2.settled();
    expect(s2.aborted).toEqual([]);
    r2.u.cancel(r2.u.items()[0].key);
    expect([s2.aborted, r2.u.items()[0].state]).toEqual([['u1'], 'cancelled']);
  });

  it('a damaged file at finish is a failure and the sent bytes are not sent again on retry', async () => {
    const s = fake({ finish: new ApiError('bad', 422) });
    const r = rig(s);
    r.add([file('a.iso', 8 * MB)]);
    await r.settled();
    expect(r.u.items()[0].error).toBe('This file looks damaged.');
  });

  it('says so when the file is stored but nothing can play it', async () => {
    const s = fake({ finish: { system: 'gba', registered: false, duplicate: false, game: null } });
    const r = rig(s);
    r.add([file('a.gba', 8 * MB)]);
    await r.settled();
    const it = r.u.items()[0];
    expect([it.state, it.note]).toEqual(['done', 'Stored, but no emulator can play it yet.']);
  });

  it('says so when the game was already there', async () => {
    const s = fake({ finish: { system: 'wii', registered: true, duplicate: true, game: null } });
    const r = rig(s);
    r.add([file('a.iso', 8 * MB)]);
    await r.settled();
    expect(r.u.items()[0].note).toBe('This game was already in the library.');
  });

  it('reports speed and time left from the bytes sent', async () => {
    let t = 0;
    const s = fake();
    const r = rig(s, { now: () => (t += 1000) });
    r.add([file('a.iso', 32 * MB)]);
    await r.settled();
    const seen = r.log.flat().filter((i) => i.state === 'sending' && i.speed > 0);
    expect(seen.length).toBeGreaterThan(0);
    expect(seen[0].speed).toBeGreaterThan(0);
    expect(seen[0].etaS).not.toBeNull();
    expect(r.u.items()[0].etaS).toBeNull();
  });

  it('onDone fires once per finished file', async () => {
    const s = fake();
    const got: string[] = [];
    const r = rig(s, { onDone: (i: Item) => got.push(i.name) });
    r.add([file('a.iso', 8 * MB), file('b.iso', 8 * MB)]);
    await r.settled();
    expect(got.sort()).toEqual(['a.iso', 'b.iso']);
  });

  it('clear drops finished and failed rows only', async () => {
    const s = fake({ createFail: undefined });
    const r = rig(s, { concurrency: 1 });
    r.add([file('a.iso', 8 * MB)]);
    await r.settled();
    r.u.clear();
    expect(r.u.items()).toEqual([]);
  });
});

describe('uploadError', () => {
  it('maps statuses to plain words', () => {
    const t = (n: number) => uploadError(new ApiError('raw', n));
    expect(t(507)).toBe('This machine keeps 5 GB free, so there is no room for this file.');
    expect(t(415)).toBe('This file type is not supported.');
    expect(t(422)).toBe('This file looks damaged.');
    expect(t(429)).toBe('This machine is already taking two uploads. Try again in a moment.');
    expect(t(404)).toBe('The machine lost track of this upload. Try again.');
    expect(t(401)).toBe('Brisk is not signed in to this machine.');
    expect(t(403)).toBe('Brisk is not signed in to this machine.');
    expect(uploadError(new ApiError('Something odd', 500))).toBe('Something odd');
    expect(uploadError(new TypeError('Failed to fetch'))).toBe('The connection keeps dropping. Check the network and try again.');
  });
});

describe('scanRows', () => {
  it('lists the counts in plain words and leaves out zeros but added', () => {
    const r = {
      scanned: 9,
      added: [{}, {}],
      aliased: 1,
      duplicates: 2,
      unplayable: 0,
      removed: 1,
      unknown: 1,
      broken: 0,
      truncated: false,
    };
    expect(scanRows(r as never)).toEqual([
      ['Added', 2],
      ['Already there', 3],
      ['Skipped, you removed them', 1],
      ['Not recognised', 1],
    ]);
    expect(scanRows({ ...r, added: [], aliased: 0, duplicates: 0, removed: 0, unknown: 0 } as never)).toEqual([['Added', 0]]);
  });
});

describe('formatting', () => {
  it('speed and time left', () => {
    expect([fmtSpeed(12.34e6), fmtSpeed(120e6)]).toEqual(['12.3 MB/s', '120 MB/s']);
    expect([fmtEta(0.2), fmtEta(40), fmtEta(89), fmtEta(300)]).toEqual(['1 s left', '40 s left', '89 s left', '5 min left']);
  });
});
