import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

type M = { id: string; state: string; rttMs?: number | null };
const mk = (id: string, state = 'ready') => ({
  id,
  name: id,
  state,
  canPower: true,
  sunshine: null,
  host: null,
  url: `http://${id}.test:8443`,
  rttMs: 5,
  vm: null,
});

/** hostd's answer to a layout PUT: the cleaned order, or the colours as stored (a null removes one). */
const layoutAnswer = (b: { machinesOrder?: string[]; machineColors?: Record<string, string | null> }) =>
  b.machineColors
    ? { homeOrder: [], machineColors: Object.fromEntries(Object.entries(b.machineColors).flatMap(([k, v]) => (v ? [[k, v]] : []))) }
    : { homeOrder: [], machinesOrder: b.machinesOrder!.filter((i) => i !== 'bad') };

/** Fresh module graph per test (the stores are module state), a stubbed hub and in-memory localStorage. */
async function setup(opts: { list?: M[]; stored?: string; machinesStatus?: number } = {}) {
  vi.resetModules();
  const local = new Map<string, string>(opts.stored ? [['brisk.machine', opts.stored]] : []);
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => local.get(k) ?? null,
    setItem: (k: string, v: string) => void local.set(k, v),
    removeItem: (k: string) => void local.delete(k),
  });
  vi.stubGlobal('sessionStorage', { getItem: () => null, setItem() {} });
  vi.stubGlobal('location', { href: 'http://localhost/' });
  const state = {
    list: opts.list ?? [mk('a'), mk('b')],
    status: opts.machinesStatus ?? 200,
    fail: false,
    order: [] as string[],
    colors: undefined as unknown,
    putFail: false,
    ping: 'ok' as 'ok' | 'fail' | 'hang',
    pingMs: 20,
  };
  const f = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/machines') {
      if (state.fail) throw new TypeError('network');
      return new Response(JSON.stringify(state.status === 200 ? { machines: state.list } : { error: 'nope' }), { status: state.status });
    }
    if (url === '/api/prefs')
      return new Response(JSON.stringify({ prefs: {}, machinesOrder: state.order, machineColors: state.colors }), { status: 200 });
    if (url === '/api/prefs/layout')
      return state.putFail
        ? new Response('{"error":"nope"}', { status: 500 })
        : new Response(JSON.stringify(layoutAnswer(JSON.parse(init!.body as string))), { status: 200 });
    if (url.startsWith('http')) {
      if (state.ping === 'fail') throw new TypeError('network');
      await new Promise<void>((ok, no) => {
        init?.signal?.addEventListener('abort', () => no(new DOMException('aborted', 'AbortError')));
        if (state.ping === 'ok') setTimeout(ok, state.pingMs);
      });
      return new Response(null, { status: 200 });
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
    const early = a.api.host(); // before the probe the route is unknown: held, never sent to the hub's own /api/host
    await vi.advanceTimersByTimeAsync(0);
    expect(calls(f, /\/api\/host/)).toBe(0);
    const stop = m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.hub)).toBe(true);
    await early;
    expect(f.mock.calls.at(-1)![0]).toBe('/api/m/a/host');
    const routed = [
      '/api/host',
      '/api/games',
      '/api/games/5/info?refresh=1',
      '/api/friends',
      '/api/stream',
      '/api/launch',
      '/api/install',
      '/api/quit',
      '/api/sessions',
      '/api/sessions/9/series',
      '/api/sunshine/restart',
      '/api/clipboard?since=3',
      '/api/files/x',
    ];
    for (const p of routed) expect(a.route(p)).toBe(p.replace('/api/', '/api/m/a/'));
    for (const p of [
      '/api/prefs',
      '/api/prefs/5',
      '/api/notes/tags/x',
      '/api/machines',
      '/api/machines/a/power',
      '/api/gamesx',
      '/api/other',
    ])
      expect(a.route(p)).toBe(p);
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
    a.setRoute(null);
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
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('x');
      },
      setItem: () => {
        throw new Error('x');
      },
      removeItem: () => {
        throw new Error('x');
      },
    });
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
    expect(JSON.parse((f.mock.calls.find((c) => /power/.test(String(c[0])))![1] as RequestInit).body as string)).toEqual({
      action: 'start',
    });
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
    f.mockImplementation(async (url: string) =>
      String(url).endsWith('/power')
        ? new Response('{"error":"proxmox said no"}', { status: 502 })
        : new Response(
            '{"machines":[{"id":"a","name":"a","state":"off","canPower":true,"sunshine":null,"host":null,"url":"http://a.test","rttMs":null,"vm":null}]}',
          ),
    );
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

describe('machine order', () => {
  it('sorts_by_the_saved_order_and_appends_unknown_machines', async () => {
    const { m, state } = await setup({ list: [mk('a'), mk('b'), mk('c'), mk('d')] });
    state.order = ['c', 'gone', 'a'];
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.machinesOrder)).toEqual(['c', 'gone', 'a']);
    expect(get(m.orderedMachines).map((x) => x.id)).toEqual(['c', 'a', 'b', 'd']);
  });

  it('keeps_server_order_without_a_saved_one', async () => {
    const { m } = await setup();
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    expect(get(m.orderedMachines).map((x) => x.id)).toEqual(['a', 'b']);
  });

  it('saves_optimistically_and_takes_the_cleaned_answer', async () => {
    const { m, f } = await setup();
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    const p = m.setMachinesOrder(['b', 'bad', 'a']);
    expect(get(m.machinesOrder)).toEqual(['b', 'bad', 'a']);
    await p;
    expect(get(m.machinesOrder)).toEqual(['b', 'a']);
    const put = f.mock.calls.find((c) => c[0] === '/api/prefs/layout')!;
    expect((put[1] as RequestInit).method).toBe('PUT');
    expect(JSON.parse((put[1] as RequestInit).body as string)).toEqual({ machinesOrder: ['b', 'bad', 'a'] });
  });

  it('reverts_and_rethrows_on_failure', async () => {
    const { m, state } = await setup();
    state.order = ['a', 'b'];
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    state.putFail = true;
    await expect(m.setMachinesOrder(['b', 'a'])).rejects.toMatchObject({ status: 500 });
    expect(get(m.machinesOrder)).toEqual(['a', 'b']);
  });
});

describe('machine colours', () => {
  const boot = async (o: Parameters<typeof setup>[0] = {}, colors?: unknown) => {
    const r = await setup({ list: [mk('a'), mk('b'), mk('c')], ...o });
    r.state.colors = colors;
    r.m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    return r;
  };
  const putBody = (f: ReturnType<typeof vi.fn>) =>
    JSON.parse((f.mock.calls.find((c) => c[0] === '/api/prefs/layout')![1] as RequestInit).body as string);

  it('hue_defaults_to_the_palette_by_server_position', async () => {
    const { m } = await boot();
    expect(get(m.machineColors)).toEqual({});
    expect(get(m.machineHue)).toEqual({ a: '#60a5fa', b: '#a78bfa', c: '#f472b6' });
  });

  it('hue_prefers_a_saved_colour_and_normalizes_it', async () => {
    const { m } = await boot({}, { b: '#ABC', c: 'nope', z: '#00ff00', d: 5 });
    expect(get(m.machineColors)).toEqual({ b: '#aabbcc', z: '#00ff00' });
    expect(get(m.machineHue)).toEqual({ a: '#60a5fa', b: '#aabbcc', c: '#f472b6' });
  });

  it('tolerates_absent_or_garbage_colours', async () => {
    for (const bad of [undefined, null, 'x', ['#fff']]) {
      const { m } = await boot({}, bad);
      expect(get(m.machineHue).a).toBe('#60a5fa');
    }
  });

  it('sets_optimistically_and_takes_the_answer', async () => {
    const { m, f } = await boot();
    const p = m.setMachineColor('b', '#FA0');
    expect(get(m.machineHue).b).toBe('#ffaa00');
    await p;
    expect(get(m.machineColors)).toEqual({ b: '#ffaa00' });
    expect(putBody(f)).toEqual({ machineColors: { b: '#ffaa00' } });
  });

  it('null_resets_to_the_default', async () => {
    const { m, f } = await boot({}, { b: '#112233' });
    await m.setMachineColor('b', null);
    expect(get(m.machineColors)).toEqual({});
    expect(get(m.machineHue).b).toBe('#a78bfa');
    expect(putBody(f)).toEqual({ machineColors: { b: null } });
  });

  it('rejects_an_invalid_colour_before_any_request', async () => {
    const { m, f } = await boot();
    await expect(m.setMachineColor('b', 'red')).rejects.toBeInstanceOf(Error);
    expect(calls(f, /layout/)).toBe(0);
    expect(get(m.machineColors)).toEqual({});
  });

  it('reverts_and_rethrows_on_failure', async () => {
    const { m, state } = await boot({}, { a: '#112233' });
    state.putFail = true;
    await expect(m.setMachineColor('b', '#445566')).rejects.toMatchObject({ status: 500 });
    expect(get(m.machineColors)).toEqual({ a: '#112233' });
  });
});

describe('forMachine', () => {
  it('always_goes_through_the_hub_path_whatever_is_selected', async () => {
    const { m, a, f } = await setup();
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    m.selected.set('b');
    await a.api.forMachine('a').games(true);
    expect(f.mock.calls.at(-1)![0]).toBe('/api/m/a/games?refresh=1');
    await a.api.forMachine('a').sessions();
    expect(f.mock.calls.at(-1)![0]).toBe('/api/m/a/sessions');
    await a.api.forMachine('a').host();
    expect(f.mock.calls.at(-1)![0]).toBe('/api/m/a/host');
  });

  it('works_without_a_hub_route_and_503_is_not_denied', async () => {
    const { a, f } = await setup();
    await a.api.forMachine('x y').host();
    expect(f.mock.calls.at(-1)![0]).toBe('/api/m/x%20y/host');
    f.mockImplementationOnce(async () => new Response('{"error":"machine is off"}', { status: 503 }));
    await expect(a.api.forMachine('a').games()).rejects.toMatchObject({ status: 503, message: 'machine is off' });
    expect(get(a.denied)).toBe(false);
  });
});

describe('pings', () => {
  function stubDoc() {
    const l = new Set<() => void>();
    const doc = {
      hidden: false,
      addEventListener: (_: string, f: () => void) => l.add(f),
      removeEventListener: (_: string, f: () => void) => l.delete(f),
    };
    vi.stubGlobal('document', doc);
    return { doc, fire: () => l.forEach((f) => f()), l };
  }
  const direct = (f: ReturnType<typeof vi.fn>) => f.mock.calls.filter((c) => String(c[0]).startsWith('http'));

  it('samples_every_3s_with_direct_timing_and_the_hub_rtt', async () => {
    stubDoc();
    const { m, f } = await setup({ list: [mk('a'), mk('b', 'starting')] });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    const stop = m.startPings();
    await vi.advanceTimersByTimeAsync(100);
    expect(get(m.pings).a).toEqual([{ t: expect.any(Number), direct: 20, hub: 5 }]);
    const [url, init] = direct(f)[0] as [string, RequestInit];
    expect(url).toBe('http://a.test:8443/api/ping');
    expect(init).toMatchObject({ method: 'HEAD', mode: 'no-cors', cache: 'no-store' });
    await vi.advanceTimersByTimeAsync(3_000);
    expect(get(m.pings).a).toHaveLength(2);
    expect(get(m.pings).b).toHaveLength(2);
    stop();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(get(m.pings).a).toHaveLength(2);
  });

  it('is_ref_counted', async () => {
    const { l } = stubDoc();
    const { m } = await setup();
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    const s1 = m.startPings();
    const s2 = m.startPings();
    expect(l.size).toBe(1);
    s1();
    s1();
    await vi.advanceTimersByTimeAsync(3_100);
    expect(get(m.pings).a.length).toBeGreaterThan(1);
    s2();
    expect(l.size).toBe(0);
  });

  it('times_out_after_5s_and_failures_give_null_without_overlap', async () => {
    stubDoc();
    const { m, f, state } = await setup({ list: [mk('a')] });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    state.ping = 'hang';
    const stop = m.startPings();
    await vi.advanceTimersByTimeAsync(3_100);
    expect(direct(f)).toHaveLength(1); // the 3 s tick skips: still in flight
    expect(get(m.pings).a ?? []).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(2_000);
    expect(get(m.pings).a).toEqual([{ t: expect.any(Number), direct: null, hub: 5 }]);
    state.ping = 'fail';
    await vi.advanceTimersByTimeAsync(3_000);
    expect(get(m.pings).a.at(-1)).toMatchObject({ direct: null });
    stop();
  });

  it('off_stopping_and_unreachable_machines_get_null_samples_without_a_request', async () => {
    stubDoc();
    const { m, f } = await setup({ list: [mk('a', 'off'), mk('b', 'stopping'), mk('c', 'unreachable')] });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    const stop = m.startPings();
    await vi.advanceTimersByTimeAsync(100);
    for (const id of ['a', 'b', 'c']) expect(get(m.pings)[id]).toEqual([{ t: expect.any(Number), direct: null, hub: null }]);
    expect(direct(f)).toHaveLength(0);
    stop();
  });

  it('keeps_60_samples_and_drops_vanished_machines', async () => {
    stubDoc();
    const { m, state } = await setup();
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    const stop = m.startPings();
    await vi.advanceTimersByTimeAsync(3_000 * 70);
    expect(get(m.pings).a).toHaveLength(60);
    state.list = [mk('a')];
    await vi.advanceTimersByTimeAsync(6_000);
    expect(Object.keys(get(m.pings))).toEqual(['a']);
    stop();
  });

  it('pauses_while_hidden_and_resumes_at_once', async () => {
    const { doc, fire } = stubDoc();
    const { m } = await setup({ list: [mk('a')] });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    const stop = m.startPings();
    await vi.advanceTimersByTimeAsync(100);
    expect(get(m.pings).a).toHaveLength(1);
    doc.hidden = true;
    fire();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(get(m.pings).a).toHaveLength(1);
    doc.hidden = false;
    fire();
    await vi.advanceTimersByTimeAsync(100);
    expect(get(m.pings).a).toHaveLength(2);
    stop();
  });

  it('does_nothing_without_a_hub', async () => {
    stubDoc();
    const { m, f } = await setup({ machinesStatus: 404 });
    m.startMachines();
    await vi.advanceTimersByTimeAsync(0);
    const stop = m.startPings();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(get(m.pings)).toEqual({});
    expect(direct(f)).toHaveLength(0);
    stop();
  });
});
