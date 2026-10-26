import { writable } from 'svelte/store';
import type { Preset } from './api';

export type Accent = 'mint' | 'indigo' | 'pink' | 'orange' | 'lime' | 'sky';
/** Same values as tokens.css, for code that needs the colour itself (the smoke shader). */
export const ACCENT_HEX: Record<Accent, string> = { mint: '#5eead4', indigo: '#818cf8', pink: '#f472b6', orange: '#fb923c', lime: '#a3e635', sky: '#60a5fa' };
/** Instant replay length; 0 = off (nothing is kept). */
export const REPLAY_SECONDS = [0, 15, 30, 60] as const;
export type ReplaySeconds = (typeof REPLAY_SECONDS)[number];
/** `clipboardSync`: the Stream page syncs the clipboard with the host both ways. `replaySeconds`: Save replay keeps this much. */
export type Settings = { accent: Accent; sounds: boolean; shaders: boolean; defaultPreset: Preset; clipboardSync: boolean; replaySeconds: ReplaySeconds };

export const SETTINGS_KEY = 'brisk.settings';
const DEFAULTS: Settings = { accent: 'mint', sounds: true, shaders: true, defaultPreset: 'balanced', clipboardSync: true, replaySeconds: 30 };

function read(): Settings {
  try {
    const v = { ...DEFAULTS, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') };
    if (!Object.hasOwn(ACCENT_HEX, v.accent)) v.accent = DEFAULTS.accent;
    if (!REPLAY_SECONDS.includes(v.replaySeconds)) v.replaySeconds = DEFAULTS.replaySeconds;
    return v;
  } catch {
    return { ...DEFAULTS }; // private window, blocked storage, bad JSON, or no localStorage at all
  }
}

export const settings = writable<Settings>(read());
settings.subscribe((v) => {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(v));
  } catch {}
});
