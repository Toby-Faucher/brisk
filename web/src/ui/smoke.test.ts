import { describe, expect, it } from 'vitest';
import { smokeAllowed, smokeGL } from './smoke';

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
});
