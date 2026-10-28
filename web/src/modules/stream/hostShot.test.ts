import { describe, expect, it, vi } from 'vitest';
import { keepShot, MAX_SHOT_BYTES } from './hostShot';
import { storesShots } from '../../state/gameId';

const png = (n = 10) => new Blob([new Uint8Array(n)], { type: 'image/png' });

describe('keepShot', () => {
  it('posts the PNG and says ok', async () => {
    const post = vi.fn().mockResolvedValue({});
    const b = png();
    expect(await keepShot(b, post)).toEqual({ ok: true });
    expect(post).toHaveBeenCalledWith(b);
  });
  it('refuses over 8 MiB without sending, accepts exactly 8 MiB', async () => {
    const post = vi.fn().mockResolvedValue({});
    const r = await keepShot(png(MAX_SHOT_BYTES + 1), post);
    expect(r).toMatchObject({ ok: false });
    expect(post).not.toHaveBeenCalled();
    expect(await keepShot(png(MAX_SHOT_BYTES), post)).toEqual({ ok: true });
  });
  it('turns a refusal into a reason, never a throw', async () => {
    expect(await keepShot(png(), vi.fn().mockRejectedValue(new Error('the disk is full')))).toEqual({ ok: false, why: 'the disk is full' });
  });
});

describe('storesShots', () => {
  const caps = { install: false, controls: true, achievements: false, launchOptions: false, saves: false, screenshots: true, art: true };
  it('is for emulator games whose hostd says so', () => {
    expect(storesShots({ appid: 0xe0000001, id: 'emu:dolphin:x', source: 'emu', caps })).toBe(true);
    expect(storesShots({ appid: 0xe0000001, id: 'emu:dolphin:x', source: 'emu', caps: { ...caps, screenshots: false } })).toBe(false);
  });
  it('never for Steam games or an old hostd (no caps, no source)', () => {
    expect(storesShots({ appid: 620, id: 'steam:620', source: 'steam', caps: { ...caps, screenshots: true } })).toBe(false);
    expect(storesShots({ appid: 620 })).toBe(false);
    expect(storesShots({ appid: 0xe0000001 })).toBe(false);
    expect(storesShots({ appid: 0xe0000001, emulator: 'dolphin' })).toBe(false);
  });
});
