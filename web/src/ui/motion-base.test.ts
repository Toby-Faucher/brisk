import { afterEach, describe, expect, it, vi } from 'vitest';
import { motionAllowed, ms, onStreamRoute, reducedMotion } from './motion-base';

let path = '/';
let reduce = false;
let vars: Record<string, string> = {};
vi.stubGlobal('location', {
  get pathname() {
    return path;
  },
});
const matchMedia = () => ({
  get matches() {
    return reduce;
  },
});
vi.stubGlobal('matchMedia', matchMedia);
vi.stubGlobal('document', { documentElement: {} });
vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: (n: string) => vars[n] ?? '' }));

afterEach(() => {
  path = '/';
  reduce = false;
  vars = {};
  vi.unstubAllEnvs();
});

describe('onStreamRoute', () => {
  it('matches /stream and its sub paths only', () => {
    expect(onStreamRoute('/stream')).toBe(true);
    expect(onStreamRoute('/stream/')).toBe(true);
    expect(onStreamRoute('/stream-legacy/')).toBe(true);
    expect(onStreamRoute('/stream-legacy')).toBe(true);
    expect(onStreamRoute('/library')).toBe(false);
    expect(onStreamRoute('/')).toBe(false);
    expect(onStreamRoute('/library/stream')).toBe(false);
  });
  it('reads location when no path is given', () => {
    path = '/stream';
    expect(onStreamRoute()).toBe(true);
    path = '/history';
    expect(onStreamRoute()).toBe(false);
  });
});

describe('motionAllowed', () => {
  it('follows reduced motion off /stream', () => {
    expect(motionAllowed()).toBe(true);
    reduce = true;
    expect(reducedMotion()).toBe(true);
    expect(motionAllowed()).toBe(false);
  });
  it('throws on /stream in dev', () => {
    vi.stubEnv('DEV', true);
    path = '/stream';
    expect(() => motionAllowed()).toThrow(/stream/);
  });
  it('is a silent no-op on /stream in prod', () => {
    vi.stubEnv('DEV', false);
    path = '/stream';
    expect(motionAllowed()).toBe(false);
  });
  it('allows motion when there is no matchMedia', () => {
    vi.stubGlobal('matchMedia', undefined);
    expect(reducedMotion()).toBe(false);
    expect(motionAllowed()).toBe(true);
    vi.stubGlobal('matchMedia', matchMedia);
  });
});

describe('ms', () => {
  it('reads ms and the minifier s form', () => {
    vars = { '--a': ' 960ms ', '--b': '.96s', '--c': '0s' };
    expect(ms('--a')).toBe(960);
    expect(ms('--b')).toBe(960);
    expect(ms('--c')).toBe(0);
  });
  it('falls back on a missing or unparsable token', () => {
    vars = { '--bad': 'auto' };
    expect(ms('--missing')).toBe(200);
    expect(ms('--missing', 400)).toBe(400);
    expect(ms('--bad', 50)).toBe(50);
  });
});
