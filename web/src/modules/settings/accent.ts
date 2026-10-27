import { ACCENT_HEX, type Settings } from '../../state/settings';

/** tokens.css swaps --accent through html[data-accent]; unknown values fall back to mint. */
export const applyAccent = (root: { dataset: DOMStringMap }, a: string) => void (root.dataset.accent = Object.hasOwn(ACCENT_HEX, a) ? a : 'mint');

/** Keeps root in step with the store (single source of truth); returns the unsubscribe. */
export const bindAccent = (store: { subscribe: (f: (s: Settings) => void) => () => void }, root: { dataset: DOMStringMap }) =>
  store.subscribe((s) => applyAccent(root, s.accent));
