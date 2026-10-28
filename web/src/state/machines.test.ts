import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

type M = { id: string; state: string };
const mk = (id: string, state = 'ready') => ({ id, name: id, state, canPower: true, sunshine: null, host: null });

/** Fresh module graph per test (the stores are module state), a stubbed hub and in-memory localStorage. */
async function setup(opts: { list?: M[]; stored?: string; machinesStatus?: number } = {}) {
  vi.resetModules();
  const local = new Map<string, string>(opts.stored ? [['brisk.machine', opts.stored]] : []);
  vi.stubGlobal('localStorage', { getItem: (k: string) => local.get(k) ?? null, setItem: (k: string, v: string) => void local.set(k, v), removeItem: (k: string) => void local.delete(k) });
  vi.stubGlobal('sessionStorage', { getItem: () => null, setItem() {} });
  vi.stubGlobal('location', { href: 'http://localhost/' });
  const state = { list: opts.list ?? [mk('a'), mk('b')], status: opts.machinesStatus ?? 200, fail: false };
  const f = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/machines') {
      if (state.fail) throw new TypeError('network');
      return new Response(JSON.stringify(state.status === 200 ? { machines: state.list } : { error: 'nope' }), { status: state.status });
    }
    if (url.endsWith('/power')) return new Response('{"state":"starting"}', { status: 202 });
    return new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } });
  });
  vi.stubGlobal('fetch', f);
  const m = await import('./machines');
  const a = await import('./api');
  return { m, a, f, state, local };
}

const calls = (f: ReturnType<typeof vi.fn>, re: RegExp) => f.mock.calls.filter((c) => re.test(String(c[0]))).length;

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('routing', () => {
  it('routes_machine_scoped_paths_only_in_hub_mode', async () => {
    const { m, a, f } = await setup();
    await a.api.host().catch(() => {});
    expect(f.mock.calls.at(-1)![0]).toBe('/api/host'); // before the probe: untouched
    const stop = m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.hub)).toBe(true);
    const routed = ['/api/host', '/api/games', '/api/games/5/info?refresh=1', '/api/friends', '/api/stream', '/api/launch', '/api/install', '/api/quit', '/api/sessions', '/api/sessions/9/series', '/api/sunshine/restart', '/api/clipboard?since=3', '/api/files/x'];
    for (const p of routed) expect(a.route(p)).toBe(p.replace('/api/', '/api/m/a/'));
    for (const p of ['/api/prefs', '/api/prefs/5', '/api/notes/tags/x', '/api/machines', '/api/machines/a/power', '/api/gamesx', '/api/other']) expect(a.route(p)).toBe(p);
    await a.api.gameInfo(5, true);
    expect(f.mock.calls.at(-1)![0]).toBe('/api/m/a/games/5/info?refresh=1');
    await a.api.prefs();
    expect(f.mock.calls.at(-1)![0]).toBe('/api/prefs');
    await a.api.power('b', 'start');
    expect(f.mock.calls.at(-1)![0]).toBe('/api/machines/b/power');
    stop();
  });

  it('leaves_paths_alone_without_a_hub', async () => {
    const { m, a } = await setup({ machinesStatus: 404 });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.hub)).toBe(false);
    expect(a.route('/api/host')).toBe('/api/host');
  });

  it('503_machine_is_off_is_an_error_not_denied', async () => {
    const { a, f } = await setup();
    f.mockImplementationOnce(async () => new Response('{"error":"machine is off"}', { status: 503 }));
    await expect(a.api.host()).rejects.toMatchObject({ status: 503, message: 'machine is off' });
    expect(get(a.denied)).toBe(false);
  });
});

describe('hub detection and polling', () => {
  it('stays_unknown_on_a_network_error_and_retries', async () => {
    const { m, state } = await setup();
    state.fail = true;
    const stop = m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.hub)).toBeNull();
    state.fail = false;
    await vi.advanceTimersByTimeAsync(5_000);
    expect(get(m.hub)).toBe(true);
    stop();
  });

  it('polls_every_5s_and_every_1_5s_while_starting_and_stops', async () => {
    const { m, f, state } = await setup();
    const stop = m.startMachines();
    const stop2 = m.startMachines(); // ref-counted: one loop
    await vi.advanceTimersByTimeAsync(0);
    expect(calls(f, /^\/api\/machines$/)).toBe(1);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(calls(f, /^\/api\/machines$/)).toBe(2);
    state.list = [mk('a', 'starting'), mk('b')];
    await vi.advanceTimersByTimeAsync(5_000);
    expect(calls(f, /^\/api\/machines$/)).toBe(3);
    await vi.advanceTimersByTimeAsync(3_000);
    expect(calls(f, /^\/api\/machines$/)).toBe(5);
    stop();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(calls(f, /^\/api\/machines$/)).toBeGreaterThan(5); // still one user
    stop2();
    const n = calls(f, /^\/api\/machines$/);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(calls(f, /^\/api\/machines$/)).toBe(n);
  });
});

describe('selection', () => {
  it('persists_and_falls_back_to_the_first_machine', async () => {
    const { m, local } = await setup({ stored: 'zzz' });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.selected)).toBe('a');
    expect(local.get('brisk.machine')).toBe('a');
    m.selected.set('b');
    expect(local.get('brisk.machine')).toBe('b');
    expect(get(m.selectedMachine)?.id).toBe('b');
  });

  it('keeps_a_known_stored_id_and_survives_broken_storage', async () => {
    const { m } = await setup({ stored: 'b' });
    vi.stubGlobal('localStorage', { getItem: () => { throw new Error('x'); }, setItem: () => { throw new Error('x'); }, removeItem: () => { throw new Error('x'); } });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.selected)).toBe('b');
    m.selected.set('a');
    expect(get(m.selected)).toBe('a');
  });
});

describe('ensureReady', () => {
  it('resolves_at_once_when_ready_without_touching_wake', async () => {
    const { m, f } = await setup();
    await m.ensureReady('a');
    expect(get(m.wake)).toBeNull();
    expect(calls(f, /power/)).toBe(0);
  });

  it('starts_an_off_machine_and_waits_for_ready', async () => {
    const { m, f, state } = await setup({ list: [mk('a', 'off')] });
    const p = m.ensureReady('a');
    await vi.advanceTimersByTimeAsync(0);
    expect(calls(f, /\/api\/machines\/a\/power/)).toBe(1);
    expect(JSON.parse((f.mock.calls.find((c) => /power/.test(String(c[0])))![1] as RequestInit).body as string)).toEqual({ action: 'start' });
    expect(get(m.wake)).toMatchObject({ id: 'a', phase: 'waiting' });
    state.list = [mk('a', 'starting')];
    await vi.advanceTimersByTimeAsync(1_500);
    expect(get(m.wake)).toMatchObject({ phase: 'waiting' });
    state.list = [mk('a')];
    await vi.advanceTimersByTimeAsync(1_500);
    await p;
    expect(get(m.wake)).toBeNull();
  });

  it('does_not_start_a_machine_that_is_already_starting', async () => {
    const { m, f, state } = await setup({ list: [mk('a', 'starting')] });
    const p = m.ensureReady('a');
    await vi.advanceTimersByTimeAsync(0);
    state.list = [mk('a')];
    await vi.advanceTimersByTimeAsync(1_500);
    await p;
    expect(calls(f, /power/)).toBe(0);
  });

  it('shares_one_promise_between_concurrent_calls', async () => {
    const { m, f, state } = await setup({ list: [mk('a', 'off')] });
    const p1 = m.ensureReady('a');
    const p2 = m.ensureReady('a');
    expect(p1).toBe(p2);
    await vi.advanceTimersByTimeAsync(0);
    state.list = [mk('a')];
    await vi.advanceTimersByTimeAsync(1_500);
    await Promise.all([p1, p2]);
    expect(calls(f, /power/)).toBe(1);
  });

  it('fails_when_proxmox_refuses', async () => {
    const { m, f } = await setup({ list: [mk('a', 'off')] });
    f.mockImplementation(async (url: string) => (String(url).endsWith('/power') ? new Response('{"error":"proxmox said no"}', { status: 502 }) : new Response('{"machines":[{"id":"a","name":"a","state":"off","canPower":true,"sunshine":null,"host":null}]}')));
    await expect(m.ensureReady('a')).rejects.toMatchObject({ message: 'proxmox said no' });
    expect(get(m.wake)).toMatchObject({ phase: 'failed', error: 'proxmox said no' });
  });

  it('fails_after_30s_unreachable', async () => {
    const { m } = await setup({ list: [mk('a', 'unreachable')] });
    const p = m.ensureReady('a');
    const seen = p.catch((e: Error) => e);
    await vi.advanceTimersByTimeAsync(33_000);
    expect(await seen).toMatchObject({ message: 'machine is unreachable' });
    expect(get(m.wake)).toMatchObject({ phase: 'failed' });
  });

  it('fails_after_5_minutes', async () => {
    const { m } = await setup({ list: [mk('a', 'starting')] });
    const seen = m.ensureReady('a').catch((e: Error) => e);
    await vi.advanceTimersByTimeAsync(5 * 60_000 + 2_000);
    expect(await seen).toMatchObject({ message: 'machine did not start in time' });
    expect(get(m.wake)).toMatchObject({ id: 'a', phase: 'failed', error: 'machine did not start in time' });
  });
});

describe('actions', () => {
  it('selecting_an_off_machine_wakes_it_and_an_on_one_does_not', async () => {
    const { m, f, state } = await setup({ list: [mk('a'), mk('b', 'off')] });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    await m.selectMachine('a');
    expect(calls(f, /power/)).toBe(0);
    const p = m.selectMachine('b');
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.selected)).toBe('b');
    expect(calls(f, /\/api\/machines\/b\/power/)).toBe(1);
    state.list = [mk('a'), mk('b')];
    await vi.advanceTimersByTimeAsync(1_500);
    await p;
  });

  it('power_errors_propagate', async () => {
    const { m, f } = await setup();
    f.mockImplementationOnce(async () => new Response('{"error":"no power control"}', { status: 409 }));
    await expect(m.powerMachine('a', 'shutdown')).rejects.toMatchObject({ status: 409, message: 'no power control' });
  });
});
