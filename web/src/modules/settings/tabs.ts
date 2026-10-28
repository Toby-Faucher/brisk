import { emulatorOf, sectionOf } from './emulators/registry';

export const TABS = ['general', 'streaming', 'controller', 'emulators', 'about'] as const;
export type Tab = (typeof TABS)[number];
export const TAB_KEY = 'brisk.settingsTab';

const isTab = (v: unknown): v is Tab => TABS.includes(v as Tab);

/** '#about' or 'about' -> 'about'; anything unknown -> null. A sub part after '/' ('#controller/dolphin') is ignored here. */
export const parseHash = (h: string): Tab | null => {
  const v = h.replace(/^#/, '').split('/')[0];
  return isTab(v) ? v : null;
};
/** The sub part: '#controller/dolphin' -> 'dolphin'. Only a known tab has one; the card decides if it knows the name. */
export const subOf = (h: string): string | null => (parseHash(h) ? h.replace(/^#/, '').split('/')[1] || null : null);
export const hashFor = (t: Tab) => `#${t}`;
/** '#emulators/dolphin/controllers' -> its ids; a missing or unknown part falls back to the first emulator / section. */
export const emulatorRoute = (h: string) => {
  const [, e, s] = h.replace(/^#/, '').split('/');
  const emu = emulatorOf(e);
  return { emu: emu.id, section: sectionOf(emu, s).id };
};
export const emulatorHash = (emu: string, section: string) => `#emulators/${emu}/${section}`;
/** Old links: '#controller/dolphin' became '#emulators/dolphin/controllers'. Null when the hash needs no redirect. */
export const redirectOf = (h: string) => (h.replace(/^#/, '') === 'controller/dolphin' ? emulatorHash('dolphin', 'controllers') : null);
/** A known hash wins; an unknown one is General; no hash uses the last used tab, then General. */
export const initialTab = (hash: string, stored: string | null): Tab =>
  parseHash(hash) ?? (hash.replace(/^#/, '') ? 'general' : isTab(stored) ? stored : 'general');
