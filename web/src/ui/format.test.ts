import { describe, expect, it } from 'vitest';
import { fmtAgo, fmtDate, fmtDuration, fmtTemp, fmtUptime } from './format';

describe('format', () => {
  it('dash_for_null_values', () => {
    expect(fmtTemp(null)).toBe('–');
    expect(fmtTemp(47.6)).toBe('48 °C');
    expect(fmtUptime(null)).toBe('–');
    expect(fmtUptime(75_600)).toBe('21 h');
    expect(fmtUptime(2 * 86_400 + 3 * 3600)).toBe('2 d 3 h');
    expect(fmtUptime(600)).toBe('10 min');
    expect(fmtAgo(null, 0)).toBe('–');
    expect(fmtAgo(0, 38 * 60_000)).toBe('38 min ago');
    expect(fmtDuration(0, null)).toBe('live');
    expect(fmtDuration(0, 64 * 60_000)).toBe('1 h 04');
    expect(fmtDuration(0, 12 * 60_000)).toBe('12 min');
  });

  it('date_is_the_short_locale_date', () => {
    const s = Date.UTC(2026, 9, 3, 12) / 1000;
    expect(fmtDate(s)).toBe(new Date(s * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' }));
    expect(fmtDate(s)).toMatch(/2026/);
  });
});
