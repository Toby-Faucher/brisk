import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { api, captureSecret, denied } from './api';

function stubPage(href: string) {
  const m = new Map<string, string>();
  const replaceState = vi.fn();
  vi.stubGlobal('location', { href });
  vi.stubGlobal('history', { state: null, replaceState });
  vi.stubGlobal('sessionStorage', { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) });
  return { m, replaceState };
}

function stubFetch(status = 200, body: unknown = {}) {
  const f = vi.fn(async () => new Response(status === 204 ? null : JSON.stringify(body), { status }));
  vi.stubGlobal('fetch', f);
  return f;
}

afterEach(() => vi.unstubAllGlobals());

describe('api', () => {
  it('captures_secret_from_url_and_strips_it', () => {
    const { m, replaceState } = stubPage('http://localhost:8443/library?s=abc123&x=1#top');
    captureSecret();
    expect(m.get('brisk.secret')).toBe('abc123');
    expect(String(replaceState.mock.calls[0][2])).toBe('http://localhost:8443/library?x=1#top');
  });

  it('sends_secret_on_every_call', async () => {
    const { m } = stubPage('http://localhost:8443/');
    m.set('brisk.secret', 'sek');
    const f = stubFetch(204);
    await api.launch(570);
    await api.games().catch(() => {});
    const [postUrl, post] = f.mock.calls[0] as unknown as [string, RequestInit];
    const [getUrl, getInit] = f.mock.calls[1] as unknown as [string, RequestInit];
    expect(postUrl).toBe('/api/launch');
    expect(post.method).toBe('POST');
    expect((post.headers as Record<string, string>)['x-brisk-secret']).toBe('sek');
    expect(post.body).toBe('{"appid":570}');
    expect(getUrl).toBe('/api/games');
    expect((getInit.headers as Record<string, string>)['x-brisk-secret']).toBe('sek');
  });

  it('keeps_a_captured_secret_in_localstorage_for_bookmarks', async () => {
    const { m } = stubPage('http://localhost:8443/?s=abc123');
    const local = new Map<string, string>();
    vi.stubGlobal('localStorage', {
      getItem: (k: string) => local.get(k) ?? null,
      setItem: (k: string, v: string) => void local.set(k, v),
    });
    captureSecret();
    expect(local.get('brisk.secret')).toBe('abc123');
    m.clear(); // a new tab opened from a bookmark of the bare URL
    const f = stubFetch(204);
    await api.launch(1);
    expect(((f.mock.calls[0] as unknown as [string, RequestInit])[1].headers as Record<string, string>)['x-brisk-secret']).toBe('abc123');
  });

  it('putLayout_carries_machine_colours_and_keeps_the_array_form', async () => {
    stubPage('https://bawn.tail0.ts.net:8443/');
    const f = stubFetch(200, { homeOrder: ['1'], machineColors: { a: '#112233' } });
    const r: { machineColors?: Record<string, string> } = await api.putLayout({ machineColors: { a: '#112233', b: null } });
    expect(r.machineColors).toEqual({ a: '#112233' });
    expect(JSON.parse((f.mock.calls[0] as unknown as [string, RequestInit])[1].body as string)).toEqual({
      machineColors: { a: '#112233', b: null },
    });
    await api.putLayout(['1']);
    expect(JSON.parse((f.mock.calls[1] as unknown as [string, RequestInit])[1].body as string)).toEqual({ homeOrder: ['1'] });
  });

  it('works_without_a_secret_and_survives_broken_storage', async () => {
    stubPage('https://bawn.tail0.ts.net:8443/');
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('denied');
      },
    });
    const f = stubFetch(200, { name: 'bawn' });
    await expect(api.host()).resolves.toMatchObject({ name: 'bawn' });
    const h = (f.mock.calls[0] as unknown as [string, RequestInit])[1].headers as Record<string, string>;
    expect(h['x-brisk-secret']).toBeUndefined();
    expect(h['x-brisk-csrf']).toBe('1');
  });

  it('flags_403_as_denied_until_a_call_succeeds', async () => {
    stubPage('https://bawn.tail0.ts.net:8443/');
    stubFetch(403);
    await expect(api.games()).rejects.toMatchObject({ status: 403 });
    expect(get(denied)).toBe(true);
    stubFetch(200, { games: [], ownedAvailable: true });
    await api.games();
    expect(get(denied)).toBe(false);
  });

  it('maps_error_json_to_thrown_message', async () => {
    stubPage('http://localhost:8443/');
    stubFetch(429, { error: 'restarted a moment ago' });
    await expect(api.restartSunshine()).rejects.toMatchObject({ message: 'restarted a moment ago', status: 429 });
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('<html>', { status: 502, statusText: 'Bad Gateway' })),
    );
    await expect(api.host()).rejects.toThrow('502');
  });
});

describe('route', () => {
  it('scopes the emulator controls path to the selected machine', async () => {
    const { route, setRoute } = await import('./api');
    setRoute('m1');
    for (const p of ['/api/launch/defaults', '/api/games/steam%3A620/launch']) expect(route(p)).toBe(`/api/m/m1/${p.slice(5)}`);
    expect(route('/api/emulators/dolphin/controls/classic')).toBe('/api/m/m1/emulators/dolphin/controls/classic');
    expect(route('/api/prefs')).toBe('/api/prefs');
    setRoute(null);
    expect(route('/api/emulators/dolphin/controls/classic')).toBe('/api/emulators/dolphin/controls/classic');
  });
});
