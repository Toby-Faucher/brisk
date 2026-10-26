import { describe, expect, it } from 'vitest';
import { writable } from 'svelte/store';
import { applyAccent, bindAccent } from './accent';
import { DEFAULTS, type Settings } from '../../state/settings';

const root = () => ({ dataset: {} as DOMStringMap });

describe('applyAccent', () => {
  it('sets_a_known_accent_and_falls_back_to_mint', () => {
    const r = root();
    applyAccent(r, 'pink');
    expect(r.dataset.accent).toBe('pink');
    applyAccent(r, 'chartreuse');
    expect(r.dataset.accent).toBe('mint');
  });
});

describe('bindAccent', () => {
  it('follows_the_store_until_unsubscribed', () => {
    const r = root();
    const store = writable<Settings>(DEFAULTS);
    const off = bindAccent(store, r);
    expect(r.dataset.accent).toBe('mint');
    store.update((s) => ({ ...s, accent: 'sky' }));
    expect(r.dataset.accent).toBe('sky');
    off();
    store.update((s) => ({ ...s, accent: 'lime' }));
    expect(r.dataset.accent).toBe('sky');
  });
});
