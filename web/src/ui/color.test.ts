import { describe, expect, it } from 'vitest';
import { PALETTE, defaultColor, headerBg, normalizeHex, onColor } from './color';

describe('normalizeHex', () => {
  it.each([
    ['#abc', '#aabbcc'],
    ['#ABCDEF', '#abcdef'],
    ['  #1A2b3C ', '#1a2b3c'],
    ['#000', '#000000'],
  ])('accepts %j', (i, o) => expect(normalizeHex(i)).toBe(o));
  it.each(['', 'abc', '#ab', '#abcd', '#abcde', '#abcdefa', '#ggg', 'red', 'rgb(1,2,3)', '# abc'])('rejects %j', (i) =>
    expect(normalizeHex(i)).toBeNull(),
  );
});

describe('defaultColor', () => {
  it('walks_the_palette_and_wraps', () => {
    expect(PALETTE).toHaveLength(8);
    PALETTE.forEach((c, i) => expect(defaultColor(i)).toBe(c));
    expect(defaultColor(8)).toBe(PALETTE[0]);
    expect(defaultColor(19)).toBe(PALETTE[3]);
  });
  it('is_safe_for_negative_and_nan', () => {
    expect(defaultColor(-1)).toBe(PALETTE[7]);
    expect(defaultColor(-8)).toBe(PALETTE[0]);
    expect(defaultColor(NaN)).toBe(PALETTE[0]);
    expect(defaultColor(Infinity)).toBe(PALETTE[0]);
    expect(defaultColor(2.7)).toBe(PALETTE[2]);
  });
});

describe('onColor', () => {
  // The average of the header gradient is mixed 44% into #0b0d12, so its linear luminance tops out just under 0.3 (pure white: 0.298).
  it.each(['#60a5fa', '#a78bfa', '#f472b6', '#f87171', '#fb923c', '#000000', '#000080'])('%s reads white', (c) =>
    expect(onColor(c)).toBe('#ffffff'),
  );
  it.each(['#facc15', '#4ade80', '#2dd4bf', '#ffffff', '#fff', '#ffff00'])('%s reads dark', (c) => expect(onColor(c)).toBe('#0b0d12'));
  it('keeps_every_palette_colour_decided', () => expect(PALETTE.map(onColor).filter((c) => c === '#0b0d12')).toHaveLength(3));
  it('is_case_and_space_tolerant', () => expect(onColor(' #FACC15 ')).toBe(onColor('#facc15')));
  it.each(['', 'nope', '#12'])('gives white for invalid %j', (c) => expect(onColor(c)).toBe('#ffffff'));
});

describe('headerBg', () => {
  it('darkens_toward_the_base_under_white_text', () => expect(headerBg('#60a5fa')).toContain('#0b0d12'));
  it('lightens_under_dark_text', () => {
    const g = headerBg('#facc15');
    expect(g).toContain('#ffffff');
    expect(g).not.toContain('#0b0d12');
  });
  it('falls_back_to_the_first_palette_colour', () => expect(headerBg('nope')).toBe(headerBg(PALETTE[0])));
});
