import { describe, expect, it } from 'vitest';
import { ApiError, type HostInfo } from '../../state/api';
import { get } from 'svelte/store';
import { afterEach, vi } from 'vitest';
import { applyAccent, bindAccent } from '../settings/accent';
import {
  chipData,
  gauges,
  headerVars,
  pingText,
  isTap,
  dimmed,
  dotState,
  elapsedS,
  machinesFrom,
  powerWhy,
  restartError,
  statRows,
  STATE_LABEL,
} from './machines';

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
    expect(statRows(info, null, false).map((r) => [r.label, r.text])).toEqual([
      ['GPU', '–'],
      ['CPU', '–'],
    ]);
  });
  it('statRows_hub_shows_only_what_exists', () => {
    const vm = { cpuPct: 41.6, memUsedBytes: 8 * 1024 ** 3, memTotalBytes: 16 * 1024 ** 3 };
    expect(statRows(info, null, true)).toEqual([]);
    expect(statRows({ gpuTempC: 60.4, cpuTempC: null }, null, true).map((r) => [r.label, r.text])).toEqual([['GPU', '60 °C']]);
    expect(statRows({ gpuTempC: null, cpuTempC: 50 }, vm, true).map((r) => [r.label, r.text])).toEqual([
      ['CPU', '50 °C'],
      ['RAM', '8.0 GB / 16.0 GB'],
    ]);
    expect(statRows(info, vm, true).map((r) => [r.label, r.text])).toEqual([
      ['CPU load', '42 %'],
      ['RAM', '8.0 GB / 16.0 GB'],
    ]);
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

describe('card click and swap helpers', () => {
  it('isTap_allows_a_few_px_of_jitter_only', () => {
    expect(isTap({ x: 10, y: 10 }, { x: 13, y: 14 })).toBe(true);
    expect(isTap({ x: 10, y: 10 }, { x: 10, y: 16 })).toBe(false);
    expect(isTap(null, { x: 0, y: 0 })).toBe(false);
  });
  it('dimmed_only_for_other_cards_in_hub_mode_with_several_machines', () => {
    expect(dimmed(true, 2, 'a', 'b')).toBe(true);
    expect(dimmed(true, 2, 'a', 'a')).toBe(false);
    expect(dimmed(true, 1, 'a', 'b')).toBe(false);
    expect(dimmed(false, 2, 'a', 'b')).toBe(false);
    expect(dimmed(true, 2, null, 'b')).toBe(false);
  });
});

describe('card header helpers', () => {
  const G = 1024 ** 3;
  const vm = { cpuPct: 3.4, memUsedBytes: 16 * G, memTotalBytes: 15.6 * G };
  const h = (cpuTempC: number | null, gpuTempC: number | null) => ({ cpuTempC, gpuTempC });
  it('gauges_none_without_temperature_or_vm', () => {
    expect(gauges(h(null, null), null)).toEqual([]);
    expect(gauges(null, null)).toEqual([]);
  });
  it('gauges_cpu_temp_wins_over_vm_load_and_has_no_gpu_without_a_temp', () => {
    expect(gauges(h(38.4, null), vm).map((g) => [g.key, g.text, g.level])).toEqual([
      ['cpu', '38°', 'ok'],
      ['ram', '16G', 'static'],
    ]);
  });
  it('gauges_vm_load_when_no_cpu_temp', () => {
    const g = gauges(h(null, null), vm);
    expect(g[0]).toEqual({ key: 'cpu', label: 'CPU', pct: 3.4, text: '3%', level: 'ok' });
    expect(g[1]).toMatchObject({ key: 'ram', label: 'RAM', pct: 100, level: 'static' });
  });
  it('gauges_ram_only_on_a_vm_and_total_missing_is_a_dash', () => {
    expect(gauges(h(40, null), null).map((g) => g.key)).toEqual(['cpu']);
    expect(gauges(h(null, null), { ...vm, memTotalBytes: 0 })[1].text).toBe('–');
  });
  it('gauges_thresholds_on_both_sides', () => {
    const lv = (t: number, k: 'cpu' | 'gpu') => gauges(k === 'cpu' ? h(t, null) : h(null, t), null).find((g) => g.key === k)!.level;
    expect([lv(69.9, 'cpu'), lv(70, 'cpu'), lv(84.9, 'cpu'), lv(85, 'cpu')]).toEqual(['ok', 'warn', 'warn', 'bad']);
    expect([lv(74.9, 'gpu'), lv(75, 'gpu'), lv(87.9, 'gpu'), lv(88, 'gpu')]).toEqual(['ok', 'warn', 'warn', 'bad']);
    const load = (p: number) => gauges(h(null, null), { ...vm, cpuPct: p })[0].level;
    expect([load(69), load(70), load(89), load(90)]).toEqual(['ok', 'warn', 'warn', 'bad']);
  });
  it('gauges_hot_gpu_and_clamped_pct', () => {
    const g = gauges(h(120, 90), null);
    expect(g.map((x) => [x.key, x.pct, x.text, x.level])).toEqual([
      ['cpu', 100, '120°', 'bad'],
      ['gpu', 90, '90°', 'bad'],
    ]);
  });
  it('chipData_words_the_four_chips', () => {
    expect(
      chipData({ sunshine: 'busy', uptimeS: 3 * 3600, path: 'relay', relay: 'fra' }, 'Nothing').map((c) => [c.label, c.value]),
    ).toEqual([
      ['Sunshine', 'Streaming'],
      ['Uptime', expect.any(String)],
      ['Network', 'Relay · fra'],
      ['Running', 'Nothing'],
    ]);
    expect(chipData({ sunshine: 'ready', uptimeS: 5, path: null, relay: null }, 'x')[2].value).toBe('–');
  });
  it('pingText_uses_the_latest_sample', () => {
    expect(pingText([])).toEqual({ direct: '–', hub: '–' });
    expect(
      pingText([
        { t: 1, direct: 99, hub: 9 },
        { t: 2, direct: 10.4, hub: 1 },
      ]),
    ).toEqual({ direct: '10 ms', hub: '1 ms' });
    expect(pingText([{ t: 1, direct: null, hub: 2 }])).toEqual({ direct: '–', hub: '2 ms' });
  });
  it('headerVars_dark_text_gets_darker_warning_rings_and_idle_takes_page_text', () => {
    expect(headerVars('#3b82f6', '#ffffff', true, 'BG')).toBe('--h:#3b82f6;--hbg:BG;--on:#ffffff;--rw:var(--warn);--rb:#f87171');
    expect(headerVars('#facc15', '#0b0d12', true, 'BG')).toBe('--h:#facc15;--hbg:BG;--on:#0b0d12;--rw:#92400e;--rb:#b91c1c');
    expect(headerVars('#facc15', '#0b0d12', false, 'BG')).toContain('--on:var(--fg);--rw:var(--warn)');
  });
});
