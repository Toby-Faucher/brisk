# UI styles

Settings > General > Style (or Alt+U) sets `html[data-ui]`. `brisk` is the original look and has no folder here.
Every other id in `registry.ts` owns one folder, `themes/<id>/`.

Rules for a style's author:

- Edit only `themes/<id>/`. Do not touch shared files (Shell, Sidebar, base.css, tokens.css, Settings, registry.ts, index.css).
- Every selector starts with `:root[data-ui='<id>']`. Never leak into other styles or the original look.
- Restyle by overriding tokens (`--bg --panel --panel2 --line --fg --muted --accent --font --r-card --r-ctl` ...) and by
  rearranging the existing DOM with CSS (grid, position, order). Component styles are scoped by Svelte, so to beat them use
  `:root[data-ui='<id>'] .side` (higher specificity) or `:global` is not available in plain CSS, just use the class names.
- To add elements (a dock, overlay, canvas, extra nav) ship `themes/<id>/Layer.svelte`. Shell mounts it with `{path}`.
  It must be `pointer-events: none` where it covers content, and must not draw over `/stream`.
- Fonts: self-hosted only, via an exact-pinned `@fontsource` package that is at least 14 days old. No CDN.
- Every control that exists today must still exist and work (sidebar links, host pill, Command menu on Ctrl+K, Settings, the
  Stream page controls). They may move, shrink into a menu or change shape, never disappear.
- /stream: restyle the chrome only. Do not transform `main` (the stream uses `position: fixed`).
- Respect `prefers-reduced-motion`. Keep contrast at WCAG AA (axe runs on every view).
- Strings you add must pass `scripts/check-wording.sh` (no em dashes, no stock AI phrases).
