import { describe, expect, it } from 'vitest';
import { TABS, hashFor, initialTab, parseHash, subOf } from './tabs';

describe('settings tabs', () => {
  it('parses known hashes, with or without #', () => {
    for (const t of TABS) expect(parseHash(`#${t}`)).toBe(t);
    expect(parseHash('about')).toBe('about');
  });
  it('unknown or empty hash is null', () => {
    for (const h of ['', '#', '#nope', '#About', '#nope/x']) expect(parseHash(h)).toBeNull();
  });
  it('#controller/dolphin opens the Controller tab and yields the sub anchor; an unknown sub part still opens the tab', () => {
    expect(parseHash('#controller/dolphin')).toBe('controller');
    expect(subOf('#controller/dolphin')).toBe('dolphin');
    expect(initialTab('#controller/dolphin', 'about')).toBe('controller');
    expect(parseHash('#about/whatever')).toBe('about');
    expect(subOf('#controller')).toBeNull();
    expect(subOf('#controller/')).toBeNull();
    expect(subOf('#nope/dolphin')).toBeNull();
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
