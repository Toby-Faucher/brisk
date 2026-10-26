import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, captureSecret } from './api';

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

  it('maps_error_json_to_thrown_message', async () => {
    stubPage('http://localhost:8443/');
    stubFetch(429, { error: 'restarted a moment ago' });
    await expect(api.restartSunshine()).rejects.toMatchObject({ message: 'restarted a moment ago', status: 429 });
    vi.stubGlobal('fetch', vi.fn(async () => new Response('<html>', { status: 502, statusText: 'Bad Gateway' })));
    await expect(api.host()).rejects.toThrow('502');
  });
});
