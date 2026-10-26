import { describe, expect, it } from 'vitest';
import { ApiError, type HostInfo } from '../../state/api';
import { applyAccent } from '../settings/accent';
import { machinesFrom, restartError } from './machines';

const info: HostInfo = { name: 'bawn', sunshine: 'ready', uptimeS: 5, cpuTempC: null, gpuTempC: null, path: null, relay: null };

describe('machines', () => {
  it('machinesFrom_returns_single_bawn_entry', () => {
    const m = machinesFrom(info);
    expect(m).toHaveLength(1);
    expect(m[0]).toEqual({ id: 'bawn', info });
  });
  it('restart_429_message', () => {
    expect(restartError(new ApiError('slow down', 429))).toBe('Sunshine was restarted a moment ago, try again in a minute');
    expect(restartError(new ApiError('boom', 500))).toContain('boom');
  });
  it('accent_change_updates_css_variable', () => {
    const root = { dataset: {} as DOMStringMap };
    applyAccent(root, 'pink');
    expect(root.dataset.accent).toBe('pink');
  });
});
