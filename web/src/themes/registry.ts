/** The UI styles Settings > General offers. `brisk` is the original look; every other id owns web/src/themes/<id>/. */
export const UI_THEMES = [
  { id: 'brisk', name: 'Brisk', blurb: 'The original look' },
  { id: 'console', name: 'Console shelf', blurb: 'One row of big tiles, one focus' },
  { id: 'bento', name: 'Bento', blurb: 'Tiles of different sizes' },
  { id: 'aqua', name: 'Aqua glass', blurb: 'Glossy glass and soft gradients' },
  { id: 'poster', name: 'Poster wall', blurb: 'Cover art is the interface' },
  { id: 'crt', name: 'CRT arcade', blurb: 'Scanlines, phosphor, pixels' },
  { id: 'panel', name: 'Hardware panel', blurb: 'Labelled controls, one loud colour' },
  { id: 'swiss', name: 'Swiss editorial', blurb: 'Big type on a strict grid' },
  { id: 'command', name: 'Command first', blurb: 'One search box, lists under it' },
  { id: 'sticker', name: 'Sticker', blurb: 'Chunky, bright and playful' },
  { id: 'win95', name: 'Classic desktop', blurb: 'Beveled windows and a taskbar' },
] as const;

export type UiTheme = (typeof UI_THEMES)[number]['id'];
export const UI_IDS: readonly UiTheme[] = UI_THEMES.map((t) => t.id);
export const DEFAULT_UI: UiTheme = 'brisk';
export const isUiTheme = (v: unknown): v is UiTheme => UI_IDS.includes(v as UiTheme);
