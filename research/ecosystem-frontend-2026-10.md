# Research: front-end ecosystem for Brisk, October 2026

Date 2026-10-25. Research only: nothing installed in the repo, `web/dist` only read (file sizes), no repo file changed. Primary sources: `registry.npmjs.org` and `api.npmjs.org` (versions, dates, licences, weekly downloads = last 7 days, deps, `hasInstallScript`), `gh api` for the GitHub repos (last push, stars, published security advisories), `npm audit` in scratch dirs, `@mdn/browser-compat-data` 8.1.7 (published 2026-10-22), chromestatus.com's API, chromiumdash, the Node release schedule, official docs (Astro, Svelte blog, TypeScript blog, Vitest, Biome, mediabunny), and a feature probe run in the Chromium 156 headless-shell that Playwright already has on this box. Sizes come from a scratch Vite 8.3.5 + Svelte plugin build (minified, `target: chrome140`, gzip -9, one entry per library, tree-shaken; scratch dir `/tmp/claude-1000/fe-lab/`, deleted at the end). Pages read through WebFetch are summarised by a small model, so wording could be off. [U] = unverified.

Size caveat: entries that are Svelte components include the shared Svelte runtime (8.3 kB gz for a bare `mount`, measured). Brisk already ships it, so subtract about 8 kB for those. Barrel imports (`import *`) overstate what a real app pulls in; those rows say so.

## TL;DR

- **Nothing in the current set is vulnerable or deprecated today:** `npm audit` on a copy of `web/package.json` + lockfile gives 0 vulnerabilities (366 packages, 1 install-script package: `fsevents`, macOS-only optional). But there is **no `tsconfig.json` and no type-check script**, most deps are caret ranges (not exact), and layerchart pins four `-next` prerelease packages.
- **Chrome:** stable is **156** (2026-10-21). Chrome moved to a **two-week release cycle** with 153 on 2026-09-08, so Toby's 152 is about 2 months and 4 majors old. Update it. Every platform feature you listed is in 152 except a few that are still experimental (see section 3).
- **Adopt next, ranked:** (1) CSS/platform swaps with zero deps (`@starting-style` exits, `linear()` springs, `<dialog closedby>`, popover + anchor tooltips, `sibling-index()` stagger, `content-visibility`, Screen Wake Lock on Stream); (2) `svelte-check` + pinned `typescript@6.0.3` + a tsconfig; (3) `@axe-core/playwright` in `ui-smoke.mjs`; (4) exact-pin everything (`save-exact`) and bump layerchart 2.6.1 to 2.9.1 and mediabunny 1.61.4 to 1.62.1; (5) Vitest browser mode (`@vitest/browser-playwright` + `vitest-browser-svelte`) with the first component test; (6) `@fontsource-variable/geist` + `geist-mono` (about 21 kB less font, any weight); (7) with Agents: `marked` + native `Element.setHTML()` (Sanitizer) + `@xyflow/svelte` + `@dagrejs/dagre`; (8) with Terminal/Media: `@xterm/xterm` (+fit, webgl), `jassub` + `media-chrome` + `hls.js/light` (or mediabunny HLS).
- **Definitely skip:** GSAP (proprietary "no charge" licence), anime.js and Motion until CSS demonstrably fails, TypeScript 7 for now (svelte-check, `@astrojs/check`, typescript-eslint peer ranges stop at 6.x), shadcn-svelte / Skeleton (Tailwind), svelte-sonner, svelte-persisted-store (stale), cmdk-sv (abandoned), elkjs (EPL/GPL), svelte-streamdown (pulls katex with an open advisory), `@lhci/cli`, DOMPurify (native Sanitizer in Chrome 146), temporal-polyfill / date-fns (native Temporal in Chrome 144), TanStack Query, ECharts / three / Orama, vite-plugin-pwa, the npm package literally named `dash.js` (not the real dash.js, see section 4).
- **Bits UI is still the best headless base for Svelte 5** (weekly releases, 2.20.2 on 2026-10-23). Melt UI has been quiet since 2025-03. Bus factor is 1 (huntabyte).

## Top surprises

1. Chrome now ships every two weeks (153 on 2026-09-08); stable is 156, Toby is on 152.
2. TypeScript 7.0 (native Go port) shipped 2026-06-18, but the Svelte/Astro checkers still declare `typescript ^5 || ^6`.
3. Brisk has no tsconfig and no type-check at all; `typescript` is only in the lock as a peer at 6.0.3.
4. layerchart depends on four prerelease (`-next`) `@layerstack/*` packages and 31 direct deps (78 installed); it is also at 2.9.1 now, four minors ahead of the pinned 2.6.1.
5. Mediabunny 1.62 reads HLS input and is hardened against malicious files, so a media player may not need hls.js at all; and `Temporal`, `Sanitizer`, `contrast-color()`, `sibling-index()`, `if()` all work in Chromium 156 today (probed).

## 1. Current dependency set and risks

Direct deps (from `web/package.json`) versus latest on 2026-10-25:

| Package | Pinned | Latest (date) | Note |
|---|---|---|---|
| astro | ^7.4.2 | 7.4.2 (2026-10-21) | current; floor 7.3.3 matters (see advisories) |
| @astrojs/svelte | ^9.1.1 | 9.1.1 (2026-10-21) | current |
| svelte | ^5.57.2 | 5.57.2 (2026-10-06) | current; floor >=5.55.7 matters |
| bits-ui | ^2.20.2 | 2.20.2 (2026-10-23) | current |
| layerchart | 2.6.1 | **2.9.1** (2026-10-23) | 2.7 to 2.9: Axis `domain`, annotation snippets, tooltip placement for touch, new `Bin`/`GeoBin` marks; no breaking change noted in the changelog entries I read. Check History charts visually after the bump |
| mediabunny | 1.61.4 | **1.62.1** (2026-10-22) | 1.62.0 hardens parsers against hostile files and fixes HLS timestamp logic; our clips only mux our own data, so low urgency |
| @fontsource/geist, geist-mono | ^5.3.0 | 5.3.0 | current |
| vitest | ^5.0.5 | 5.0.5 (2026-10-15) | the floor 5.0.5 is exactly the patched version for GHSA-p458 (see below) |
| playwright-core | ^1.64.1 | 1.64.1 (2026-10-14) | current |

Findings:

- **`npm audit` (all and `--omit=dev`): 0 vulnerabilities.** 366 packages in the lock (269 prod). No deprecated packages. The only `hasInstallScript` package is `fsevents` (optional, macOS).
- **Resolved tree:** vite 8.3.5, rolldown 1.2.15, `@sveltejs/vite-plugin-svelte` 7.3.1, shiki 4.5.0 and zod 4.6.7 (both Astro's), `typescript` 6.0.3 (peer only), sharp 0.35.5, lightningcss 1.33.0.
- **Recent advisories that your floors already clear** (GitHub published advisories, so keep the lock at or above these):
  - Astro: critical RCE via AVIF image optimisation, fixed 7.2.8 (2026-08-27); XSS in JSX/MDX script/style, fixed 7.3.2; **CSS injection in View Transitions (low), fixed 7.3.3**; two more medium ones on other versions. We are on 7.4.2. Brisk does not use the image service.
  - Vite 8: three dev-server `server.fs` bypasses fixed in 8.3.3 (2026-10-06). We are on 8.3.5. Only matters for `astro dev` (bound to 127.0.0.1).
  - Vitest: `server.fs.deny` misapplied in 5.0.x, **fixed in 5.0.5**; earlier, critical "Browser Mode exposes an API that proxies CDP / overwrites config" (5.0.0 betas) and "provider commands bypass the file-access gate" (<=4.1.9). If Browser Mode is adopted, keep its API on loopback and never expose the dev port on the tailnet.
  - Svelte: several SSR XSS / DOM-clobbering advisories in 2026 (Feb and May), fixed by 5.55.7; we are on 5.57.2.
  - bits-ui, layerchart, mediabunny, rolldown: no published advisories.
- **Weak spots (not vulnerabilities):**
  1. No `tsconfig.json`, no `svelte-check`/`astro check`: type errors never surface. 62+ tests exist, types are unchecked.
  2. Caret ranges on astro, @astrojs/svelte, svelte, bits-ui, fontsource, vitest, playwright-core. Your taste is exact pins; add `save-exact=true` to `.npmrc` and pin the lock's resolved versions.
  3. layerchart 2.6.1 depends on `@layerstack/{utils 2.0.0-next.19, tailwind 2.0.0-next.21, svelte-state 0.1.0-next.25, svelte-actions 1.0.1-next.20}` (prerelease tags, exact-pinned inside layerchart) and brings its own `runed 0.37.1` next to bits-ui's `runed 0.35.1` (two copies). 31 direct deps (d3-geo, sankey, contour, etc.) are installed but tree-shaken out; the real History chunk is 238 kB raw, 65 kB gz. Single maintainer (techniq).
  4. bits-ui is a single-maintainer project (huntabyte; also shadcn-svelte, paneforge, vaul-svelte). Healthy cadence (12 releases since 2026-04-17), but bus factor 1.
  5. Astro 7 specifics worth knowing (docs upgrade guide): Vite 8 and the Rust compiler are the base; the compiler is stricter about invalid HTML (unclosed tags error, `<div>` in `<p>` no longer auto-corrected); `compressHTML` default is now `'jsx'` (inline whitespace can vanish); `src/fetch.ts` is reserved; Sätteri replaces remark as the Markdown processor; `astro:transitions` internals (`TRANSITION_*` constants, `isTransitionBefore*Event()`, `createAnimationScope()`) were removed. Brisk imports only `ClientRouter` and `navigate`, which remain. Astro requires Node >=22.23.4.

## 2. Library tables

Columns: version (publish date), licence, weekly downloads, maintainers/bus factor, last push, advisories, gzip, Svelte 5, verdict. "Trans." = packages installed when that package alone is installed (`npm install --package-lock-only`, includes itself). "Adv." = `npm audit` on the scratch install of the candidates (only svelte-streamdown via katex was flagged) and GitHub published advisories where I checked.

### 2.1 Testing and quality

| Package | Facts | Fit |
|---|---|---|
| vitest | 5.0.5 (2026-10-15), MIT, 83.7M/wk, vitest-dev org, pushed today, 60 trans. Needs Vite >=6.4, Node >=22.12. v5 breaking: `clearMocks` defaults true, locators strict, `bench` is a fixture, fake timers mock `Temporal` | **Have it** |
| @vitest/browser-playwright + vitest-browser-svelte | 5.0.5 (2026-10-15) MIT, 5.9M/wk; vitest-browser-svelte 3.1.0 (2026-09-04) MIT, 191k/wk, community repo (66 stars, 6 npm maintainers). 79 trans. dev-only. Peer is full `playwright`, not `playwright-core`. Svelte 5 yes. Built-in `toMatchScreenshot`, Trace View. Advisories: see section 1 | **Add later** (with the first component test): real Chrome layout, no jsdom |
| @testing-library/svelte | 5.4.2 (2026-06-23) MIT, 625k/wk, 36 trans., needs jsdom/happy-dom. Svelte 5 yes | **Skip** (browser mode is better for a Chrome-only app) |
| happy-dom / jsdom | happy-dom 21.0.4 (2026-10-20) MIT 14.4M/wk, 9 trans.; jsdom 30.1.3 (2026-10-15) MIT 73M/wk, 36 trans. | **Skip** (if a node-env DOM test is ever needed, happy-dom) |
| typescript | 7.0.2 (2026-07-08) Apache-2.0, 196M/wk. TS 7.0 (native, Go) announced 2026-06-18; 6.0 (2026-03-06) was the last JS-based release; 7.1 beta 2026-07-08. **svelte-check 4.7.7, @astrojs/check 0.9.12, svelte-preprocess 6.0.5 peer `typescript ^5 \|\| ^6`; typescript-eslint 8.72 `<6.1`** | **Stay on 6.0.3, pin it.** Retry TS 7 when those peers widen. `@typescript/native-preview` (7.0.0-dev.20260707) is now superseded |
| svelte-check | 4.7.7 (2026-10-16) MIT, 2.1M/wk, sveltejs, 28 trans. | **Add now** (needs a tsconfig; `svelte-check --tsconfig ./tsconfig.json`) |
| @astrojs/check | 0.9.12 (2026-10-21) MIT, 2.7M/wk, withastro, 89 trans. | **Add later** (only 6 thin `.astro` pages; svelte-check covers the bulk) |
| axe-core + @axe-core/playwright | axe-core 4.14.0 (2026-10-05) MPL-2.0, 52.8M/wk, Deque, pushed 2026-10-22, 0 trans.; @axe-core/playwright 4.14.0 (2026-10-15) MPL-2.0, 10M/wk, peer `playwright-core`, 3 trans. | **Add now** (dev): one `new AxeBuilder({page}).analyze()` per route in `ui-smoke.mjs`; dark-only tokens make contrast checks valuable |
| Biome | 2.6.3 (2026-10-24) MIT OR Apache-2.0, 11.9M/wk, 5 npm maintainers, 26k stars, 11 trans. **Svelte and Astro support is "experimental"** and must be enabled with `html.experimentalFullSupportEnabled`; cross-language rules can give false positives (docs) | **Skip for .svelte;** fine for ts/css only. Not worth a second toolchain |
| ESLint 10.13 + eslint-plugin-svelte 3.23.1 + prettier 3.9.10 + prettier-plugin-svelte 4.1.1 | ESLint MIT 102M/wk (77 trans.); plugin MIT 1.1M/wk (98 trans. with the parser and typescript-eslint); prettier MIT 87M/wk (1 trans.), 4.0.0 alpha exists; prettier-plugin-svelte 4.1.1 (2026-06-15) | **Skip** for a solo hobby repo (about 150 packages). If wanted later: prettier + plugin only |
| oxlint 1.89.0 / oxfmt 0.74.0 | MIT, 18M and 11M/wk, 0 and 1 trans., oxc-project (Voidzero). Svelte/Astro script-block support [U] | **Watch** |
| knip | 6.43.0 (2026-10-24) ISC, 12M/wk, 1 maintainer (webpro), pushed today, 63 trans. | **Add later as a one-off `npx knip`**, not a dependency |
| size-limit | 14.3.0 (2026-10-22) MIT, 978k/wk, 4 trans. (+ a preset) | **Skip:** a 10-line script gzipping `dist` does it (you already measure) |
| lighthouse / @lhci/cli | lighthouse 13.5.0 (2026-09-18) Apache-2.0, 3.4M/wk, 114 trans.; @lhci/cli 0.15.1 (**2025-06-25**), 330 trans., deprecated `glob`, `inflight`, `rimraf`, `uuid` in its tree | **Skip lhci.** One-off `npx lighthouse` if wanted; your CDP-based `motion-check.mjs` is closer to what you need |
| Playwright visual regression | `toHaveScreenshot` lives in `@playwright/test` 1.64.1 (Apache-2.0, 49.8M/wk, 3 trans.), not playwright-core. Vitest's `toMatchScreenshot` is the alternative. Both warn that references differ across OS/GPU/fonts | **Skip for now:** one box, one browser, moving smoke background; revisit with Browser Mode (pause animations) |
| pixelmatch | 8.0.0 (2026-10-06) ISC, 6.7M/wk, 1 trans. | Only for a DIY screenshot diff |

### 2.2 Svelte utilities and component bases

| Package | Facts | Fit |
|---|---|---|
| bits-ui | 2.20.2 (2026-10-23) MIT, 820k/wk, huntabyte (1 npm maintainer), pushed 2026-10-23, 3.6k stars, 33 trans., deps esm-env, tabbable, svelte-toolbelt, @floating-ui, runed. 49 kB gz for Dialog+Tooltip+Tabs+Popover import (incl. 8 kB Svelte runtime). Svelte 5 only. We use Dialog, Tabs, Command | **Keep.** Still the best headless Svelte 5 base. Exposes `data-starting-style`/`data-ending-style` so CSS-only enter/exit works |
| @melt-ui/svelte | 0.86.6 (**2025-03-28**) MIT, 106k/wk, repo last push 2025-09-30, `next` tag only | **Skip** (effectively superseded by Bits) |
| @ark-ui/svelte | 5.24.3 (2026-10-05) MIT, 7.8k/wk, Chakra team, pushed today, 108 trans. | **Add later** only for components Bits lacks (tree view, color picker, splitter); heavier, different API |
| shadcn-svelte | 1.7.1 (2026-10-23) MIT, 126k/wk, huntabyte; Tailwind + Bits UI copy-in CLI | **Skip** (no Tailwind) |
| @skeletonlabs/skeleton-svelte | 5.0.1 (2026-08-19) MIT, 37k/wk, 80 trans., Tailwind | **Skip** |
| paneforge | 1.0.2 (2025-08-02) MIT, 134k/wk, svecosystem | **Add later** (resizable split for terminal / agents) |
| vaul-svelte | 0.3.2 (2024-06-29), stale | Skip |
| runed | 0.38.0 (2026-10-23) MIT, 2M/wk, svecosystem, 22 trans., 8.6 kB gz (mostly runtime). Already a transitive dep (two copies) | **Skip direct use;** `PersistedState`, `useDebounce` are 10-line things. Dedupe if both copies bother you |
| svelte-persisted-store | 0.12.0 (**2024-10-31**) MIT, 56k/wk | **Skip** (stale; `$state` + `localStorage` wrapper, which Brisk has) |
| svelte-sonner | 1.2.1 (2026-08-14) MIT, 398k/wk, 21 trans., 26.5 kB gz | **Skip** (you have `Toasts.svelte`) |
| Svelte built-ins | `svelte/transition`, `svelte/animate` (`flip`), `Spring`/`Tween` classes and `prefersReducedMotion` in `svelte/motion` (verified in the 5.57.2 type definitions) | **Use first** (0 bytes) |

Is Bits UI still the best headless base? Yes for a Svelte 5, no-Tailwind project: native Svelte 5 runes API, 12 releases since mid-April, accessible focus/typeahead/dismiss logic you do not want to rewrite. The platform now covers simple cases (section 3): use native `<dialog>`, popover and anchored tooltips for new trivial overlays, and keep Bits for Command, Tabs and anything with real keyboard semantics.

### 2.3 Search, validation, i18n, PWA, errors

| Package | Facts | Fit |
|---|---|---|
| @leeoniya/ufuzzy | 1.0.19 (2025-08-22) MIT, 308k/wk, 1 maintainer, pushed 2026-09-08, 0 trans., **3.8 kB gz** | Add later if `rank()` in CommandMenu outgrows itself; best for short strings (game titles) |
| fuse.js | 7.5.0 (2026-07-13) Apache-2.0, 8.7M/wk, 1 maintainer, 9.0 kB gz | Fine alternative; Skip |
| minisearch | 7.2.0 (2025-09-16) MIT, 2.4M/wk, 1 maintainer, quiet since 2025-09, 5.6 kB gz | **Add later with Agents** (full-text over notes/memory: prefix + fuzzy + field boosts) |
| flexsearch | 0.8.212 (2025-09-06) Apache-2.0, 740k/wk, 17 kB gz | Skip |
| @orama/orama | 3.1.18 (2025-12-19) Apache-2.0 on npm (GitHub says NOASSERTION), 1M/wk, 20.4 kB gz | Skip (hybrid/vector search belongs on the Rust side) |
| valibot | 1.5.0 (2026-09-09) MIT, 14.4M/wk, 1 maintainer, 0 trans., **1.4 kB gz** for a typical object schema | **Add later:** validate hostd JSON at the boundary when the API grows |
| zod | 4.6.7 (2026-10-12) MIT, 222M/wk; `zod/mini` 5.3 kB gz, classic 23 kB; already in the lock via Astro | Skip in the browser |
| arktype | 2.2.9 (2026-10-20) MIT, 1.4M/wk, 45 kB gz | Skip |
| vite-plugin-pwa | 2.0.0 (2026-10-03) MIT, 3.5M/wk, 387 trans. (fsevents, deprecated glob) | **Skip** (non-goal in ui-design) |
| i18n, error reporting | Not researched in depth; non-goals | **Skip** |

### 2.4 Icons

Brisk already inlines SVG paths in `ui/Icon.svelte` and wants filled, coloured icons. A component package adds nothing but bytes.

| Source | Facts | Filled weight? |
|---|---|---|
| Phosphor | `@phosphor-icons/core` 2.1.1 (2024-03-29) MIT, 1.1M/wk, **0 deps**, 1,512 icons x 6 weights (thin, light, regular, bold, **fill**, duotone) as SVG files. `phosphor-svelte` 3.1.1 (2026-10-09) MIT, community (haruaki07, 279 stars), 14 MB unpacked, 13.5 kB gz for one icon incl. runtime | **Yes, true fill + duotone.** Best fit: copy chosen SVGs from the core package into `Icon.svelte` at build time or by hand |
| Tabler | `@tabler/icons-svelte` 3.49.0 (2026-10-05) MIT, 15.7k/wk; about 1,050 `*Filled` icons among 5,000+ outline icons | Partial (filled variants for roughly a fifth) |
| Lucide | `@lucide/svelte` 1.61.0 (2026-10-21) ISC; **`lucide-svelte` is deprecated** in favour of it; 8,150 icon files, 0 filled | **No** (stroke only; you can fake it with CSS `fill`) |
| Iconify | `unplugin-icons` 24.0.0 (2026-09-11) MIT, 28 trans.; `@iconify-json/ph` 1.2.2 (2024-12); `@iconify/svelte` 5.2.3 | Works, ships only used icons; only worth it above about 30 icons |

Verdict: Add now (as a copy step, no runtime dependency) when you next add icons; do not add `phosphor-svelte`.

### 2.5 Motion (and overlap)

What already covers motion with no library: CSS (`@starting-style`, `transition-behavior: allow-discrete`, `linear()` springs, `animation-timeline: view()`, `sibling-index()`, `@property`), Svelte (`transition:`, `animate:flip`, `Spring`/`Tween`, `prefersReducedMotion`), and View Transitions. The earlier research (`research/anime-opportunities.md`) found only three things CSS cannot do: tween numbers, SVG draw-in, choreographed timelines.

| Package | Facts | Fit |
|---|---|---|
| animejs | 4.5.0 (2026-06-22) MIT, 627k/wk, 1 maintainer, repo pushed 2026-08-21; `5.0.0-beta.2` on the beta tag. `animate`+`stagger` **13.1 kB gz** (earlier doc: 15.2 with a different entry). 4.5.0 had breaking changes in a minor | **Skip until CSS fails** (stat-number tween is the one real case) |
| motion (Motion One + Framer Motion core) | 14.6.0 (2026-10-22) MIT, 16.9M/wk, 2 maintainers, 34.5k stars, pushed today. **`motion/mini` 3.1 kB gz** (WAAPI, compositor), `animate` 22.0 kB, + stagger/spring/scroll 25.0 kB | If a JS animation library is ever needed, **this is the one** (smallest, most maintained). Overlaps anime.js fully: pick one, not both |
| gsap | 3.15.0 (2026-04-13), **"Standard 'no charge' license"** (proprietary, not OSI), 3.9M/wk, 26.3 kB gz core. Website says "GSAP is now free for everyone, thanks to Webflow's support"; the licence page itself was unreachable today (site in a security-maintenance notice). From memory: free for commercial use, but not for building tools that compete with Webflow's visual animation products [U] | **Skip** (licence outside your list, 26 kB) |
| @formkit/auto-animate | 0.10.0 (2026-07-10) MIT, 1.1M/wk, 3.0 kB gz | Skip (`animate:flip` / View Transitions cover list reorders) |
| @humanspeak/svelte-motion 2.1.1, svelte-motion 0.12.2 | tiny / stale Framer ports | Skip |

### 2.6 Charts (dense time series, about 3,000 points)

| Package | Facts | Fit |
|---|---|---|
| layerchart | 2.9.1 (2026-10-23) MIT, 197k/wk, 1 maintainer, pushed 2026-10-26, Svelte 5 native. Pinned 2.6.1. Real chunk 65 kB gz (lazy). 3,000 points in an SVG path with a quadtree tooltip is fine | **Keep**; bump to 2.9.1 |
| uplot | 1.6.32 (2025-03-14) MIT, 449k/wk, 1 maintainer (leeoniya), pushed today, **21.3 kB gz**, canvas, built for 100k+ points and live streaming; not Svelte-aware | **Add later** only for a live/streaming chart; do not put it on /stream |
| @observablehq/plot | 0.6.17 (2025-02-14) ISC, 713k/wk, 88 kB gz, SVG, 42 trans. | Skip |
| chart.js | 4.6.1 (2026-10-22) MIT, 7.8M/wk, 53.5 kB gz, canvas | Skip |
| echarts | 6.1.0 (2026-05-19) Apache-2.0, 4M/wk, 179 kB gz tree-shaken line chart | Skip |

### 2.7 Drag and drop, virtualisation, tables, data, hotkeys, palette

| Package | Facts | Fit |
|---|---|---|
| @neodrag/svelte | 2.3.3 (2025-06-18) MIT, 7k/wk, 2.2 kB gz; v3 only on `next` | Add later (free-drag panels) |
| svelte-dnd-action | 0.9.80 (2026-10-05) MIT, 170k/wk, 1 maintainer, 11.5 kB gz, sortable lists. Svelte 5 support [U: check peer range] | **Add later** (reorder bento widgets) |
| @dnd-kit/dom | 0.5.1 (2026-10-15) MIT, 1.5M/wk, pre-1.0, 28.6 kB gz | Skip |
| native HTML DnD / pointer events | no cost | Prefer for a single list |
| @tanstack/virtual-core | 3.19.7 (2026-10-22) MIT, 20.8M/wk, 3 maintainers, 8.9 kB gz, 0 deps; `@tanstack/svelte-virtual` 3.13.48 is a 9 kB wrapper | Skip now (Brisk has its own shelf virtualiser; 139 cards barely need it, and `content-visibility: auto` is native). Use virtual-core for Agents logs |
| virtua | 0.53.6 (2026-10-21) MIT, 706k/wk, 1 maintainer, 18.4 kB gz, has a Svelte entry | Skip |
| @tanstack/svelte-table | 9.3.1 (2026-10-12) MIT, 78k/wk, 23 trans. (39 kB gz via a barrel import, overstated) | Skip until a sortable/filterable table exists |
| @tanstack/svelte-query | 6.3.5 (2026-10-19) MIT, 196k/wk, 20.9 kB gz | **Skip:** stores with refresh-on-focus already do it |
| tinykeys | 4.0.1 (2026-09-25) MIT, 194k/wk, 1 maintainer, **0.9 kB gz** | Skip (you have `shell/keys.ts` + tests) |
| @tanstack/hotkeys + svelte-hotkeys | 0.18.0 / 0.17.0 (both 2026-10-24) MIT, 770k/wk and 5k/wk, 0.x, new in 2026 | Watch |
| hotkeys-js | 4.0.8 (2026-09-09) MIT, 764k/wk | Skip |
| cmdk-sv | 0.0.19 (2025-03-14), no licence field | **Skip** (Bits `Command` is what you use) |

### 2.8 Date and time

| Package | Facts | Fit |
|---|---|---|
| native Intl + Temporal | `Temporal` is in Chrome 144 (MDN); **probe: `typeof Temporal` = object in Chromium 156**. Node 26.11 does **not** have it (`typeof Temporal` undefined), so a node-env unit test of Temporal code needs Browser Mode | **Use native** |
| temporal-polyfill | 1.0.6 (2026-10-13) MIT, 3M/wk (fullcalendar), 18.5 kB gz; `@js-temporal/polyfill` 0.5.1 (2025-03-31) ISC | **Skip** (Chrome-only) |
| date-fns | 4.4.0 (2026-05-29) MIT, 60M/wk, 6.9 kB gz for 3 functions; dayjs 1.11.23 | **Skip** |

### 2.9 Terminal

| Package | Facts | Fit |
|---|---|---|
| @xterm/xterm | 6.0.0 (2025-12-22) MIT, 4.1M/wk, xtermjs org, repo pushed 2026-10-13, 80 kB gz, 0 deps | **Add later with Terminal module** |
| addon-fit 0.11.0 / addon-webgl 0.19.0 / addon-search 0.16.0 | all 2025-12-22 MIT; 0.5 / 29.9 / 9.4 kB gz | fit + webgl yes; search when wanted. `addon-canvas` 0.7.0 (2024) is dead, skip |
| ghostty-web | 0.4.0 (2025-12-09) MIT, 225k/wk, Coder, 3.0k stars, pushed 2026-10-15, `next` builds as late as 2026-06. Real: Ghostty's VT parser in WASM (about 400 kB) with an xterm.js-compatible API, 0 runtime deps; I measured 180 kB gz for the JS entry (WASM presumably inlined). Pre-1.0; selection/accessibility/mobile not documented [U] | **Watch;** swap behind the same API later if it matures |

The host side (PTY over WebTransport/WebSocket from `brisk-hostd`) is the harder part and is Rust, not front-end.

### 2.10 Graphs and flow

| Package | Facts | Fit |
|---|---|---|
| @xyflow/svelte | 1.7.0 (2026-09-24) MIT, 170k/wk, xyflow, repo pushed 2026-10-22, Svelte 5 native, 65.6 kB gz (incl. runtime), 37 trans. v2 on `next` | **Add later with Agents** |
| @dagrejs/dagre | 3.1.3 (2026-10-24) MIT, 3.4M/wk, 15.3 kB gz | **Add later** (layered DAG layout for run graphs) |
| d3-force | 3.0.0 (2021) ISC, 16M/wk, 4.7 kB gz | Fine for force layouts; finished software |
| elkjs | 0.12.0 (2026-07-17) **EPL-2.0 OR GPL-3.0**, 7.9M/wk, 424 kB gz bundled (2.1 kB gz for the worker API) | **Skip** (licence outside your list) |
| cytoscape 3.34.4 (133 kB gz), sigma 4.0.3 (97 kB gz) | MIT | Skip |

Note layerchart already pulls `@dagrejs/dagre` 2.x and `d3-force` in `node_modules` (unused in our bundle).

### 2.11 Markdown, code, sanitising

| Package | Facts | Fit |
|---|---|---|
| marked | 18.1.1 (2026-10-12) MIT (GitHub says NOASSERTION), 55M/wk, 4 maintainers, 13.2 kB gz, 0 deps | **Add later (Agents output)**; re-lex only the last block while streaming |
| markdown-it | 15.0.3 (2026-10-16) MIT, 18M/wk, 39 kB gz, 7 trans. | Skip unless plugins are needed |
| micromark | 4.0.3 (2026-09-26) MIT, 41.8M/wk, 14.7 kB gz, 29 trans. | Alternative if you want a safe-by-default parser |
| mdsvex | 0.12.8 (2026-07-19) MIT, 130k/wk | Skip (build-time docs only) |
| svelte-streamdown 5.0.0 / streamdown 2.7.1 | 2026-10-19; community Svelte port (46k/wk, 1 maintainer) pulls **katex, which has an open "no fix available" prototype-pollution advisory GHSA-238p-pmpm-9mq7 (npm audit)** | **Skip** |
| shiki | 4.5.0 (2026-10-01) MIT, 17M/wk, 4 maintainers. JS-regex engine + 1 language + 1 theme **69 kB gz**; Oniguruma WASM variant 279 kB gz. Already in your lock via Astro | Lazy `import()` only, with the JS engine; or skip |
| highlight.js | 11.12.0 (2026-08-12) BSD-3-Clause, 24M/wk, core + 3 languages **12.5 kB gz** | **Preferred for chat-style code blocks** (small, lazy per language) |
| sugar-high | 2.6.4 (2026-10-24) MIT, 276k/wk, 1 maintainer, 11.2 kB gz; JS/JSX-like only | Skip |
| DOMPurify | 3.4.17 (2026-10-12) MPL-2.0 OR Apache-2.0, 45M/wk, 11 kB gz, 1 maintainer (Cure53), pushed 2026-10-24 | **Skip: use the native Sanitizer** |
| sanitize-html | 2.18.1 MIT, 54 kB gz | Skip |
| native **Sanitizer API** (`Element.setHTML()`) | Chrome 146 (chromestatus: enabled by default 146; MDN agrees; Firefox 148). **Probe: `typeof Sanitizer` = function, `setHTML` present in Chromium 156.** Default config strips scripts and event handlers; more options (`javascriptURLs`) proposed for 157 | **Use it** for rendering untrusted model output as HTML. Still prefer rendering markdown to Svelte nodes where possible |

### 2.12 Video and media

| Package | Facts | Fit |
|---|---|---|
| hls.js | 1.7.3 (2026-09-11) Apache-2.0, 6.4M/wk, 4 maintainers, pushed 2026-10-24, full 172 kB gz, `hls.js/light` 110 kB gz | **Add later with Media** (lazy chunk, never on /stream) |
| mediabunny | 1.62.1 (2026-10-22) MPL-2.0, 2.7M/wk, 1 maintainer (Vanilagy), 7.4k stars, 3 trans.; mp4 mux entry 28.9 kB gz. Docs: HLS input and output supported (`reading-hls`); ASS/SRT not read, WebVTT write-only; no player UI. Hardened in 1.62.0 | Keep. Candidate to feed HLS/WebCodecs into a canvas later; a `<video>` + hls.js is the simpler default |
| dashjs | 5.2.1 (2026-08-17) BSD-3, 558k/wk, 230 kB gz | Skip. **Careful:** the npm package named `dash.js` (4.0.1, 132 downloads/week, 9 maintainers) links to `github.com/hanzo-js/dash.js`, which returns 404. It is not the Dash Industry Forum library (`dashjs`). Do not install it |
| shaka-player | 5.3.2 (2026-10-23) Apache-2.0, 240k/wk, 279 kB gz, 97 MB unpacked | Skip |
| media-chrome | 4.19.4 (2026-10-23) MIT, 2.6M/wk, Mux, 40.6 kB gz, web components, themeable by CSS variables | **Add later** for styled player controls (custom elements work in Svelte) |
| jassub | 2.5.23 (2026-10-19), 19.5k/wk, 1 maintainer (ThaUnknown), 210 stars, 3 trans. + `lfa-ponyfill` has an install script. JS 49.6 kB gz plus two WASM workers (2.1 MB each) and a 143 kB font. **Licence is a stack: LGPL-2.1-or-later AND (FTL OR GPL-2.0-or-later) AND MIT AND ISC AND ...** (libass compiled in) | **Add later with Media for ASS;** flag LGPL (fine for a personal, non-redistributed app). `libass-wasm` 4.1.0 is abandoned (2022) |
| WebVTT | native `<track>` + `::cue` styling | Start here |
| Native HLS in Chrome | not verified [U]; assume hls.js or mediabunny is needed | |

### 2.13 Shaders and WebGL

| Package | Facts | Fit |
|---|---|---|
| shaders (`shaders/svelte`) | 4.0.5 (2026-10-21) MIT, 145k/wk, org shader-effects-inc, 5.5k stars, 29.5 MB unpacked, WebGPU components. Site says the library is free open source; **Shaders Pro is a paid subscription** (code export and MCP server need Pro) | **Skip** (you wrote your own WebGL2 smoke) |
| ogl 1.0.11 | Unlicense, last release 2025-01, repo pushed 2025-04, 12.3 kB gz | Skip |
| twgl.js 7.0.0 | MIT, 2025-07, 6.1 kB gz | **Add later** if you write more than two shaders |
| regl 2.1.1 (38 kB gz), three 0.186.1 (125 kB gz) | MIT | Skip |

### 2.14 Audio, gestures, fonts

| Package | Facts | Fit |
|---|---|---|
| zzfx 1.4.0 | MIT, 1 kB gz, 6k/wk | Skip: `ui/sounds.ts` already synthesises |
| tone 15.1.22 (2025-04), howler 2.2.4 (2023-09) | MIT | Skip |
| svelte-gestures 5.2.2 (2025-09), @use-gesture/vanilla (2024), hammerjs (2016) | | Skip (desktop-first) |
| @fontsource/geist | 5.3.0 (2026-07-19) OFL-1.1, 237k/wk | Current. You load 4 Geist + 2 Geist Mono static weights: about 53 kB + 20 kB of latin woff2 |
| @fontsource-variable/geist + geist-mono | 5.3.0, OFL-1.1, 2.3M/wk and 1.5M/wk | **Add later (small win):** latin variable files are 29.4 kB + 23.1 kB = 52.5 kB for any weight 100-900, about 21 kB less than today and no more weight imports. Official `geist` 1.8.2 (Vercel) is 8 MB and Next-oriented: skip |

## 3. Platform features that replace libraries

"Chrome" = version in MDN browser-compat-data 8.1.7. "P" = probed in Chromium 156 headless-shell (a feature test, not a visual check). Everything marked 152 or lower is available in Toby's browser.

| Feature | Chrome | P | Brisk use | Replaces |
|---|---|---|---|---|
| Popover API (`popover`, `:popover-open`, `popovertarget`) | 114 | yes | ghost bar menus, Quality menu, host pill menu | custom open/close + click-outside code |
| CSS anchor positioning (`anchor-name`, `position-anchor`, `position-area`, `position-try-fallbacks`) | 125 / 129 / 128 | yes | GhostBar `.tip` tooltips, menus pinned to chips, sidebar active indicator glide (anchor to the active link) | @floating-ui for simple tooltips (Bits still uses it internally) |
| `popover="hint"` | 151 | yes | tooltips that do not close other popovers | |
| Interest invokers (`interestfor`) | 142, **origin trial / experimental only** | present in 156 | declarative hover/focus tooltips | **Not yet:** chromestatus lists it as an origin trial, Firefox neutral |
| `@starting-style` + `transition-behavior: allow-discrete` | 117 | yes | enter/exit for Quit/Restart dialogs (currently no animation), command menu exit, Install button reveal, toasts | anime.js / `forceMount` plumbing |
| `<dialog>` `closedby` (+ `requestClose()`), `command` / `commandfor` invokers | 134 / 135 | yes | confirm dialogs with light dismiss and no JS opener | simple Bits Dialogs (keep Bits where you need focus-return control) |
| Customizable `<select>` (`appearance: base-select`, `<selectedcontent>`) | 135 | yes | Settings selects (codec, preset) styled with our tokens | a custom Bits Select |
| View Transitions, same-document (`startViewTransition`, `view-transition-class`, types) | 111 / 125 / 125 | yes | Library filter reorder, History list to detail, tab swaps | FLIP libraries, `createLayout` (anime) |
| View Transitions, cross-document (`@view-transition { navigation: auto }`) | 126 | yes | not applicable: Brisk needs one persistent document (audio activation, module stores), which is why it uses Astro's `ClientRouter`. Docs say ClientRouter "will increasingly become unnecessary" | keep `ClientRouter` |
| Scroll-driven animations (`animation-timeline: view()/scroll()`, `timeline-scope`) | 115 / 116 | yes | already used for `.reveal` | anime `onScroll`, GSAP ScrollTrigger |
| `sibling-index()` / `sibling-count()` | 138 | yes | CSS stagger: `animation-delay: calc(sibling-index() * 40ms)` on bento cards | `stagger()` |
| `linear()` easing | 113 | yes | spring curves as design tokens (generate once) | spring libraries for CSS transitions |
| `@property` | 85 | n/a | tween `--p` of the host ring, `transition: --accent` colour morph, number counters via `counter-set` | JS number tween for visuals (not for text a11y) |
| `interpolate-size: allow-keywords` / `calc-size()` | 129 | yes | animate `height: auto` (shelf "Show all", disclosure) | JS height measurement |
| Container queries, style queries, `scroll-state()` queries | 105 / style queries [U] / 133 (experimental) | yes | card layouts per container; stuck sticky Settings strip styling | JS IntersectionObserver for "stuck" |
| `field-sizing: content` | 123 | yes | auto-growing inputs | JS autosize |
| CSS `if()` | 137 (experimental) | yes | token variants | Sass-style logic |
| `contrast-color()` | 147 | yes | readable text on the six accent swatches without per-accent overrides | manual `--on-accent` |
| `text-wrap: balance / pretty` | 114 / 117 | yes | Hero title, card copy | JS balancers |
| `corner-shape: squircle` | 139 (experimental) | yes | Linear/Apple-style cards on a Chrome-only app | SVG masks |
| `content-visibility: auto` + `contain-intrinsic-size` | 85 / 83 | n/a | 139-card shelves, long History lists | virtualisation libraries at this size |
| Scroll-triggered animations (`animation-trigger`) | 146 flag-level, spec "Proposed" | yes in 156 | one-shot entrance on scroll | not stable enough for 152 |
| `random()` | 156 | yes | none | |
| Sanitizer API / `Element.setHTML()` | 146 | yes | untrusted agent output | DOMPurify |
| `Temporal` | 144 | yes | session durations, History dates | date libraries, polyfills |
| Navigation API | 102 | yes | Astro's router does not require it; skip | |
| Declarative shadow DOM | 111 | n/a | none (Svelte islands) | |
| `CloseWatcher`, `Element.moveBefore()`, `ariaNotify()`, `scheduler.yield()` | 126 / 133 / 141 / 129 | yes | Esc handling for custom layers; reparent a node without losing state (useful for PiP); announce without live-region DOM; yield in long loops | |
| WebCodecs / WebTransport | 94 / 97 | (secure context needed in the probe) | already used | |
| WebGPU (`navigator.gpu`, `GPUDevice`) | 144 per MDN (Linux availability [U]) | not in headless | not needed, keep WebGL2 | |
| Gamepad `vibrationActuator`, `dual-rumble`; **trigger-rumble** (`playEffect('trigger-rumble')`) | 68; trigger-rumble 126 (chromestatus: enabled by default) | n/a | Controller test could add a trigger-rumble button [U: depends on pad/OS support]. Gamepad `touches` has no Chrome support | |
| Keyboard Lock (`navigator.keyboard.lock`) | 68 | n/a | already used in fullscreen | |
| Screen Wake Lock | 84 | needs secure context | **Stream:** request a `screen` lock while a stream is live (1 call, release on disconnect; re-request on visibility change). `localhost` counts as secure | a video/NoSleep hack |
| Web Locks | 69 | n/a | guard against two stream tabs fighting for one bridge | BroadcastChannel hacks |
| Document Picture-in-Picture | 116 | n/a | float the stats card or a mini stream; re-parenting the canvas could disturb the decoder path, so test latency first [U] | |
| Window Management (`getScreenDetails`) | 100 | n/a | choose which monitor to go fullscreen on | permission prompt |
| File System Access (`showOpenFilePicker`, `FileSystemObserver` 133) | 86 | n/a | Files module | |
| Compute Pressure (`PressureObserver`) | 125, origin-trial level per chromestatus | n/a | host load is already server-side; skip | |
| Async Clipboard (`ClipboardItem.supports`) | 121 | n/a | already used; use `supports('image/png')` | |
| Speculation Rules | 121 [U] | n/a | prefetch of other pages is irrelevant with `ClientRouter` | |

What would improve Brisk, in order:

1. **Library filter animation:** same-document View Transition with `view-transition-name` only on visible cards (not all 139), `view-transition-class` for one rule. Falls back to the current instant swap.
2. **Dialogs and menus:** `@starting-style` + `allow-discrete` gives Quit/Restart dialogs and the command menu an enter and exit with no JS; Bits exposes `data-starting-style`/`data-ending-style` for it.
3. **Tooltips and ghost-bar menus:** popover + anchor positioning removes JS positioning for the simple cases.
4. **Stagger and springs:** `sibling-index()` and a `linear()` spring token replace anime.js's two main selling points for Home.
5. **Select styling:** `appearance: base-select` for Settings selects.
6. **Stream:** Screen Wake Lock now; Document PiP only after a latency check. Nothing else here should run on /stream.
7. **Accent contrast:** `contrast-color()`.

## 4. What changed since mid-2026 (things you may not know)

- **Chrome release cadence:** two weeks from Chrome 153 (stable 2026-09-08; the blog post is dated 2026-03-03). Stable on Linux today: 156.0.8078.47 (2026-10-21). 154 landed 09-22. Chrome 153's release notes list 230 security fixes and an actively exploited V8 bug (secondary coverage, not checked against Google's page [U]). Toby's 152 predates all of it.
- **Node:** Node 24 entered maintenance on 2026-10-20; **Node 26 becomes LTS on 2026-10-28** (3 days); Node 27 alpha the same day. This box runs 26.11.1. Astro 7 needs >=22.23.4, Vitest 5 >=22.12. Node has no `Temporal` yet.
- **TypeScript 7.0** (native Go compiler) released 2026-06-18, 7.0.2 on npm (2026-07-08), 7.1 beta since 2026-07-08. Tooling peers have not caught up (section 2.1).
- **Vitest 5** (Oct 2026): Vite >=6.4 required, built-in Trace View, `toMatchScreenshot`, `vitest doctor`, fake timers mock `Temporal`. Several Browser Mode security advisories this summer.
- **Vite 8 / Rolldown 1.x:** Astro 7 runs on Vite 8 with Rolldown (confirmed in the lock: vite 8.3.5, rolldown 1.2.15). Three `server.fs` advisories fixed in 8.3.3. `tsdown` 0.23 (Rolldown-based library bundler) is the new tsup. Rust compiler (`@astrojs/compiler-rs`) and Sätteri Markdown are Astro 7's other swaps; `@astrojs/db` was removed as unmaintained.
- **SvelteKit 3** (2026-10-01) and **sv 1.0**; Svelte core is calm (5.57.x, parser performance work; no Svelte 6 mentioned). Official community add-on API. `@sveltejs/enhanced-img` 1.0.0.
- **New or newly relevant libraries:** `@tanstack/hotkeys` / `svelte-hotkeys` (0.x, Oct 2026); `@tanstack/svelte-table` v9 (Svelte 5); `@dnd-kit/dom` 0.5 (rewrite); `vitest-browser-svelte`; Ark UI Svelte 5.24; Sivir UI, Baby UI, FluentUI Svelte 0.4 (from Svelte's October post, not evaluated); `horizon-layout` (IDE-style panes); `svdevtools` (early Svelte devtools preview); `@anthropic-ai/sdk` reached 1.x (1.3.0 on 2026-10-23, relevant to the Agents module).
- **Deprecations:** `lucide-svelte` (use `@lucide/svelte`), `@astrojs/db`, Astro `TRANSITION_*` internals, `@lhci/cli` unmaintained since 2025-06, `libass-wasm` (use jassub), mp4-muxer (use mediabunny, already done), `@xterm/addon-canvas` (2024).
- **Mediabunny:** HLS read and write, parser hardening against hostile files, `@mediabunny/server`.
- **Package-name hazard:** `dash.js` (npm) is not dash.js (`dashjs`); its repository link is a 404.
- **GSAP is free,** under a proprietary licence (not MIT).
- **Platform:** Chrome 155 beta adds `symbols()`, `corner` shorthands, `random()` caching keys; `random()` itself is 156; `popover=hint` 151; customizable select is in Safari 27 and Firefox 149 now, so it is a cross-browser feature.

## 5. Astro 7, Svelte 5, Vite/Rolldown, TypeScript notes

- Astro 7.4.2 + @astrojs/svelte 9.1.1 + Svelte 5.57.2 + vite-plugin-svelte 7.3.1 + Vite 8.3.5 + Rolldown 1.2.15 form the working set in your lock. The build you saw reference Rolldown is Vite 8's default bundler.
- Breaking changes that matter to you: stricter HTML in `.astro` (the Rust compiler), `compressHTML: 'jsx'`, `src/fetch.ts` reserved name (do not create one), `markdown.processor` change (irrelevant, no Markdown content collections here).
- View transitions: `ClientRouter` is still the documented route; it is what keeps Brisk's single-document behaviour. Native cross-document transitions would reload the document and lose audio activation and in-memory stores, so they are not a replacement for you. Keep using `navigate()` from `astro:transitions/client`.
- TypeScript: stay on 6.0.3 for checking; pin it explicitly in devDependencies (today it is only a peer).
- Vitest 5 + Vite 8: supported; Vitest 5.1.0 is in beta.

## 6. Open questions for Toby

1. Is the box's Chrome 152 pinned on purpose (managed install)? If not, update; the Sanitizer, `contrast-color()` and `random()` need 146/147/156 and security fixes are four majors behind.
2. Do you want type checking at all (tsconfig + `svelte-check` in `npm test`)? It will surface a backlog of errors the first time.
3. Is `layerchart` 2.9.1 acceptable without a visual diff of History, or do you want a screenshot comparison first?
4. Component tests: Browser Mode (needs the full `playwright` package, which is a peer, and downloads or reuses a Chromium) or keep node-env tests only?
5. Agents output rendering: native Sanitizer + `marked`, or render Markdown straight to Svelte nodes (no HTML sinks, no sanitiser)?
6. Media player: hls.js (simple) versus mediabunny (WebCodecs, HLS read, fewer bytes if you already ship it, no UI)? And is LGPL jassub acceptable for ASS subtitles?
7. Terminal: is a Rust PTY endpoint on hostd planned? ghostty-web vs xterm.js only matters after that.
8. GSAP's licence text could not be fetched today (site maintenance); if GSAP ever matters, read `https://gsap.com/standard-license` yourself.

## 7. Method notes and unverified items

- Download counts are npm's last-7-days number on 2026-10-25. Bus-factor figures are npm maintainer counts and repo knowledge, not contributor analysis.
- Sizes were built with entries I wrote; import names for a few packages (TanStack, layerchart) are barrels and overstated. `svelte-streamdown` failed to build in the scratch project and is only judged from its dependency tree and the katex advisory.
- The Chromium probe ran on `about:blank`, so secure-context APIs (Wake Lock, Web Locks, WebCodecs, WebTransport, Document PiP, WebGPU) showed false or undefined; their versions come from MDN.
- [U]: GSAP licence terms; Svelte 5 peer range of svelte-dnd-action; oxlint's Svelte support; native HLS in Chrome; Chrome 153 security details; `interestfor` default status in 152; whether `animation-trigger` is on by default in 152; Document PiP effect on stream latency; style-query support version.
