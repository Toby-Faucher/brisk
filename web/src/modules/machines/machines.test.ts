import { describe, expect, it } from 'vitest';
import { ApiError, type HostInfo } from '../../state/api';
import { get } from 'svelte/store';
import { afterEach, vi } from 'vitest';
import { applyAccent, bindAccent } from '../settings/accent';
import { machinesFrom, restartError } from './machines';

const info: HostInfo = { name: 'bawn', sunshine: 'ready', uptimeS: 5, cpuTempC: null, gpuTempC: null, path: null, relay: null };

describe('machines', () => {
  it('machinesFrom_returns_single_bawn_entry', () => {
    const m = machinesFrom(info);
    expect(m).toHaveLength(1);
    expect(m[0]).toEqual({ id: 'bawn', info });
  });
  it('machinesFrom_falls_back_to_bawn_and_lowercases', () => {
    expect(machinesFrom({ ...info, name: '' })[0].id).toBe('bawn');
    expect(machinesFrom({ ...info, name: 'BaWn' })[0].id).toBe('bawn');
  });
  it('restart_429_message', () => {
    expect(restartError(new ApiError('slow down', 429))).toBe('Sunshine was restarted a moment ago, try again in a minute');
    expect(restartError(new ApiError('boom', 500))).toContain('boom');
  });
  it('accent_change_updates_css_variable', async () => {
    const mem = new Map<string, string>();
    vi.stubGlobal('localStorage', { getItem: (k: string) => mem.get(k) ?? null, setItem: (k: string, v: string) => void mem.set(k, v) });
    vi.resetModules();
    const { settings, SETTINGS_KEY } = await import('../../state/settings');
    const root = { dataset: {} as DOMStringMap };
    const off = bindAccent(settings, root);
    expect(root.dataset.accent).toBe('mint');
    settings.update((s) => ({ ...s, accent: 'pink' }));
    expect(root.dataset.accent).toBe('pink');
    expect(JSON.parse(mem.get(SETTINGS_KEY)!).accent).toBe('pink');
    settings.update((s) => ({ ...s, accent: 'nope' as never }));
    expect(root.dataset.accent).toBe('mint');
    off();
    settings.update((s) => ({ ...s, accent: 'sky' }));
    expect(root.dataset.accent).toBe('mint');
    expect(get(settings).accent).toBe('sky');
  });
  it('stored_invalid_accent_falls_back_to_mint', async () => {
    vi.stubGlobal('localStorage', { getItem: () => '{"accent":"zzz"}', setItem() {} });
    vi.resetModules();
    expect(get((await import('../../state/settings')).settings).accent).toBe('mint');
    applyAccent(root0, 'zzz');
    expect(root0.dataset.accent).toBe('mint');
  });
});
const root0 = { dataset: {} as DOMStringMap };
afterEach(() => vi.unstubAllGlobals());
