import { describe, expect, it } from 'vitest';
import { ApiError, type HostInfo } from '../../state/api';
import { get } from 'svelte/store';
import { afterEach, vi } from 'vitest';
import { applyAccent, bindAccent } from '../settings/accent';
import { dotState, elapsedS, machinesFrom, powerWhy, restartError, statRows, STATE_LABEL } from './machines';

const info: HostInfo = { name: 'bawn', sunshine: 'ready', uptimeS: 5, cpuTempC: null, gpuTempC: null, path: null, relay: null };

describe('machines', () => {
  it('machinesFrom_returns_single_bawn_entry', () => {
    const m = machinesFrom(info);
    expect(m).toHaveLength(1);
    expect(m[0]).toEqual({ id: 'bawn', info });
  });
  it('machinesFrom_falls_back_to_host_and_lowercases', () => {
    expect(machinesFrom({ ...info, name: '' })[0].id).toBe('host');
    expect(machinesFrom({ ...info, name: 'BaWn' })[0].id).toBe('bawn');
    expect(machinesFrom({ ...info, name: '  BaWn ' })[0].id).toBe('bawn');
  });
  it('restart_429_message', () => {
    expect(restartError(new ApiError('slow down', 429))).toBe('Sunshine was restarted a moment ago, try again in a minute');
    expect(restartError(new ApiError('boom', 500))).toContain('boom');
  });
  it('hub_labels_and_dots', () => {
    expect(STATE_LABEL.unreachable).toBe('Not reachable');
    expect(dotState({ state: 'ready', sunshine: 'busy' })).toBe('busy');
    expect(dotState({ state: 'ready', sunshine: null })).toBe('ready');
    expect(dotState({ state: 'starting', sunshine: null })).toBe('busy');
    expect(dotState({ state: 'unreachable', sunshine: null })).toBe('down');
    expect(dotState({ state: 'off', sunshine: null })).toBe('off');
    expect(powerWhy('starting')).toContain('Starting');
    expect(powerWhy('ready')).toBeNull();
    expect(elapsedS(1000, 4500)).toBe(3);
    expect(elapsedS(5000, 4500)).toBe(0);
  });
  it('statRows_single_host_keeps_fixed_lines', () => {
    expect(statRows(info, null, false).map((r) => [r.label, r.text])).toEqual([['GPU', '–'], ['CPU', '–']]);
  });
  it('statRows_hub_shows_only_what_exists', () => {
    const vm = { cpuPct: 41.6, memUsedBytes: 8 * 1024 ** 3, memTotalBytes: 16 * 1024 ** 3 };
    expect(statRows(info, null, true)).toEqual([]);
    expect(statRows({ gpuTempC: 60.4, cpuTempC: null }, null, true).map((r) => [r.label, r.text])).toEqual([['GPU', '60 °C']]);
    expect(statRows({ gpuTempC: null, cpuTempC: 50 }, vm, true).map((r) => [r.label, r.text])).toEqual([['CPU', '50 °C'], ['RAM', '8.0 GB / 16.0 GB']]);
    expect(statRows(info, vm, true).map((r) => [r.label, r.text])).toEqual([['CPU load', '42 %'], ['RAM', '8.0 GB / 16.0 GB']]);
    expect(statRows(info, { ...vm, memTotalBytes: 0 }, true)[1].text).toBe('–');
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
    for (const bad of ['zzz', 'toString', 'constructor']) {
      vi.stubGlobal('localStorage', { getItem: () => JSON.stringify({ accent: bad }), setItem() {} });
      vi.resetModules();
      expect(get((await import('../../state/settings')).settings).accent).toBe('mint');
      applyAccent(root0, bad);
      expect(root0.dataset.accent).toBe('mint');
    }
    applyAccent(root0, 'zzz');
    expect(root0.dataset.accent).toBe('mint');
  });
});
const root0 = { dataset: {} as DOMStringMap };
afterEach(() => vi.unstubAllGlobals());
