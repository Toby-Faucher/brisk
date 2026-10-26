import { describe, expect, it } from 'vitest';
import type { SessionRec, SessionStats } from '../../state/api';
import { chartRows, fmtClock, fmtWhen, listState, qualityLabel, sessionParam, sparkPath, statRows } from './history';

const rec = (o: Partial<SessionRec> = {}): SessionRec => ({
  appid: 10,
  name: 'Hades',
  started: new Date(2026, 9, 25, 21, 4).getTime(),
  ended: new Date(2026, 9, 25, 22, 31).getTime(),
  host: 'bawn',
  preset: 'crisp',
  codec: 'hevc',
  bitrateKbps: 40000,
  stats: null,
  ...o,
});

describe('list view model', () => {
  it('empty state, loading and errors', () => {
    expect(listState(null, false)).toBe('loading');
    expect(listState(null, true)).toBe('error');
    expect(listState([], false)).toBe('empty');
    expect(listState([rec()], false)).toBe('list');
  });
  it('formats the date, quality and codec; old records have no codec or bitrate', () => {
    expect(fmtWhen(rec().started)).toBe('Sun 25 Oct · 21:04');
    expect(qualityLabel(rec())).toBe('Crisp · HEVC · 40 Mbps');
    expect(qualityLabel(rec({ preset: 'custom', codec: 'h264', bitrateKbps: 8500 }))).toBe('Custom · H.264 · 8.5 Mbps');
    expect(qualityLabel(rec({ codec: undefined, bitrateKbps: undefined }))).toBe('Crisp');
    expect(fmtClock(0)).toBe('0:00');
    expect(fmtClock(605)).toBe('10:05');
    expect(fmtClock(3725)).toBe('1:02:05');
  });
  it('sparkline: scaled into the box, y down; flat and short series', () => {
    expect(sparkPath([10, 20, 30], 100, 20)).toBe('M0,19L50,10L100,1');
    expect(sparkPath([5, 5], 100, 20)).toBe('M0,10L100,10');
    expect(sparkPath([5], 100, 20)).toBe('');
    expect(sparkPath([], 100, 20)).toBe('');
  });
});

describe('detail view model', () => {
  it('reads the session param', () => {
    expect(sessionParam('?session=1761426240000')).toBe(1761426240000);
    expect(sessionParam('?session=abc')).toBeNull();
    expect(sessionParam('')).toBeNull();
  });
  it('chart rows from series points', () => {
    expect(chartRows([[0, 20, 60, 18], [5, 22.5, 59, 19]])).toEqual([
      { t: 0, delay: 20, fps: 60, mbps: 18 },
      { t: 5, delay: 22.5, fps: 59, mbps: 19 },
    ]);
  });
  it('stat rows: formatted, missing values as a dash, none for an old record', () => {
    const s: SessionStats = {
      durationS: 970.2,
      frames: 58193,
      sent: 58181,
      dropped: 11,
      resets: 0,
      idrRequests: 2,
      bridgeP50: 4.3,
      bridgeP99: 8.7,
      hostP50: 4.2,
      hostP99: 8.6,
      audioLost: 0,
      audioErrors: 0,
      inputReceived: 48138,
      inputSent: 46605,
      inputDropped: null,
      gate: 'pass',
    };
    const rows = Object.fromEntries(statRows(s));
    expect(rows['Bridge time']).toBe('16:10');
    expect(rows['Frames sent']).toBe('58,181 of 58,193');
    expect(rows['Dropped / resets / key frames']).toBe('11 / 0 / 2');
    expect(rows['Capture → bridge write']).toBe('4.3 / 8.7 ms');
    expect(rows['Host capture + encode']).toBe('4.2 / 8.6 ms');
    expect(rows['Input received / sent / dropped']).toBe('48,138 / 46,605 / –');
    expect(statRows(null)).toEqual([]);
  });
});
