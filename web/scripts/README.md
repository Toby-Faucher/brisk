# web/scripts

Check scripts for the UI. Run them from `web/`. None are part of `npm run ci` (that is `check` + `test` + `build`).
Most drive headless Chrome with `playwright-core` (`BRISK_CHROMIUM` overrides the browser path) and print PASS/FAIL.
The "preview" scripts need `npx astro preview --host 127.0.0.1 --port <port> &` running first (build with `npm run build`);
they mock `/api`, so no hostd is needed. Screenshots go to `$OUT`. Details and results: `docs/ui-notes.md`.

| Script | Checks | Run when |
|---|---|---|
| `ui-smoke.mjs` (`npm run smoke`) | Real hostd on fixtures + fake bridge, every main view, restart dialog, Ctrl+K, a launch to `live`, axe on each view. Needs `target/release/brisk-hostd` (or `BRISK_HOSTD`) and port 8443 free (exit 3 = skipped). | Before merging UI or hostd API changes. |
| `axe-check.mjs` | axe-core WCAG A/AA on all views against preview (port 4424). Same rules as the smoke, no hostd. | Any markup or colour change; use it when 8443 is taken. |
| `axe.mjs` | Helper, not a command: the shared axe pass and its `EXCEPTIONS` list. Imported by the checks marked axe below. | Edit when an axe exception is justified. |
| `gen-spring-tokens.mjs` (`npm run springs:check`) | Writes (or with `--check` verifies) the generated spring block in `src/styles/tokens.css`. | After changing a spring definition. |
| `headless.mjs` (`npm run headless`) | Against a running `brisk-bridge`: WebTransport connect, framing, H.264/Opus decode, input. Uses `/stream-legacy/`. | Bridge or stream pipeline changes (see `docs/m2-notes.md`). |
| `clip-check.mjs` | Screenshot and replay-clip code in headless Chrome: native-size PNG, Crisp-size ring, long tasks, MP4 playback, hotkeys. | Changes to `src/modules/stream` replay, screenshot or hotkeys. |
| `platform-check.mjs` | Platform pass: dialog exits, popover tooltips, Library view transition, wake lock (port 4470; sections `dialog tip vt wake`). axe. | Changes to dialogs, tooltips, Library filter, Stream wake lock. |
| `motion-check.mjs` | Page fade, tabs, scroll reveal (port 4421). | Motion changes. |
| `motion-check-2.mjs` | Load entrance, Library skeleton to data, no re-fire, reduced motion (port 4422). | Motion changes. |
| `anime-check.mjs` | anime.js pass: `/stream` import graph, tweens, revert, reduced motion, chart draw-in (port 4423). | Changes to `src/ui/motion.ts` or anime usage. |
| `motion2a-check.mjs` | Status-dot ring, icon morphs, hero split-text, controller chips, connection path (port 4480). | Changes to those widgets. |
| `f3b-check.mjs` | Machines status transitions, Add-a-module picker (port 4490). axe. | Changes to Machines or the picker. |
| `f3c1-check.mjs` | Home drag-to-reorder: mouse, touch, pen, keyboard, persistence (port 4491). axe. | Changes to Home layout or drag code. |
| `f3c2-check.mjs` | History cover to detail header view transition, Back/Forward (port 4492). axe. | After any Astro upgrade (see the overnight report), or History changes. |

The f3b/f3c1/f3c2/motion2a names are the motion pass 2 task ids from `docs/plans/2026-10-25-queue-tooling-hardening.md`.
