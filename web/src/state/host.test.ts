import { afterEach, describe, expect, it, vi } from 'vitest';
import { pollHost } from './host';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('host', () => {
  it('host_store_polls_only_when_visible', async () => {
    vi.useFakeTimers();
    const f = vi.fn(async (_url: string) => new Response('{}'));
    vi.stubGlobal('fetch', f);
    const doc = Object.assign(new EventTarget(), { hidden: true });
    const stop = pollHost(doc);
    await vi.advanceTimersByTimeAsync(35_000);
    expect(f).not.toHaveBeenCalled();

    doc.hidden = false;
    doc.dispatchEvent(new Event('visibilitychange'));
    expect(f).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(f).toHaveBeenCalledTimes(3);
    expect(f.mock.calls[0][0]).toBe('/api/host');

    stop();
    await vi.advanceTimersByTimeAsync(60_000);
    doc.dispatchEvent(new Event('visibilitychange'));
    expect(f).toHaveBeenCalledTimes(3);
  });
});
