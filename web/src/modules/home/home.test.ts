import { describe, expect, it } from 'vitest';
import type { Game } from '../../state/api';
import { fmtAgo, fmtDuration, fmtTemp, fmtUptime, pickResume } from './home';

const g = (appid: number, installed: boolean, lastPlayed: number | null, running = false): Game => ({ appid, name: `g${appid}`, installed, sizeBytes: null, lastPlayed, playtimeMin: 0, cover: '', running });

describe('home', () => {
  it('resume_picks_most_recent_played_installed_game', () => {
    expect(pickResume([g(1, true, 100), g(2, false, 900), g(3, true, 500), g(4, true, null)])?.appid).toBe(3);
    expect(pickResume([g(1, true, null), g(2, false, 5)])).toBeNull();
    expect(pickResume([])).toBeNull();
  });

  it('resume_prefers_the_running_game', () => {
    expect(pickResume([g(1, true, 900), g(2, true, 100, true), g(3, true, 500)])?.appid).toBe(2);
  });

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
});
