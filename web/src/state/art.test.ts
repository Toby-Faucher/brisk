import { afterEach, describe, expect, it, vi } from 'vitest';
import { api } from './api';
import { gameCaps } from './gameId';

afterEach(() => vi.unstubAllGlobals());

describe('cached art', () => {
  it('loads art with the auth headers and keeps one blob per url', async () => {
    vi.stubGlobal('sessionStorage', { getItem: () => 'sek', setItem: () => {} });
    const f = vi.fn(async () => new Response(new Blob([new Uint8Array([137, 80])]), { status: 200 }));
    vi.stubGlobal('fetch', f);
    const make = vi.fn(() => 'blob:one');
    vi.stubGlobal('URL', { createObjectURL: make });
    const path = '/api/games/emu%3Adolphin%3ARMCE01/art/cover?v=1';
    expect(await api.artImage(path)).toBe('blob:one');
    expect(await api.artImage(path)).toBe('blob:one');
    expect(f).toHaveBeenCalledTimes(1);
    const [url, init] = f.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe(path);
    expect((init.headers as Record<string, string>)['x-brisk-secret']).toBe('sek');
  });

  it('does not keep a failed load', async () => {
    const f = vi.fn(async () => new Response('{}', { status: 404 }));
    vi.stubGlobal('fetch', f);
    const path = '/api/games/emu%3Adolphin%3AX/art/hero?v=1';
    await expect(api.artImage(path)).rejects.toThrow();
    await expect(api.artImage(path)).rejects.toThrow();
    expect(f).toHaveBeenCalledTimes(2);
  });

  it('answers with the id form and the right body', async () => {
    const f = vi.fn(async () => new Response('{"state":"fetching"}', { status: 202 }));
    vi.stubGlobal('fetch', f);
    await api.artAnswer('emu:dolphin:RMCE01', { candidateId: 7 });
    await api.artRefresh('emu:dolphin:RMCE01');
    const [u1, i1] = f.mock.calls[0] as unknown as [string, RequestInit];
    expect([u1, i1.method, i1.body]).toEqual(['/api/art/emu%3Adolphin%3ARMCE01/match', 'POST', '{"candidateId":7}']);
    expect((f.mock.calls[1] as unknown as [string])[0]).toBe('/api/art/emu%3Adolphin%3ARMCE01/refresh');
  });

  it('an old hostd has no art controls', () => {
    expect(gameCaps({ appid: 0xe0000001, emulator: 'dolphin' }).art).toBe(false);
    expect(gameCaps({ appid: 620 }).art).toBe(false);
  });
});
