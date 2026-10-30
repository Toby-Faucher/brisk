import { DEFAULT_UI, isUiTheme, UI_IDS, type UiTheme } from './registry';

/** Theme CSS keys off html[data-ui]; unknown values fall back to the original look. */
export const applyUi = (root: { dataset: DOMStringMap }, ui: string) => void (root.dataset.ui = isUiTheme(ui) ? ui : DEFAULT_UI);

/** Keeps root in step with the store; returns the unsubscribe. */
export const bindUi = (store: { subscribe: (f: (s: { ui: UiTheme }) => void) => () => void }, root: { dataset: DOMStringMap }) =>
  store.subscribe((s) => applyUi(root, s.ui));

/** The style `d` steps from `ui` in the list (wraps), for the Alt+U cycle. */
export const nextUi = (ui: UiTheme, d = 1): UiTheme => UI_IDS[(UI_IDS.indexOf(ui) + d + UI_IDS.length) % UI_IDS.length];
