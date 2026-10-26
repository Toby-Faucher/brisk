# Research: where anime.js would help the Brisk web app

Date 2026-10-25. Read-only audit: no repo code changed, nothing installed in the repo, `web/dist` only listed and gzipped (no build). Library facts come from primary sources: `registry.npmjs.org/animejs`, the 4.5.0 tarball (`npm pack` equivalent, unpacked in `/tmp/claude-1000/`, deleted afterwards), the GitHub repo and releases (`gh api`), and `animejs.com/documentation/*` and `bits-ui.com/docs` read through WebFetch (a small model summarises, so exact wording could be off). The repo's `CHANGELOG.md` does not exist at the paths tried (404); GitHub release notes were used instead. [U] = not verified from a primary source.

## TL;DR

- **Verdict:** adopt anime.js narrowly, if at all. Chrome only plus the Chrome-new CSS features (`@starting-style`, `linear()` easing, `@property`, view timelines, `sibling-index()` [U]) already cover most of the "alive" motion. Anime.js earns its bytes only for: tweened numbers, a few timelines/choreographies, SVG draw-in, optional spring physics where interruption matters, and later `createDraggable`.
- **Cost:** about 15 kB gzipped for `animate`+`stagger`, 18 kB with timeline/scope/spring, 8 kB for the `waapi` flavour alone (measured below). The Shell chunk is 9 kB gzipped today, so it must be a lazy import, never static.
- **Biggest trap:** the global reduced-motion rule in `base.css` (`animation: none !important; transition: none !important`) does not stop anime.js. Everything needs an explicit guard in one `motion.ts`.
- **Second trap:** many elements have CSS `transition: transform` (Card, GameCard, sidebar `.nav`). If anime.js writes inline `transform` on them every frame, the CSS transition re-smooths each write and the motion lags. Use `translate`/`scale`/`opacity` (the `waapi` module uses individual transform properties) or leave those elements to CSS.
- **Stream route:** nothing from this document touches `/stream`. Section 6 lists what stays out.

## 1. The library (primary sources)

| Item | Finding | Source |
|---|---|---|
| Current stable | **4.5.0**, published 2026-06-22. Earlier: 4.4.0 2026-04-29, 4.3.0 2026-01-20, 4.2.2 2025-10-07 | npm registry `time` |
| Pre-release | `5.0.0-beta.1` (2026-08-05), `5.0.0-beta.2` (2026-08-17) under dist-tag `beta`. Do not use; v4 stays `latest` | npm registry |
| Licence | **MIT** (npm, LICENSE.md, GitHub). Repo `juliangarnier/anime`, about 73.8k stars, last push 2026-08-21 | npm, `gh api` |
| Dependencies | none at runtime. `peerDependencies` lists `three` and `@types/three` (for the Three.js adapter only; I did not check whether they are marked optional) [U] | npm registry |
| Package | ESM + CJS, ships types, `sideEffects` only for `adapters/**`, so tree-shakeable. Unpacked size 2,126,307 bytes (includes the 408 kB unminified and 118 kB minified all-in bundles, which we would not use) | npm registry, tarball |
| Sub-path exports | `animejs`, `/svg`, `/text`, `/scope`, `/timer`, `/utils`, `/waapi`, `/engine`, `/events`, `/layout`, `/easings`, `/timeline`, `/animation`, `/draggable`, `/animatable` | package.json `exports` |
| v4 API names | `animate`, `createTimeline`, `createTimer`, `createAnimatable`, `createDraggable`, `createScope`, `onScroll`, `createLayout`, `waapi.animate`, `utils.stagger`, `createSpring`, `svg.createDrawable/morphTo/createMotionPath`, `splitText`, `scrambleText` | tarball `dist/modules/index.js`, docs index |
| Recent changes | 4.5.0 (2026-06-22): adapters + Three.js adapter, 3D stagger grids, `jitter`/`seed`. **Breaking in 4.5.0:** transforms now render in a fixed order (translate, rotate, scale, skew); `matrix()` can no longer be animated; function-based value and stagger callbacks get `targets` array instead of `total`. Colour animation now blends in pseudo-linear RGB. Minor releases have broken things, so pin an exact version | GitHub release notes |

### Measured size (our bundler, tree-shaken, minified, gzip -9)

Built with the repo's own Vite from `/tmp`, importing from the 4.5.0 tarball; one entry per feature set. These are the cost if that set is the only anime.js code on the page.

| Feature set | min | gzip |
|---|---|---|
| `animate` + `stagger` | 45.6 kB | **15.2 kB** |
| + `createTimeline`, `createScope`, `createSpring` | 56.6 kB | **18.1 kB** |
| + `onScroll` | 72.6 kB | **22.8 kB** |
| `waapi.animate` + `stagger` (alone) | 21.7 kB | **7.9 kB** |
| `animate` + `createDrawable` + `stagger` | 46.7 kB | **15.7 kB** |
| `createDraggable` | 72.5 kB | 21.4 kB |
| `splitText` | 9.3 kB | 3.6 kB |

For scale, from `web/dist` today (gzip -9): Shell chunk 9.0 kB, svelte runtime `client.*.js` 22.1 kB, Settings 13.4 kB, Home 6.6 kB, Library 4.7 kB, History 3.2 kB, Stream 14.1 kB, the lazy `Charts` chunk 65.1 kB (238 kB raw), `Layout.css` 56.5 kB (88 kB raw). `dist` is 1.9 MB total. So `animate` alone is more than the Shell chunk; the `waapi` entry is the cheap one.

### Does it work with Svelte 5?

- The docs have a "Using with React" page and a Three.js adapter. **There is no Svelte page** (docs index). The library is framework-free, so it works; the lifecycle pattern is the same as React's:
  - React example: `scope.current = createScope({ root }).add(self => { ... })` in `useEffect`, cleanup `return () => scope.current.revert()`. `self.add('name', fn)` registers methods callable later from event handlers.
  - Svelte 5 equivalent: create the scope in `onMount` or in a `{@attach}` (Shelf already uses `{@attach track}` with a cleanup return) and call `scope.revert()` in the cleanup. `createScope` takes `root` (limits selector queries), `defaults`, `mediaQueries`. [Scope docs]
- SSR: `engine.js` picks `requestAnimationFrame` vs `setImmediate` by an `isBrowser` check, so import-time looks SSR-safe, but I did not test an Astro SSR build [U]. We would lazy-import inside `onMount` anyway.

### Reduced motion

- **No built-in global honouring.** `grep -ri "reduced-motion"` over the whole `dist/modules` source finds nothing. The only support is manual: `createScope({ mediaQueries: { reduce: '(prefers-reduced-motion)' } })`, then read `self.matches.reduce` inside `.add(self => ...)`, as the Scope docs example does (it sets `duration: reduceMotion ? 0 : ...`). The doc example uses the bare `(prefers-reduced-motion)` form; I would write `(prefers-reduced-motion: reduce)` [U on any difference].
- Our CSS kill-switch only stops CSS animations and transitions. It does **not** cancel Web Animations created by `element.animate()` (anime's `waapi`), nor inline-style writes from JS. So a guard is mandatory.
- Our `motion.ts` should make every helper a no-op or an instant `set` under reduced motion, so call sites do not branch.

### Compositor (WAAPI) versus main thread

- `animate()` (the default) is **JS-driven**: a shared `requestAnimationFrame` loop in `engine` writes inline styles every frame (source: `engine.js`, tick method is `requestAnimationFrame`; it also listens to `visibilitychange`, and has a `pauseOnDocumentHidden` parameter per the docs). Layout and paint work for non-transform properties happens on the main thread. No compositor offload.
- `waapi.animate()` wraps the native Web Animations API (`element.animate`). Per the docs: opacity, transform, translate, scale and rotate are accelerated "in all major browsers"; clip-path and filter in some. "Off the main thread" keeps them smooth when the main thread is busy. **Custom or JS easings can prevent acceleration** (docs name Safari's missing `linear()` support and custom power eases like `out(3)`); whether Chrome accelerates anime's spring-as-`linear()` easing I could not verify [U]. It adds sensible defaults, multi-target, individual CSS transforms, function-based values, springs and `onScroll` linking, with some API differences from native (`loop`/`alternate`/`ease` instead of `iterations`/`direction`/`easing`). Size 7.9 kB gzip vs 15.2 kB for `animate`.
- `onScroll`: the source adds a `scroll` listener and a `ResizeObserver` (`events/scroll.js`), so it is main-thread, not CSS scroll timelines. Our current `.reveal` (CSS `animation-timeline: view()`) is strictly cheaper; anime's `onScroll` would only be worth it for things CSS cannot do.

### Bits UI interaction (docs)

- `Dialog.Content`/`Overlay` expose `data-state="open|closed"`, plus `data-starting-style` on the first frame and `data-ending-style` "while closing before unmount", so CSS-only enter and exit works.
- For anime.js (or Svelte transitions) use **`forceMount`** with the `child` snippet and `{#if open}` around the element, spreading `props`; the docs describe `forceMount` as "useful if you want to use Svelte transitions or another animation library". With `preventScroll` set `restoreScrollDelay` longer than the transition.
- The docs do not say Bits waits for a JS-driven animation before unmounting; assume it does not, unless it is a CSS animation/transition [U]. `onOpenChangeComplete` fires "after ... all animations have completed".
- Conclusion: for dialogs and the command menu, CSS (`data-state`, `@starting-style`) is the cheap exit hook. Anime.js exits need `forceMount` plumbing in three places for little gain.

## 2. What the app does now (motion)

From `docs/ui-notes.md` Motion and the code:

- CSS only today: page enter (`.page-in` on Shell `<main>`, 260 ms, opacity + `translate`), Settings tab panels and History list/detail swap (`.swap-in`, 180 ms), scroll reveal (`.reveal` on Card, Library hero and shelf headers, view timeline), `pulse` on the status `.dot`, skeleton `shimmer`, command menu `cm-in`, hover lifts (Card `translateY(-3px)`, GameCard `translateY(-5px) scale(1.02)`, sidebar `translateX(2px)`).
- Svelte: Toasts use `transition:fly` (300 ms). Settings tab underline is a CSS transition on `transform`/`width`, positioned by measuring in JS.
- Not animated: Quit and Restart dialogs (`.rs` has no animation at all), game card Install button, accent change, host/machine state changes, charts, controller test (except its own rAF loop), the cards while they mount and unmount in the virtualised shelf.
- Branch `motion-2`: `git log motion-2` shows the same head as `main` (merge of `motion`), so its work is uncommitted elsewhere; I could not see it. Anything labelled "replaces" below must be checked against that branch before work starts.
- Design constraints: `docs/ui-design.md` section 2 anti-goal (no animation or shader cost on the stream screen), section 4 (lively but tasteful, Linear and Emil Kowalski taste, "springy hover lifts, sliding panels", reduced motion honoured). Emil Kowalski's own guidance is short durations (about 150 to 300 ms), no animation on keyboard-driven high-frequency actions, and no bounce; I am applying that from general knowledge, not from a fetched source [U].

## 3. Opportunities

Effort: S under 1 h, M a few hours, L a day or more. Cost column: "main" = JS on the main thread, "comp" = compositor-friendly.

| # | Where / screen | Today | Anime.js would | Benefit | Cost / risk | Effort | Verdict |
|---|---|---|---|---|---|---|---|
| 1 | **Sidebar active indicator** (Sidebar.svelte). The active item gets a background + inset ring | Background snaps between items. The Shell island remounts on every navigation, so any in-component state is lost | One absolutely positioned indicator; remember last index in a module-level variable (module state survives, as Layout.astro notes), place it at the old spot on mount, then `waapi.animate` `translate` (and height) to the new item with a short spring/`out` ease | Navigation feels continuous; the one place a "shared element" glide is visible on every click | comp with `waapi` (translate only). Needs measure on mount, fonts-ready. Keep `.nav` hover `transform` untouched (use `translate` on indicator only). CSS-only version (a transition from stored position) is possible; anime adds spring and cleanup | S to M | **Do** |
| 2 | **Stat values**: Host card temps and conic ring (HostCard.svelte), Machines `dd` temps/uptime, Library widget count, Resume playtime | Values replace instantly on each 10 s poll; the ring `--p` jumps | `animate` on a plain object with `onUpdate` writing text (`utils.round`), 400 to 600 ms `out`. Ring: tween `--p` (needs `@property --p` or set via style each frame) | Numbers read as alive, changes are noticed instead of silently swapped; the only clean CSS-impossible win | main, tiny (a handful of nodes, every 10 s). Must not tween on every poll if unchanged. Screen readers: write final value to an `aria-label`/live text, tween only a visual copy. Under reduced motion set instantly | S | **Do** |
| 3 | **Home bento intro** (Home.svelte, Card.svelte `.reveal`) | Cards use the view-timeline `.reveal`; all above-the-fold cards rise at once because they enter together. Home layout shifts about 0.06 when widgets mount (ui-notes) | One `waapi.animate` over `.card` with `delay: stagger(40)`, `opacity` + `translate`, on first mount only, instead of the CSS reveal for above-the-fold cards. Keep CSS `.reveal` for below-the-fold | Choreographed arrival instead of a block fade. **Replaces** the CSS reveal on Home only | comp (waapi). The late widget mount (async `widget()` imports) means stagger must run after widgets resolve or accept the layout shift. A pure CSS stagger (`animation-delay: calc(var(--i) * 40ms)` or `sibling-index()` [U]) gets 90% of this with zero JS | S | **Maybe** (try CSS first) |
| 4 | **History chart draw-in** (Charts.svelte, session detail) | Charts appear fully drawn after the lazy chunk loads | `svg.createDrawable` on each series path, 700 ms `out`, plus area fade. Selecting LayerChart's rendered `<path>` needs its class names (`.lc-*`) [U]. LayerChart may offer its own `motion` prop for tweened/spring updates [U]; check before adding a second animation system | The one screen where a data reveal reads as polish | main (stroke-dashoffset is paint, not compositor). Chart chunk already 65 kB gzip, so +15.7 kB there is not a regression for other pages. CSS alternative: `pathLength="1"` plus `stroke-dasharray` animation, no JS | M | **Maybe** |
| 5 | **Library first load: skeleton to content** (Library.svelte `.skel`, Shelf) | Skeleton shimmer, then shelves and cards pop in. Cards are virtualised and "do not animate" (ui-notes) | On the first data arrival only, `waapi.animate` the first visible ~8 `.slot`s with `opacity`/`translate` stagger 30 ms; later mount/unmount while scrolling stays untouched (mark by a first-batch flag) | Removes the empty-to-full pop. Hero and shelf headers already have `.reveal` | comp. Risk: the virtualiser remounts cards on scroll, so the animation must key off "first render of this shelf" not mount. Hover owns `transform`, so use `translate`. CSS alternative: `@starting-style` on `.slot` | M | **Maybe** |
| 6 | **Library hero** (Hero.svelte) text/cover entrance, split title | `.reveal` rise | `splitText` words/lines stagger on the 28 px title | Cute, but heavier than the Linear taste asks for | +3.6 kB, layout work from splitting text, a11y duplicates text | S | **Skip** |
| 7 | **Game card hover/select** (GameCard.svelte lift, `.play` pop, `.over` Install) | CSS transitions, interruptible | Spring via `waapi` or `createSpring` | "Springy" in ui-design section 4, but JS must reproduce CSS's free interruption (hover in/out mid-flight) and would fight `transition: transform` | CSS `linear()` spring easing in a token gives the same curve with no JS. Install button appearance is a plain `@starting-style` fade | M | **Skip** (use CSS) |
| 8 | **Bento Card hover/press** (Card.svelte) | `translateY(-3px)` transition | same as 7 | same | same | S | **Skip** (use CSS) |
| 9 | **Command menu open/close, list stagger** (CommandMenu.svelte) | `cm-in` CSS animation on open, no exit, list static | Exit via `forceMount` and a timeline; stagger rows | Keyboard-driven, high-frequency, so staggering slows the user; Linear does not stagger. Exit fade is the only real gain | CSS `data-ending-style` does the exit. Rows are re-ranked on every keystroke (`rank(query, items)`), animating them would flicker | M | **Skip** (CSS exit if wanted) |
| 10 | **Quit and Restart dialogs** (QuitDialog.svelte, Machines.svelte) | No enter/exit at all | `forceMount` + anime enter/exit | Real gap, but trivial | CSS `data-state`/`@starting-style` plus `translate`/`opacity` fixes it, no library, no portal plumbing | S | **Skip** (do it in CSS) |
| 11 | **Toasts** (Toasts.svelte) | Svelte `fly` 300 ms, stack reflows abruptly when one leaves | Anime stack spring; swipe-to-dismiss with `createDraggable` | Reflow smoothness only | Svelte `animate:flip` on the keyed `#each` solves the reflow for free. Toasts also appear on `/stream` (reconnect toast), so keep them library-free | S | **Skip** |
| 12 | **Settings tab indicator** (Settings.svelte `.ind`) | CSS transition on `transform` + `width`, measured in JS | Spring slide | Marginal | `width` transition is layout; better as `scaleX` in CSS. No library needed | S | **Skip** |
| 13 | **Accent swatch change** (Settings, tokens.css) | `data-accent` swaps `--accent`; everything flips at once | `animate(':root', {'--accent': ...})` [U on CSS-variable colour tweening in 4.5] | Soft colour morph | CSS `@property --accent { syntax: '<color>'; inherits: true }` + `transition: --accent .3s` on `:root` does it. Many `color-mix` consumers repaint for 300 ms, fine for a rare action. Smoke canvas takes the colour via a getter, unaffected | S | **Skip** (CSS) |
| 14 | **Host pill / Machines state change** (HostPill.svelte, Machines.svelte `.dot`, "Sunshine is down" swap) | `.dot` changes colour without a transition; text swaps | One-shot attention ping on change | Clarity: a state change you did not cause is currently easy to miss | A CSS `transition: background` on `.dot` plus a short `@keyframes` ping triggered by a changing class covers it. Anime only if the ping must be triggered imperatively on store change | S | **Maybe** |
| 15 | **Controller test** (ControllerTest.svelte, PadDiagram) stick dot/trail, button pulse, drift progress | One `requestAnimationFrame` loop reads the pad and writes SVG; trail points in state | Ease the dot, tween the drift percentage, pulse buttons | A diagnostic tool must show the raw reading; easing the dot hides real drift and adds lag. Drift result chips could stagger in | Raw data path stays untouched. Result chips only: `animate` stagger, trivial; button press pulse is CSS `:active`/class | S | **Skip** (maybe a tiny stagger for the drift result later) |
| 16 | **Route-to-route transitions** (ClientRouter, Shell `main.page-in`) | CSS page-in only; no exit | Exit animation in `astro:before-swap`, or shared-element via `createLayout` (FLIP) | Exit adds latency to every navigation, which this app is meant to avoid. Native View Transitions (Astro's ClientRouter already uses them) cover shared elements in Chrome | **Replaces** `.page-in` if done; high risk: Shell remount, `/stream` exclusion, `position: fixed` trapped by a `translate` on `<main>` (noted in ui-notes) | L | **Skip** |
| 17 | **History list to detail swap**, cover grows into the header | `swap-in` rise 180 ms | `createLayout` FLIP between card cover and header | Nice shared-element feel | `createLayout` is new (module exists in 4.5; behaviour unverified [U]); native View Transitions could do it. Touches pushState handling in History | L | **Skip** for now |
| 18 | **Library filter / "Show all" grid reorder** (Library.svelte, Shelf grid mode) | Items appear/disappear instantly | `createLayout` or FLIP on the grid when the filter changes | Spatial continuity when filtering | Only the non-virtualised grid mode could be animated; the virtualised row cannot. Moderate value | M | **Maybe** (later) |
| 19 | **Skeleton shimmer** (Library `.skel`, `@keyframes shimmer`) | Animates `background-position`: repaints every frame | Not an anime job: a CSS fix (translate a gradient pseudo-element, compositor) | Cheaper paint | CSS only | S | **Leave to CSS** (note for motion-2) |
| 20 | **Shelf chevrons** (Shelf.svelte `scrollBy` smooth) | Native smooth scroll | Eased or spring scroll | Native is already compositor-driven and interruptible | Reinventing it in JS adds main-thread cost | S | **Skip** |
| 21 | **Icon micro-interactions** (Icon.svelte, `{@html}` paths) | Static | `morphTo` for vol to mute, play-nudge on hover | Cute | `morphTo` needs compatible paths, which these are not; hover nudges are CSS | S | **Skip** |
| 22 | **Drag**: reorder shelves, drag a toast, drag the stats card | None | `createDraggable` (21 kB gzip) | No feature needs it today | Only if a drag feature is designed; the stats card is on the stream | L | **Skip** |

## 4. Recommended first batch of 5

Ordered by value for least risk. Items 1, 2 and 5 below genuinely need JS; 3 and 4 have CSS fallbacks, so check them first and add anime.js only if they fall short.

1. **`motion.ts` module** (no visible change): lazy import, reduced-motion guard, scope/cleanup, tokens (section 5). Everything else depends on it.
2. **Stat tween** (opp. 2): HostCard temps/ring first, then Machines and Library widget. The only effect CSS cannot make.
3. **Sidebar indicator glide** (opp. 1): visible on every navigation, `waapi` translate only.
4. **Home bento stagger** (opp. 3): first-mount stagger replacing the CSS reveal above the fold; try CSS `animation-delay` first; coordinate with `motion-2`.
5. **History chart draw-in** (opp. 4): one place where a library's SVG helpers beat hand-written CSS, and the chart chunk is already lazy.

## 5. Shared setup: a single `web/src/ui/motion.ts`

- **Lazy only.** `export const loadMotion = () => import('animejs')` (or `animejs/waapi` plus `animejs/scope` when only those are used), memoised. Never imported statically from `Shell.svelte`, `Layout.astro` or any shared chunk. Verify after a build that `dist/stream/index.html`'s module graph and `Stream.*.js` import closure contain no animejs chunk, and that a Rollup/Vite chunk of its own is created.
- **Stream guard.** `motion.ts` returns no-op helpers when `location.pathname` starts with `/stream` (same test as Shell's `.page-in`). Even so, do not call it from `Toasts`, `CommandMenu`, `QuitDialog`, `Dialog` code that is mounted on `/stream`.
- **Reduced motion.** One `matchMedia('(prefers-reduced-motion: reduce)')` (listen for change). Helpers: `enter(el, opts)`, `tweenNumber(from, to, onUpdate)`, `glide(el, to)`; when reduced, apply the end state immediately and return a finished handle. Also pass it as `mediaQueries.reduce` to `createScope` for any raw use.
- **Scope and cleanup.** The Shell island remounts on every ClientRouter navigation, so create `createScope({ root })` per mounted component inside `onMount` or a `{@attach}`, and `scope.revert()` in the cleanup (the same pattern Shelf's `track` attach and CommandMenu's `onMount` already use). Never keep animations in module state across navigations; module state is fine for plain data such as the last sidebar index.
- **Tokens that match `tokens.css`.** Export `dur = { page: 260, tab: 180, quick: 140, slow: 450 }`, `rise = 8`, and an easing equal to `--ease: cubic-bezier(.2, .8, .2, 1)` (anime accepts `cubicBezier(.2, .8, .2, 1)`, from the `easings` export). Read them from CSS custom properties at runtime (`getComputedStyle`) where practical, so CSS and JS cannot drift. One spring preset only (for example `createSpring({ stiffness: 170, damping: 26 })` [U on parameter names in v4]), no bounce.
- **Properties.** Only `opacity` and individual `translate`/`scale`, via `waapi` where possible. Never `transform` on elements that have a CSS hover transform or `transition: transform`. Never `width`/`height`/`top`.
- **Pin the version exactly** (`"animejs": "4.5.0"`, no caret) because 4.5.0 shipped breaking changes in a minor release.

## 6. Leave as CSS

- Hover lifts and press feedback (Card, GameCard, sidebar, buttons): interruptible for free; if more spring is wanted use a `linear()` easing token.
- Page rise, tab rise, scroll reveal on below-the-fold cards: compositor and scroll-driven, cheaper than any JS.
- Dialog, overlay and command-menu enter/exit: `data-state`, `data-starting-style`, `data-ending-style`.
- Status `.dot` colour and pulse, accent colour (`@property`), skeleton shimmer (fix to a translating pseudo-element), list item `@starting-style` fades.
- Toasts: Svelte `fly` and `animate:flip`.
- Settings tab underline.

### Must stay out of anime.js (stream route, latency-measured)

- `web/src/modules/stream/*` entirely, especially: `GhostBar.svelte` (slide uses CSS `transform .45s`; any spring there must be CSS `linear()`), `StatsCard.svelte` (explicitly "no animation loop while streaming", redraw at 2 Hz), the hold-Esc ring and countdown, the spinner `.spin`, `Stream.svelte` itself.
- `web/src/lib/{video,audio,input,gamepad}.ts`, `audio-worklet.ts`: tested, p50 measured in `docs/m0-baseline.md`. A rAF engine competing with the WebCodecs/WebTransport work on the main thread is the risk, and the anti-goal forbids it.
- Anything in the Shell that is mounted on `/stream` (Toasts, CommandMenu, QuitDialog, sidebar): no library calls while the path is `/stream`.
- `ControllerTest` input sampling and `Trail` (diagnostic accuracy).

## 7. Open questions

1. Does the Chrome build we target accelerate anime's spring easing in `waapi.animate` (the docs warn custom eases can block acceleration, mostly on Safari)? Needs a DevTools check; not answerable from the docs.
2. Can `animate()` tween a CSS custom property that holds a colour (`--accent`) or a registered `@property` number (`--p`) in 4.5? The docs index lists "tween value types" but I did not open that page.
3. What does `createLayout` (FLIP) actually guarantee with Svelte keyed `#each`, and how does it behave with the virtualised shelf? Docs sections exist ("states", "layout id", "common gotchas"); not read.
4. Does importing `animejs` start the engine loop, or only when the first animation is created? Matters for idle CPU on pages that never animate. Not verified.
5. Does anime's scope `revert()` clean up inline styles in a way that is safe with Svelte-owned `style:` bindings? Probably, but untested.
6. Is `peerDependencies: three` marked optional, so npm installs it or warns? I did not read `peerDependenciesMeta`.
7. What is on `motion-2`? It had no committed difference from `main` when I looked, so the overlap with `.reveal`, `.page-in` and `.swap-in` cannot be judged.
8. LayerChart: its class names for series paths and whether its own `motion` prop already gives a draw-in, so a second system is not needed.
9. Does Astro's static SSR build import-evaluate `animejs` safely if some file imports it at top level (we avoid this by lazy import, but a stray import would break the build)? Not tested.
10. Is `sibling-index()` in the Chrome version Toby runs? That decides whether item 4 in the first batch can be pure CSS. Not verified from a primary source.
