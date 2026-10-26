# Research: anime.js feature map and where Brisk could use it (round 2)

Date 2026-10-25. Round 1 (`research/anime-opportunities.md`) asked "where does anime.js beat the CSS we have" and found 3 of 22 spots. Toby's reply was that there must be more in the library. This round surveys the whole library and tests the open questions for real. Nothing was changed in the repo: every experiment ran in `/tmp/claude-1000/anime-lab/` (Vite pages, one Astro page, `playwright-core` driving Chrome for Testing 156 headless shell). `web/dist` and `web/node_modules` were not written to; the live Brisk on :8443 was not touched. [U] = not verified from a primary source or by a test.

Tested version: **animejs 4.5.0**, published 2026-06-22, MIT (npm registry). `5.0.0-beta.2` exists under the `beta` tag; ignore it.

## TL;DR

- Round 1 was right about CSS-first, but it undersold the library. The best additions are small and shared: a **number tween**, a **glide helper** (sidebar pill, quality chips, tabs), a **chart draw-in**, and **FLIP for the Library grid** (`createLayout`, new in 4.3, tested and working with a keyed Svelte list).
- A cheap trick with no runtime cost: use anime only **offline** to generate the `linear()` spring easing (`waapi.convertEase(spring(...))` gives the string) and put it in `tokens.css`. CSS then gets real springs on hover lifts and the ghost bar, with no library on `/stream`.
- Three measured surprises that change the plan: (1) `waapi.animate({ x })` is **not** compositor-accelerated (Chrome trace: `compositeFailed`, unsupported property `--translateX`); `translate`/`scale`/`opacity` are, spring easing included. (2) `onScroll` keeps a **60 Hz rAF loop alive while the observer exists, even with no scrolling**; every other part is idle-clean. (3) `createLayout` rewrites inline styles on children (it drops an inline `border-color`, and a mid-flight `revert()` leaves `transition: none !important`).
- Importing `animejs` does **not** start the engine (0 rAF at import and at idle). A top-level import in a Svelte island survives an Astro static build. The three.js peers are optional. Leaked looping animations (no `revert()` on destroy) run forever: measured.
- Draggable works with mouse and touch (it uses mouse and touch events, not pointer events), throw-with-spring-release feels right, and a spring snap-back is one option (`snap: [0]`). It has no sort logic: drag-to-reorder is `createDraggable` plus our own index maths plus `createLayout`, about 40 kB gzip lazy and a real build.
- Verdict: 26 ideas below. 6 are tier 1 (do), 10 are tier 2 (build if wanted), 10 are tier 3 (CSS does it, or it hurts latency, a11y or taste). Nothing touches `/stream`.

## 1. Docs digest, feature by feature

Source: all 419 pages of `animejs.com/documentation` (fetched as HTML and read as text), `github.com/juliangarnier/anime` (cloned at depth 1: `src/`, `tests/suites/`, `examples/`), release notes 4.3.0 to 4.5.0 via `gh api`, npm registry. v4 sub-path imports exist for every module (`animejs/animation`, `/waapi`, `/layout`, ...), which is how we keep bundles small.

### animate() and the animation model
- `animate(targets, { prop: value, ...settings })`. Targets: selector, element(s), JS objects (tween any number, e.g. a counter), arrays. Properties: CSS, CSS transforms (`x`, `rotate`, or individual `translate`/`scale`), CSS variables, HTML and SVG attributes, JS object properties.
- Values: from-to arrays `[0, 100]`, relative `'+=50'`, `'*=.5'`, colours, unit conversion, `'var(--x)'` (must call `.refresh()` if the variable changes), function-based `(target, i, targets) => ...` (since 4.4 the third arg is the `targets` array, not `total`), keyframes (array, duration-based, percentage-based `'50%': {...}`), per-tween `ease/delay/duration/modifier`.
- `composition`: `'replace'` (default under 1000 targets), `'none'` (cheaper), `'blend'` (additive, good for retargeting a hover mid-flight, only for forward playback). `modifier` post-processes numbers (`utils.round(1)`).
- Playback: `loop`, `alternate`, `reversed`, `autoplay` (can take `onScroll()`), `frameRate`, `playbackRate`, `playbackEase`; methods `play/pause/reverse/restart/seek/complete/cancel/revert/refresh/stretch`; promise `.then()`. Gotcha: JS animations leave the **end value as an inline style** after completing; `revert()` or `utils.cleanInlineStyles(anim)` removes it. Measured below.

### stagger()
- `stagger(value | [a, b] range, { start, from, reversed, ease, grid, axis, modifier, use, total, jitter, seed })`. `from`: index, `'first' | 'center' | 'last' | 'random'`, or normalised `[x, y]`. `grid: [cols, rows]` or `grid: true` (auto-detect from element positions since 4.4; 3D since 4.5). `use: 'data-order'` for a custom order. Works for values, delays (time staggering), and timeline positions.
- Gotcha: `from: 'random'` is only reproducible with `seed` (4.5). Grid staggering on a responsive grid needs the grid shape at call time.

### Timeline
- `createTimeline({ defaults, loop, ... }).add(targets, params, position).add(timerParams).call(fn, pos).label('x', pos).sync(otherTimeline | waapiAnim, pos).set()`.
- Positions: absolute ms, `'+=100'`, `'-=100'`, `'*=.5'`, `'<'` previous end, `'<<'` previous start, `'<<+=250'`, labels, `stagger()`. Sync can include WAAPI animations (4.3.4 sets `persist` automatically).
- Gotcha: `add()` on a big timeline is slow with composition on; 4.3.0 added `composition` per `add`.

### Timer
- `createTimer({ duration, loop, onUpdate, ... })`: an `setInterval` that shares the engine clock and can be paused, reversed, seeked. Same playback options as animations. Good for a pausable poll countdown; a plain `setInterval` is fine for us.

### Animatable (createAnimatable)
- `const a = createAnimatable(el, { x: 400, y: 400, ease: 'out(3)' })`, then `a.x(120)` retargets with its own duration; `a.x()` reads. Designed for cursor following and anything that sets a value many times a second; internally reuses one animation (no new object per call). Only numbers or number arrays. Per-property `unit`, `duration`, `ease`, `modifier`. `revert()`.

### Draggable
- `createDraggable(target, { container, x/y: { snap, mapTo, modifier } | false, trigger, releaseEase (spring allowed), releaseMass/Stiffness/Damping, containerFriction, minVelocity/maxVelocity/velocityMultiplier, dragSpeed, scrollSpeed/scrollThreshold, cursor, onGrab/onDrag/onUpdate/onRelease/onSnap/onSettle/onResize/onAfterResize })`. Methods: `enable/disable/setX/setY/reset/stop/refresh/scrollInView/animateInView/revert`. Properties: `x, y, progressX, velocity, snapX...`.
- Gotchas: it listens to **mouse and touch events** (not pointer events) on the trigger and the document; it sets `touch-action: none` on the trigger (blocks page scrolling that starts on it); `setX/setY/reset()` move instantly (duration 0), so to spring back to the origin use `x: { snap: [0] }` with a spring `releaseEase`. No sort/drop logic.

### onScroll (ScrollObserver)
- `animate(el, { ..., autoplay: onScroll({ container, target, axis, enter, leave, sync, repeat, debug, onEnter/onLeave/onUpdate/onResize/onSyncComplete }) })`, or `.link(anim)`. Thresholds: `enter: 'bottom top'` (container position, target position), object form, `'top+=40'`, `'50%'`, `'min'`/`'max'`.
- `sync`: `true` (scrub, 1:1), a number 0-1 (smoothed, lower is laggier), an ease (eased scrub), or method names `'play pause'`, `'play reverse'`, `'enter leave'` lists. `container` can be any scrollable element.
- Gotcha (measured): a 60 Hz rAF loop runs while any observer exists. See check 8.

### Layout (createLayout), since 4.3.0
- `createLayout(root, { children: '.item', duration, delay, ease, properties: ['boxShadow'], enterFrom, leaveTo, swapAt, onBegin/onUpdate/onComplete })`. Flow: `layout.record()`, change DOM, `layout.animate()`; or `layout.update(() => { ...mutate... }, overrides)` which does all three and returns a Timeline.
- Handles: reorder (FLIP), grid/flex changes, `display` toggles, parent swaps, enter (`enterFrom`, default opacity 0), exit (default `leaveTo` opacity 0). Elements sharing a `data-layout-id` morph between each other when one is hidden (shared-element transitions, e.g. a button into a modal).
- Exit rule: elements must **stay in the DOM** (hidden via `display: none`/a class) during `update()`; afterwards you remove `layout.leaving` yourself. Elements removed by the framework are just gone (measured).
- Gotchas (docs and measured): root position is never animated (use the parent as root); SVG elements are excluded; descendants of tracked children fade at 50% when their size changes (add them to `children` or `swapAt: { opacity: 1 }`); `enterFrom/leaveTo/swapAt` do not accept `scale`/`x` shorthands, only a full `transform` string; during the animation each child gets inline `position:absolute`, explicit size, `transition: none !important`; the default `properties` set (`opacity, fontSize, color, backgroundColor, borderRadius, border, filter, clipPath`) are interpolated and reset from computed style.

### Scope (createScope)
- `createScope({ root, defaults, mediaQueries }).add(self => { ...create animations... return cleanupFn })`; everything created inside (animate, timeline, timer, animatable, draggable, onScroll, layout, nested scopes) is registered and reverted by `scope.revert()`. `self.matches.<name>` is the media query state; the constructor re-runs when a query changes. `add('name', fn)` registers methods callable later from event handlers (`scope.methods.name()`). `addOnce`. `data` is cleared on revert. Selector queries inside are limited to `root`.
- Gotcha: nothing is reverted automatically. The Shell remount means every component needs its own scope and a cleanup (Svelte `onDestroy`, or `{@attach}` returning the revert).

### SVG
- `svg.createDrawable(path)` returns a proxy with a `draw` property: `'0 0'` to `'0 1'` draws a line; `'.25 .75'` shows a window. It writes `stroke-dasharray/offset` (paint, not compositor); `vector-effect: non-scaling-stroke` makes it slow.
- `svg.morphTo(otherPath, precision = .33)` returns [from, to] `d` strings to tween; works for `path d`, `polyline/polygon points`. Shapes with different subpath counts still morph, through an interpolated in-between that looks odd (measured: a triangle to two bars).
- `svg.createMotionPath(path, offset)` returns `translateX/translateY/rotate` tween params that follow the path (measured: dot placed on the curve with correct tangent angle).

### Text
- `splitText(el, { lines, words, chars, accessible = true, includeSpaces, debug })`, each accepts `true`, a `{ class, wrap: 'clip'|'hidden', clone }` object or an HTML template. Waits for `document.fonts.ready`, re-splits on resize. `accessible: true` adds a visually-hidden full-text clone (measured: a 1px clipped span precedes the split spans), so text is **duplicated in the DOM** (find-in-page, copy). Nested elements are duplicated across lines.
- `scrambleText(...)` (new in 4.4) as an `innerHTML` tween: random-glyph reveal with `chars, from, cursor, seed, revealRate, settleRate...`. Decorative; see tier 3.

### Easings
- Names (`'out(3)'`, `'inOutExpo'`, `'outElastic(.8, 1.2)'`), `cubicBezier(x1,y1,x2,y2)`, `steps(n)`, `linear(0, '.5 50%', 1)`, `irregular(steps, randomness)` (jittery, for glitch-style motion), `spring({ bounce, duration })` (perceived, SwiftUI model, bounce -1..1, recommended -.5..+.5) or `spring({ mass, stiffness, damping, velocity })`. A spring **sets the animation duration** to its settling time. `ease` can be a function (4.3).
- `waapi.convertEase(fn)` turns any JS ease, spring included, into a CSS `linear(...)` string. Measured below.

### Utilities
- `utils.$` (querySelectorAll, scope-aware), `get/set/remove/cleanInlineStyles`, `round`, `clamp`, `snap`, `wrap`, `lerp`, `damp` (frame-rate independent lerp: good for smoothing a value toward a target), `mapRange`, `random/randomPick/shuffle/createSeededRandom`, `padStart/padEnd/roundPad`, `keepTime` (rebuild an animation with changed params without losing its playback time), `degToRad/radToDeg`. `round/clamp/snap/wrap` chain: `utils.clamp(0,100).round(1)`; most work as `modifier`.

### WAAPI module (waapi.animate)
- Same ergonomics on top of `Element.animate()`: multi-target, function-based values, individual transforms, default units, stagger, spring and JS eases (converted to `linear()`), `persist`, `.then()`. 3 kB claimed vs 10 kB for `animate` (measured below: 7.8 kB with stagger and spring). Docs say accelerated properties: `opacity, transform, translate, scale, rotate` (+ `clip-path`, `filter` in some browsers). CSS variables animate only when registered with `@property`/`CSS.registerProperty`.
- Gotcha (measured): its **`x`/`y`/`scale`-style shorthands are implemented through a hidden custom property and `transform: translateX(var(--translateX))`**. That is not compositor-accelerated. Pass the CSS property names `translate`, `scale`, `rotate`, `opacity` directly.

### Engine
- `engine.defaults` (duration, ease, ...), `engine.pauseOnDocumentHidden` (default true), `engine.timeUnit`, `engine.precision`, `engine.fps`, `engine.speed`, `engine.useDefaultMainLoop` (turn off and call `engine.update()` yourself), instance `priority`. Execution order = creation order unless `priority`. Set defaults once in `motion.ts`.

### Events, adapters, React page
- Only `onScroll` in `events`. Adapters (4.5): Three.js and `registerAdapter()` for non-DOM targets: irrelevant here. The "Using with React" page is the pattern to copy for Svelte (scope in mount, revert in cleanup). There is still no Svelte page.

### Repo notes
- `tests/suites/` has 33 suites (including `leaks`, `scope`, `waapi`, `draggables`, `scroll`); no Layout suite in the clone [U on whether it lives elsewhere]. `examples/` has 25 folders worth browsing: `auto-layout`, `draggable-*`, `animatable-follow-cursor`, `svg-line-drawing`, `text`, `onscroll-*`, `stagger`, `advanced-grid-staggering`. Release history shows layout fixes in 4.3.1, 4.3.2, 4.3.5 (inline-element regressions), 4.3.6 (layout + onScroll sync), so the layout module is young: pin and re-test on upgrades.
- Breaking changes landed in minors (4.4.0 transform order and `total` to `targets`, 4.5.0 colour blending). Pin the exact version.

## 2. Empirical results

Harness: Vite 8.3.5 + Svelte 5.57.2 + Astro 7.4.2 (Brisk's own installs, symlinked read-only), `playwright-core` 1.64.1, Chrome for Testing 156.0.8078.4 headless shell, animejs 4.5.0, scratch dir `/tmp/claude-1000/anime-lab/`. rAF counts come from a wrapper installed before the library is imported (verified: it counts 19 calls during a 400 ms animation).

| # | Question | Result | Evidence |
|---|---|---|---|
| 1 | `animate()` tweens a CSS variable colour (`--accent`)? | **Works**, with two catches. `animate(':root', {'--accent': '#fb7185'})` tweened; the midpoint was `rgb(190,183,177)`, a **muddy grey** (mint to rose blends through grey in the library's pseudo-linear RGB). It leaves `--accent: rgba(...)` as an **inline style on `<html>`, which would beat the `[data-accent]` CSS rule** until reverted. `utils.cleanInlineStyles` as `onComplete`, or `revert()`, removes it (tested). | mid and end colours read from computed style |
| 1b | A registered `@property` number (`--p`)? | **Works** in `animate()` (53.7 at 150 ms of 300, 100 at end). A registered `<color>` property also works (`rgb(197,160,145)` mid). `waapi.animate` on a registered `--q` works (24.99 mid). | computed style samples |
| 2 | Does importing animejs start the engine loop? | **No.** 0 rAF callbacks at import, 0 over 1.5 s idle after import, and 0 over 1 s after animations finished. `createTimeline`, `createDraggable`, `createAnimatable`, `createLayout`, paused `animate` are all idle-clean (0 rAF/s). **Exception: `onScroll`, see 8.** | rAF wrapper counts |
| 3 | `createLayout` with a keyed Svelte 5 list reorder? | **Works** with `layout.update(() => { items = ...; flushSync() })` (the `flushSync` matters so the DOM changes inside the callback). Reverse and shuffle of 12 cards: all 12 in flight at 120 ms of 400, final positions exact, also after two shuffles 50 ms apart (interrupt is safe). Adding an item: neighbours slide, the new card fades in. | positions sampled mid and end |
| 3b | Filtered/searched grid? | **Works two ways.** (a) Svelte removes non-matching cards from the DOM: survivors glide to new slots, removed cards just vanish (no exit anim). (b) Hiding via a `display:none` class inside `update()`: hidden cards fade (opacity 0.12 at 120 ms) and survivors glide. Use (b) when an exit matters. Un-filtering re-enters cards with the `enterFrom` fade. Not applicable to the virtualised shelf (cards mount while scrolling). | opacity and rect samples |
| 3c | Side effects of layout | During the animation each child has inline `position:absolute; width; height; translate; transition: none !important; display:block !important`. After completion it also **drops an inline `border-color`/`border` set by a Svelte `style:` binding** (all other inline props survived: `color`, `background`, `--vars`, `box-shadow`). A finished entered card keeps `translate: none` inline. | `getAttribute('style')` before/mid/after |
| 4 | `revert()` next to Svelte `style:` bindings; scope cleanup on destroy? | **Safe if the animated properties are not also Svelte-bound.** `translate`/`transform` animated by anime next to Svelte's `opacity/width/background` bindings: Svelte's updates still applied. **If both write the same property**, anime overwrites Svelte's write on every frame and `revert()` restores the value from *before* the animation, silently losing Svelte's newer value (tested: Svelte set 0.5 mid-animation, revert gave back 1). `scope.revert()` in `onDestroy` stops a `loop: true` animation: rAF goes 36 per 600 ms to 0. **Without cleanup the looping animation keeps running forever on a detached node** (36 per 600 ms after unmount). `layout.revert()`/scope revert mid-flight left `transition: none !important` inline on the cards. | rAF counts, style attributes |
| 5 | Top-level `import 'animejs'` breaks the Astro static build? | **No.** A Svelte island (`client:load`, so it is server-rendered during `astro build`) with `import { animate, createScope } from 'animejs'` at the top built fine (2 pages in 0.4 s); the animejs core landed in one 30 kB `animation.*.js` chunk. A `await import('animejs/waapi')` in a click handler tree-shook to an empty chunk. So the Node-side evaluation is safe; we lazy-import for bundle reasons only. | `astro build` output |
| 6 | Are `three`/`@types/three` peers optional? | **Yes**: `peerDependenciesMeta` marks both `optional: true`; `npm i animejs@4.5.0` installed only `animejs` (0 dependencies, 0 vulnerabilities). | registry JSON, `node_modules` listing |
| 7 | Draggable with Svelte, pointer, touch | **Works**, wrapped in a `{@attach}` that returns `d.revert()`. Mouse drag of 160 px right: element follows, on release a throw adds about 90 px of glide and settles in about 300 ms with `releaseEase: spring({stiffness:300, damping:30})`. Touch (CDP `Input.dispatchTouchEvent`) gives the same numbers. `x: { snap: 100 }, y: false` snaps to multiples of 100 and locks the axis. **Spring snap-back to origin** with `x/y: { snap: [0] }` and `spring({ bounce: .25, duration: 500 })`: 146,68 then 105,51 then 70,38 then 47,28 then 33,23 then 27,20 then 25,19 (overshoot 4 px past start) then 28,20 then 29,21 (home) in about 550 ms. `touch-action: none` is set on the handle; custom cursor only applies on `(pointer:fine)`. Trackpad = mouse events, so same path; real two-finger scrolling is untouched unless it starts on a handle [real trackpad U, no hardware here]. `onSettle` did not fire within 480 ms after a free throw [U why]. | pixel traces |
| 8 | `onScroll` scrub on a nested scroll container? | **Scrubbing works**: `container: '#sc'`, `sync: true` tracked opacity 0.20 at scrollTop 300, 0.66 at 500, 1.00 at 650; window scroll did not affect it. **Cost: with any `onScroll` observer alive, rAF ran 60/s continuously, even before any scroll** (62/s idle, window or container, scrub or play/pause modes). Revert dropped it to 1/s. The source has a 500 ms "wake ticker" that should put the scroll timers to sleep, but the loop stayed on in 4.5.0 [U root cause]. | rAF wrapper, 9 configurations |
| 9 | Per-frame main-thread cost | See table below. | CDP `Performance.getMetrics` deltas |
| 10 | `linear()` spring from anime's params; does `waapi.animate` take a spring in Chrome? | **Yes to both.** `waapi.convertEase(spring({ stiffness: 170, damping: 26 }).ease)` returns a `linear(0, 0.0072, 0.0266, ...)` string (about 788 chars). `waapi.animate(el, { translate: '300px', opacity: .3, ease: spring({ bounce: .2, duration: 600 }) })` produced a native animation with `linear(0 0%, 0.0063 1%, ...)` easing. **Chrome trace: no `compositeFailed` for `translate`+`opacity` with a spring; the same trace flagged `x` (`compositeFailed: 8192, unsupportedProperties: ["--translateX"]`).** The spring overrides the duration (bounce .2 / duration 500 settled at 920 ms). | CDP trace `Animation` events |

### Check 9 numbers (Chrome 156 headless, 3 runs averaged, 1.8 s window incl. element creation; idle baseline 2.9 ms)

| Scenario (40 elements, opacity + translate, 600 ms, 25 ms stagger) | Main-thread task ms | Script ms | Style ms | rAF callbacks |
|---|---|---|---|---|
| `animate()` (`y`) | 134 | 21.6 | 30 | 98 |
| `animate()` (`translate`) | 127 | 18.2 | 29 | 99 |
| `waapi.animate()` (`translate`) | 146 | 7.3 | 31.5 | 0 |
| `waapi.animate()` (`y`, the default form) | 174 | 6.9 | 55 | 0 |
| native `el.animate()` | 95 | 0.1 | 26 | 0 |
| CSS keyframes + `animation-delay` | 90 | 0 | 27 | 0 |

Reading: CSS and native WAAPI are the floor at about 90 ms (mostly the 40-node setup itself). `animate()` adds roughly 40 ms over 98 frames, about **0.4 ms per frame**. `waapi.animate` with `translate` costs about the same total but nothing per frame in script; the `x/y` form costs more because of the registered custom property. All are fine for 40 elements; the point is that CSS is free and anime is a small, bounded tax.

| Scenario (120 counters, 600 ms) | Task ms | Script | Layout | Style |
|---|---|---|---|---|
| 120 x `animate(obj, { onUpdate })` | 158 | 18.6 | 53.6 | 5.7 |
| one `animate()` over an array | 132 | 13.6 | 45.8 | 5.1 |
| hand-written rAF loop | 124 | 10.0 | 49.5 | 5.4 |
| CSS `@property` integer counter | 276 | 0 | 48.2 | 176.6 |

Reading: text changes are layout-bound (about 45 to 50 ms whatever drives them, so give number cells a fixed width and `tabular-nums`); anime is within 10 ms of a hand loop. The CSS `@property` counter is the **worst**, because every frame recalculates style. 120 counters is a stress test; Brisk needs about 10, which is under 0.5 ms per frame.

### Bundle sizes (our Vite, minified, gzip -9, each entry alone, 4.5.0)

| Entry | gzip |
|---|---|
| `animejs/waapi` + `spring` + `stagger` | 7.8 kB |
| `animate` + `svg` (drawable, morph, motionpath) | 12.9 kB |
| `createAnimatable` | 12.3 kB |
| `animate` + `scrambleText` | 13.4 kB |
| `createLayout` (pulls in timeline and animate) | 20.1 kB |
| from round 1: `animate`+`stagger` 15.2, + timeline/scope/spring 18.1, `createDraggable` 21.4, `splitText` 3.6 | |

Chunks share code, so layout plus animate plus draggable together is roughly 35 to 40 kB, not the sum.

## 3. Brisk ideas

Effort: S under 1 h, M a few hours, L a day or more. Risk: what can go wrong. Verdicts: **Do**, **Build** (worth it when Toby wants it), **Maybe**, **CSS** (anime is not the right tool), **No**. Every idea runs only off `/stream`, lazy-loaded, behind `motion.ts`.

### Tier 1: delightful and cheap

| # | Where | anime.js feature | What the user sees | Effort | Risk | Verdict |
|---|---|---|---|---|---|---|
| 1 | Host card temps and conic ring, Machines temps/uptime, Library widget count, Resume playtime | `animate()` on a plain object, `modifier: utils.round(1)`, `onUpdate` writes text; ring via registered `@property --p` | On every 10 s poll the temperature rolls from 62.1 to 63.4 and the ring sweeps instead of snapping, so a changed number draws the eye. | S | Cost is tiny (about 10 nodes). Do not tween when unchanged; write the final value for screen readers; fixed-width `tabular-nums` cells (layout is the cost). | **Do** |
| 2 | Spring easing tokens | `waapi.convertEase(spring({...}))` run **offline** (a node script or a one-off) to print `linear(...)` | Hover lifts, the ghost bar and panel slides gain a calm spring (no bounce, tiny overshoot) with zero JS at runtime and nothing loaded on `/stream`. | S | A generated string in `tokens.css` (about 800 chars per curve); keep one or two. Round 1 already preferred CSS `linear()`; this just removes guesswork. | **Do** |
| 3 | Sidebar active item, QuickActions quality chips, Settings tab underline | one `glide(el, toRect)` using `waapi.animate` on `translate` and `scaleX`; last index kept in module state | The highlight pill slides to the item you clicked instead of the background snapping, in three places with one helper. | S-M | Must place at the old spot on mount (Shell remounts). `translate`/`scale` only so it stays on the compositor. Overlaps motion-2's tab indicator: check that branch first. | **Do** |
| 4 | History session chart | `svg.createDrawable` on series paths, `animate` draw `'0 0'` to `'0 1'`, area fade | Open a session and the FPS and latency lines draw themselves in left to right, then the area fills. | S | Needs LayerChart's path selectors [U]; check its own `motion` prop first. Chunk is already lazy (65 kB), +13 kB. Paint-bound, but only once. | **Do** |
| 5 | Header play/pause/mute/connect icons (Icon.svelte, ghost bar excluded) | `svg.morphTo` + `animate` on `d`, 180 ms | Volume bars fold into a mute slash and Resume's triangle melts into a spinner dot, so state changes feel physical. | S-M | Measured: a triangle to two bars morphs through a lumpy middle; icons need hand-matched point counts (same subpath count). Not for the ghost bar (Stream route). | **Maybe** |
| 6 | Status dot (host pill, Machines) | `waapi.animate` ring: `scale` + `opacity` once on state change | When bawn goes from "ready" to "Sunshine down", the dot sends out one soft ring so you notice the change you did not cause. | S | A CSS `@keyframes` triggered by a class does the same; use anime only if triggered from the store. | **Maybe (CSS first)** |

### Tier 2: signature moments worth building

| # | Where | anime.js feature | What the user sees | Effort | Risk | Verdict |
|---|---|---|---|---|---|---|
| 7 | Library filter chips + search (non-virtualised grid / "Show all") | `createLayout` + `layout.update(() => { filter = ...; flushSync() })`, hide via class for fade-out, `leaveTo`/`enterFrom` fade+rise | Type "hol" and the grid reshuffles: the survivors slide into their new slots, the rest fade, then the grid is whole again when you clear it. | M | Tested and working (3, 3b). Not for the virtualised shelf (cards mount on scroll). Avoid inline `border` on cards; use class-based borders. 20 kB lazy. Young module: pin. | **Build** |
| 8 | Home bento arrival | `waapi.animate('.card', { opacity, translate, delay: stagger(40, { grid: true, from: 'first' }) })` on first mount only | The cards land in a diagonal wave from the host card instead of fading as one block. | S-M | motion-2 owns page/scroll motion and Home widgets mount late (async). CSS `animation-delay` with `sibling-index()` [U] may do it free. Decide after motion-2 merges. | **Maybe** |
| 9 | Home "Add a module" card to the picker | `createLayout` with `data-layout-id` on the card and the opened panel (the docs' "modal dialog animation") | The dashed ghost card swells into the module picker and collapses back, a real shared-element feel. | M-L | Needs picker built (v1 only has slots); Bits UI dialog portals complicate layout roots; hover state clashes. | **Build later** |
| 10 | Home card drag-to-reorder | `createDraggable` per card (handle), on release compute the nearest slot, `createLayout.update` to reorder, spring `releaseEase`, persist order in prefs | Grab a bento card by its handle, drag it over the grid, drop it and the neighbours slide aside while your card springs into place. | L | No sort logic in the library; cards have different spans (2x1, 1x2) so slot maths is real work. 40 kB lazy, touch-action none on handle, keyboard alternative required (a11y). Needs a product decision: does Toby want customisable Home? | **Build (if wanted)** |
| 11 | Library hero title | `splitText(h1, { words: { wrap: 'clip' } })` + stagger on `translate` | The "Continue where you left off" headline rises word by word out of a mask, once per visit. | S | 3.6 kB, but splits after `fonts.ready` (possible reflow), duplicates text in the DOM (hidden clone), heavier than the Linear taste unless used once. | **Maybe** |
| 12 | Connection path on Machines (bawn to laptop, "direct" or "relay") | `svg.createMotionPath` dot along a path; or CSS `offset-path` | A small packet dot travels the line between the two machine nodes, slower and amber on a relay. | M | A looping anime animation = 60 Hz rAF forever. CSS `offset-path` + `offset-distance` does it with no JS [U on compositing]. Pause when off-screen. | **CSS** (anime only for a dynamic path) |
| 13 | Machines status transitions | tween the `.dot` colour and uptime; `createLayout` when "Sunshine is down" swaps in a callout (parent swap) | When Sunshine drops, the dot cross-fades red, the uptime keeps counting and the callout grows in, with neighbours moving down. | M | Layout inside the card needs `children` selectors; CSS transition covers the dot. | **Maybe** |
| 14 | Library/History cover to detail (shared element) | `createLayout` with `data-layout-id` on the card cover and the header art | Click a session and its cover grows into the detail header. | L | Native View Transitions (Astro) can do it; History keeps state in pushState. | **Maybe (View Transitions first)** |
| 15 | Settings controller test result chips | `animate` stagger on the drift result chips and a count-up of the drift percentage | After "measure drift" the verdict chips pop in one after another and the percentage rolls up. | S | Diagnostic accuracy: only the result, never the live stick dot. | **Maybe** |
| 16 | Library first load | `waapi` stagger over the first ~8 `.slot`s | Skeletons dissolve into the first row of covers with a 30 ms wave. | S | Virtualiser remounts cards on scroll: flag "first batch". | **Maybe** |

### Tier 3: tempting but wrong for us

| # | Idea | Feature | Why not | Do instead |
|---|---|---|---|---|
| 17 | Pointer-following spotlight glow on cards | `createAnimatable` smoothing `--mx/--my` | Toby's steer (design doc section 4): "no mouse effect, no card shaders"; a cursor spotlight is exactly that. Animatable itself is idle-clean (12 kB gz). | Ask Toby before building; lift + border hover stays. |
| 18 | Scroll-scrubbed Library hero parallax | `onScroll({ sync: true })` | Measured: keeps a 60 Hz rAF loop alive while it exists, plus main-thread scroll listener, on the page with 139 games. | CSS `animation-timeline: scroll()`/`view()` (compositor), as `.reveal` already does. |
| 19 | Command-menu results animate on reorder (FLIP) | `createLayout` | Results re-rank on every keystroke; animation adds latency and flicker to a keyboard-driven flow (Linear does not). Layout also forces absolute positioning on rows. | None. Optional CSS fade for the panel. |
| 20 | Ghost bar spring and any `/stream` motion | spring | Anti-goal: nothing from the library may run or load on `/stream`. | Idea 2: offline-generated CSS `linear()` spring on the existing transform transition. |
| 21 | Toast stacking and swipe-to-dismiss | `createLayout`, `createDraggable` | Toasts mount on `/stream` (reconnect toast): must stay library-free. | Svelte `animate:flip` and `fly`. |
| 22 | Controller test stick dot smoothing / trail easing | `animatable`, `damp` | A diagnostic must show the raw reading; easing hides real drift and adds lag. | Leave the rAF loop alone. |
| 23 | Accent swatch colour morph | `animate('--accent')` | Measured: blends through muddy grey, and leaves an inline `--accent` on `<html>` that beats `[data-accent]`; every `color-mix` consumer repaints. | `@property --accent { syntax: '<color>' }` + `transition: --accent .3s` on `:root`. |
| 24 | Connect sequence timeline on the Resume card | `createTimeline` with labels | Adds time between click and stream start; the timeline cannot outlive navigation to `/stream`. | One instant state change (CSS). |
| 25 | `scrambleText` on host name / game title | `scrambleText` | Decorative hacker effect, wrong tone for "calm"; text churn also hurts screen readers. | None. |
| 26 | Perpetual ambient loops (breathing dot, floating cards) and drag-to-scroll the shelf | `loop: true`, `createDraggable` | Loops hold the rAF loop forever and leak if not reverted (measured). Draggable on the shelf replaces native trackpad inertia and `scroll-snap`. | CSS `animation`, native scroll. |

## 4. Recommended motion pass (build order)

Shared setup first: **`web/src/ui/motion.ts`**.
- **Lazy, sub-path imports only.** Memoised `loadMotion()` that does `import('animejs/waapi')`, `import('animejs/animation')`, `import('animejs/layout')` etc. on demand; never the bare `animejs` entry; never imported from `Shell.svelte`, `Layout.astro` or any shared chunk. Pin `"animejs": "4.5.0"` (exact). Check after `astro build` that the Stream module's import closure contains no animejs chunk.
- **Stream guard.** Helpers are no-ops (return a finished handle) when the path starts with `/stream`, and no module mounted on `/stream` (Toasts, CommandMenu, QuitDialog, Sidebar) calls them.
- **Reduced motion.** One `matchMedia('(prefers-reduced-motion: reduce)')`; also `createScope({ mediaQueries: { reduce: '(prefers-reduced-motion: reduce)' } })` (tested: `self.matches.reduce` is true under emulation). Helpers write the end state immediately.
- **Scope per component.** `useMotion(root)` returns `{ scope }` created in `onMount`/`{@attach}` and reverted in cleanup; module state may hold plain data (last sidebar index) but never an animation.
- **Rules from the measurements.** (a) Use `translate`, `scale`, `rotate`, `opacity` in `waapi.animate`, never `x/y`. (b) Never animate a property a Svelte `style:` binding also writes; use `onComplete: utils.cleanInlineStyles` or `revert()` for tweened CSS variables. (c) No `onScroll`, no `loop: true` (rAF), no cards with inline `border` inside a `createLayout` root. (d) Tokens: `dur = { tab: 180, page: 260, slow: 450 }`, the `--ease` cubic-bezier, and the one generated spring from idea 2; no bounce above 0.25.

Build order (each step is shippable and reversible):
1. `motion.ts` + a vitest that asserts no-op on `/stream`, reduced motion end-state, and scope revert; a build check for the Stream import closure.
2. Spring tokens (idea 2): offline script, CSS only. Zero risk.
3. Stat tween (1) on HostCard, then Machines and Library widget.
4. Glide helper (3): sidebar first, then QuickActions chips and Settings tabs (after checking motion-2).
5. Chart draw-in (4).
6. Library grid FLIP (7); add `data-layout-id` shared elements later for 9 and 14.
7. Optional signature work, each its own decision: icon morphs (5), Home arrival (8), hero split text (11), Home drag-to-reorder (10).

Leave to CSS: hover lifts and presses (add the generated spring), page/tab/scroll reveal (motion-2), dialog and menu enter/exit (`data-state`, `@starting-style`), status dot colour and ping, accent colour (`@property`), skeleton shimmer, toasts (`fly`, `animate:flip`), the connection-path dot (`offset-path`), Settings underline, the whole `/stream` module, and the controller test's live loop.

## 5. Open items

- LayerChart path selectors and whether its `motion` prop already draws lines in [U].
- Why `onScroll` never sleeps (4.5.0). Worth a GitHub issue; until fixed treat it as unusable for idle pages.
- Whether `offset-distance` runs on the compositor in Chrome 156 [U].
- Real trackpad behaviour with draggable [U, no hardware]; the library uses mouse events, which Chrome also fires for trackpad drags.
- Layout `border` clobbering is documented only by the default `properties` set in source, not the docs.
