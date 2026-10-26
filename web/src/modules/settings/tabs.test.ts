import { describe, expect, it } from 'vitest';
import { TABS, hashFor, initialTab, parseHash } from './tabs';

describe('settings tabs', () => {
  it('parses known hashes, with or without #', () => {
    for (const t of TABS) expect(parseHash(`#${t}`)).toBe(t);
    expect(parseHash('about')).toBe('about');
  });
  it('unknown or empty hash is null', () => {
    for (const h of ['', '#', '#nope', '#About', '#general/x']) expect(parseHash(h)).toBeNull();
  });
  it('serialises', () => expect(hashFor('streaming')).toBe('#streaming'));
  it('falls back hash, then stored, then general', () => {
    expect(initialTab('#about', 'streaming')).toBe('about');
    expect(initialTab('#bogus', 'streaming')).toBe('general'); // a link to nowhere is General, not last used
    expect(initialTab('#', 'streaming')).toBe('streaming');
    expect(initialTab('', 'streaming')).toBe('streaming');
    expect(initialTab('', 'bogus')).toBe('general');
    expect(initialTab('', null)).toBe('general');
  });
});
