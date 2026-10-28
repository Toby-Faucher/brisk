export const TABS = ['general', 'streaming', 'controller', 'about'] as const;
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
/** A known hash wins; an unknown one is General; no hash uses the last used tab, then General. */
export const initialTab = (hash: string, stored: string | null): Tab =>
  parseHash(hash) ?? (hash.replace(/^#/, '') ? 'general' : isTab(stored) ? stored : 'general');
