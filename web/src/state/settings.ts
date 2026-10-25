import { writable } from 'svelte/store';
import type { Preset } from './api';

export type Accent = 'mint' | 'indigo' | 'pink' | 'orange' | 'lime' | 'sky';
export type Settings = { accent: Accent; sounds: boolean; shaders: boolean; defaultPreset: Preset };

export const SETTINGS_KEY = 'brisk.settings';
const DEFAULTS: Settings = { accent: 'mint', sounds: true, shaders: true, defaultPreset: 'balanced' };

function read(): Settings {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') };
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
