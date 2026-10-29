import { describe, expect, it } from 'vitest';
import { safeGet, safeSet } from './storage';

const memory = (): Storage => {
  const m = new Map<string, string>();
  return {
    get length() {
      return m.size;
    },
    clear: () => m.clear(),
    getItem: (k) => m.get(k) ?? null,
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => void m.delete(k),
    setItem: (k, v) => void m.set(k, v),
  };
};

describe('storage', () => {
  it('reads, writes and removes', () => {
    const s = memory();
    expect(safeGet(() => s, 'k')).toBeNull();
    safeSet(() => s, 'k', 'v');
    expect(safeGet(() => s, 'k')).toBe('v');
    safeSet(() => s, 'k', null);
    expect(safeGet(() => s, 'k')).toBeNull();
  });

  it('never throws when storage is out of reach or refuses', () => {
    const gone = (): Storage => {
      throw new DOMException('denied', 'SecurityError');
    };
    expect(safeGet(gone, 'k')).toBeNull();
    expect(() => safeSet(gone, 'k', 'v')).not.toThrow();
    const full = {
      ...memory(),
      setItem: () => {
        throw new DOMException('full', 'QuotaExceededError');
      },
    };
    expect(() => safeSet(() => full, 'k', 'v')).not.toThrow();
  });
});
