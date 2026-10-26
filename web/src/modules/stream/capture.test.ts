import { describe, expect, it } from 'vitest';
import { fileName, hotkey } from './capture';

const k = (code: string, m: Partial<Record<'ctrlKey' | 'altKey' | 'shiftKey' | 'metaKey', boolean>> = {}) => ({ code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...m });

describe('hotkey', () => {
  it('ctrl_alt_s_and_r_only', () => {
    expect(hotkey(k('KeyS', { ctrlKey: true, altKey: true }))).toBe('shot');
    expect(hotkey(k('KeyR', { ctrlKey: true, altKey: true }))).toBe('clip');
    expect(hotkey(k('KeyS', { ctrlKey: true }))).toBeNull(); // Ctrl+S goes to the game
    expect(hotkey(k('KeyR', { ctrlKey: true, altKey: true, shiftKey: true }))).toBeNull();
    expect(hotkey(k('KeyT', { ctrlKey: true, altKey: true }))).toBeNull();
    // AltGr (Windows reports Ctrl+Alt with it): a typed character, not our shortcut.
    expect(hotkey({ ...k('KeyS', { ctrlKey: true, altKey: true }), getModifierState: (m: string) => m === 'AltGraph' })).toBeNull();
    expect(hotkey({ ...k('KeyS', { ctrlKey: true, altKey: true }), getModifierState: () => false })).toBe('shot');
  });
});

describe('fileName', () => {
  const d = new Date(2026, 9, 5, 7, 8, 9);
  it('game_or_desktop_and_local_time', () => {
    expect(fileName('Pepper Grinder', 'png', d)).toBe('brisk-pepper-grinder-20261005-070809.png');
    expect(fileName(null, 'mp4', d)).toBe('brisk-desktop-20261005-070809.mp4');
    expect(fileName('Café: Ünïcode / ../x', 'mp4', d)).toBe('brisk-cafe-unicode-x-20261005-070809.mp4');
    expect(fileName('★★★', 'png', d)).toBe('brisk-desktop-20261005-070809.png');
  });
});
