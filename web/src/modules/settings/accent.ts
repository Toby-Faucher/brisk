import type { Accent } from '../../state/settings';

/** tokens.css swaps --accent through html[data-accent]. */
export const applyAccent = (root: { dataset: DOMStringMap }, a: Accent) => void (root.dataset.accent = a);
