import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

function memStorage() {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), m };
}

async function fresh() {
  vi.resetModules();
  return (await import('./settings')).settings;
}

afterEach(() => vi.unstubAllGlobals());

describe('settings', () => {
  it('defaults_are_mint_balanced', async () => {
    vi.stubGlobal('localStorage', memStorage());
    expect(get(await fresh())).toEqual({ accent: 'mint', sounds: true, shaders: true, defaultPreset: 'balanced' });
  });

  it('survives_throwing_localstorage', async () => {
    const boom = () => {
      throw new Error('denied');
    };
    vi.stubGlobal('localStorage', { getItem: boom, setItem: boom });
    const s = await fresh();
    expect(get(s).accent).toBe('mint');
    expect(() => s.update((v) => ({ ...v, sounds: false }))).not.toThrow();
    expect(get(s).sounds).toBe(false);
  });

  it('persists_changes', async () => {
    const ls = memStorage();
    vi.stubGlobal('localStorage', ls);
    (await fresh()).update((v) => ({ ...v, accent: 'pink', defaultPreset: 'crisp' }));
    const again = get(await fresh());
    expect(again.accent).toBe('pink');
    expect(again.defaultPreset).toBe('crisp');
    expect(again.shaders).toBe(true);
  });
});
