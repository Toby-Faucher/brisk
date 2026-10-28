import { describe, expect, it } from 'vitest';
import { TABS, emulatorHash, emulatorRoute, hashFor, initialTab, parseHash, redirectOf, subOf } from './tabs';

describe('settings tabs', () => {
  it('parses known hashes, with or without #', () => {
    for (const t of TABS) expect(parseHash(`#${t}`)).toBe(t);
    expect(parseHash('about')).toBe('about');
  });
  it('unknown or empty hash is null', () => {
    for (const h of ['', '#', '#nope', '#About', '#nope/x']) expect(parseHash(h)).toBeNull();
  });
  it('a sub part is ignored by parseHash and read by subOf; an unknown sub part still opens the tab', () => {
    expect(parseHash('#emulators/dolphin/controllers')).toBe('emulators');
    expect(subOf('#emulators/dolphin/controllers')).toBe('dolphin');
    expect(initialTab('#emulators/dolphin', 'about')).toBe('emulators');
    expect(parseHash('#about/whatever')).toBe('about');
    expect(subOf('#controller')).toBeNull();
    expect(subOf('#controller/')).toBeNull();
    expect(subOf('#nope/dolphin')).toBeNull();
  });
  it('the old #controller/dolphin link redirects; nothing else does', () => {
    expect(redirectOf('#controller/dolphin')).toBe('#emulators/dolphin/controllers');
    for (const h of ['', '#controller', '#controller/other', '#emulators/dolphin']) expect(redirectOf(h)).toBeNull();
  });
  it('emulator routes fall back to the first emulator and section', () => {
    expect(emulatorRoute('#emulators/dolphin/controllers')).toEqual({ emu: 'dolphin', section: 'controllers' });
    for (const h of ['#emulators', '#emulators/dolphin', '#emulators/nope/x', '#emulators/dolphin/metadata', ''])
      expect(emulatorRoute(h)).toEqual({ emu: 'dolphin', section: 'controllers' });
    expect(emulatorHash('dolphin', 'controllers')).toBe('#emulators/dolphin/controllers');
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
