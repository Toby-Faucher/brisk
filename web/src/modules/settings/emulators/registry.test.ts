import { describe, expect, it } from 'vitest';
import { EMULATORS, emulatorOf, sectionOf, shownSections } from './registry';

describe('emulator registry', () => {
  it('has unique ids and at least one shown section per emulator', () => {
    expect(new Set(EMULATORS.map((e) => e.id)).size).toBe(EMULATORS.length);
    for (const e of EMULATORS) expect(shownSections(e).length).toBeGreaterThan(0);
  });
  it('Dolphin shows Controllers, Games folders and Art and details; planned sections are kept out', () => {
    const d = emulatorOf('dolphin');
    expect(shownSections(d).map((s) => s.id)).toEqual(['controllers', 'folders', 'art']);
    expect(d.sections.some((s) => s.planned)).toBe(true);
  });
  it('unknown ids and planned sections fall back to the first entry', () => {
    expect(emulatorOf('nope').id).toBe('dolphin');
    expect(emulatorOf(null).id).toBe('dolphin');
    expect(sectionOf(emulatorOf('dolphin'), 'launch').id).toBe('controllers');
    expect(sectionOf(emulatorOf('dolphin'), 'art').id).toBe('art');
    expect(sectionOf(emulatorOf('dolphin'), 'folders').id).toBe('folders');
    expect(sectionOf(emulatorOf('dolphin'), undefined).id).toBe('controllers');
  });
});
