import { describe, expect, it, vi } from 'vitest';
import { hoverRelease, smokeAllowed, smokeGL } from './smoke';

describe('smoke', () => {
  it('smoke_falls_back_without_webgl2', () => {
    expect(smokeGL({ getContext: () => null })).toBeNull();
    const noFloat = { getExtension: () => null };
    expect(smokeGL({ getContext: () => noFloat })).toBeNull();
    const ok = { getExtension: () => ({}) };
    expect(smokeGL({ getContext: (id: string) => (id === 'webgl2' ? ok : null) })).toBe(ok);
  });

  it('smoke_never_on_stream', () => {
    expect(smokeAllowed('/', true)).toBe(true);
    expect(smokeAllowed('/', false)).toBe(false);
    expect(smokeAllowed('/stream', true)).toBe(false);
    expect(smokeAllowed('/stream/', true)).toBe(false);
    expect(smokeAllowed('/streamers', true)).toBe(true);
  });

  it('card_smoke_released_after_fade_and_kept_on_rehover', () => {
    vi.useFakeTimers();
    const seen: boolean[] = [];
    const h = hoverRelease((m) => seen.push(m), 600);
    h.on();
    h.off();
    vi.advanceTimersByTime(300);
    h.on(); // back before the fade ended: keep it
    vi.advanceTimersByTime(1000);
    expect(seen).toEqual([true, true]);
    h.off();
    vi.advanceTimersByTime(599);
    expect(seen.at(-1)).toBe(true);
    vi.advanceTimersByTime(1);
    expect(seen.at(-1)).toBe(false);
    h.off();
    h.stop(); // unmount cancels a pending release
    vi.advanceTimersByTime(1000);
    expect(seen).toEqual([true, true, false]);
    vi.useRealTimers();
  });
});
