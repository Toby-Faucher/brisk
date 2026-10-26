export const TABS = ['general', 'streaming', 'controller', 'about'] as const;
export type Tab = (typeof TABS)[number];
export const TAB_KEY = 'brisk.settingsTab';

const isTab = (v: unknown): v is Tab => TABS.includes(v as Tab);

/** '#about' or 'about' -> 'about'; anything unknown -> null. */
export const parseHash = (h: string): Tab | null => {
  const v = h.replace(/^#/, '');
  return isTab(v) ? v : null;
};
export const hashFor = (t: Tab) => `#${t}`;
/** A known hash wins; an unknown one is General; no hash uses the last used tab, then General. */
export const initialTab = (hash: string, stored: string | null): Tab =>
  parseHash(hash) ?? (hash.replace(/^#/, '') ? 'general' : isTab(stored) ? stored : 'general');
