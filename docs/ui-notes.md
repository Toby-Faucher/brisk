# Brisk UI notes

## Settings tabs
- Settings is split in Bits UI Tabs (arrow keys, Home/End, roving focus): General (accent, sounds, shaders), Streaming
  (default quality, replay buffer, clipboard sync, keep the screen awake), Controller (the pad test), About (commit, host name and Sunshine state
  from the `host` store, shortcuts list, doc paths as plain text; hostd does not report a version). Pure helpers and
  tests: web/src/modules/settings/tabs.ts.
- The tab lives in the URL hash (`#general|streaming|controller|about`), written with `history.replaceState` (no history
  entries) and mirrored in localStorage `brisk.settingsTab`. No hash: last used tab, then General. An unknown hash: General.
  `hashchange` and `popstate` listeners are removed on destroy (the Shell remounts islands).
- The Controller panel is only rendered while its tab is active, so its polling loop is unmounted (not just paused) on
  leaving. The commit shown comes from `git rev-parse --short HEAD` at build time (vite `define` `__BRISK_COMMIT__`).
- The server render shows General and the real tab is applied on mount, so a deep link flashes General for a frame.
- Not done: the shell sidebar has no phone layout (every page is 450 px wide at 390 px); the tab strip itself scrolls.

## Motion
- Tokens in tokens.css: `--dur-page` 200 ms, `--dur-tab` 180 ms, `--dur-enter` 420 ms, `--rise-enter` 14 px, `--stagger` 55 ms, `--rise` 8 px,
  `--rise-scroll` 14 px, easing `--ease`. All motion is opacity + the independent `translate` property (never `transform`,
  which hover owns), no `will-change`, and sits in base.css under `prefers-reduced-motion: no-preference` (reduced motion:
  no animation, `scroll-behavior: auto`).
- Page: `<main class="page-in">` (Shell only, so the sidebar, host pill and frame never replay) is an opacity-only 200 ms
  fade. Not on /stream (anti-goal, docs/ui-design.md section 2; a `translate` on `<main>` would also trap the stream's
  `position: fixed`, and `reveal.ts` does nothing there).
- Load entrance (every reload and client navigation): items above the fold rise 14 px and fade in over 420 ms, delayed
  `--i` x 55 ms (`--i` 0..8, so the 9th item and later share 440 ms). Tagging helper `shell/reveal.ts`: `enter()` finds the
  untagged `.reveal` / `.enter` elements in `<main>` that intersect the viewport, sets `--i` (DOM order) and `data-enter`;
  CSS `.reveal[data-enter], .enter[data-enter]` plays `enter-rise` on the document timeline (its shorthand resets
  `animation-timeline` to auto, so it beats the scroll reveal). It is idempotent, a tagged element never replays. Pure part
  `planEnter` is unit-tested. Callers: Shell on mount (waits for the Astro islands to hydrate, max 1.5 s) and each place
  where content arrives late: Library games landing (hero, shelf headers, first <= 8 cards per shelf), History list,
  Machines host card, Home lazy widgets. Layout's head script sets `html[data-enter-pending]` (and again on
  `astro:after-swap`; 3 s failsafe) which hides `.reveal/.enter` in `main.page-in` until `enter()` has tagged them, so the
  SSR markup does not flash before it animates.
- Classes: `.reveal` = above the fold: load entrance, below: scroll reveal (`animation-timeline: view()`, `entry 0% entry
  35%`, inside `@supports`, unsupported means plain visible) on `Card`, page headings (`.top`), Library filters, hero and shelf
  headers, Add-a-module. `.enter` = entrance only, never scroll-driven: Library cards (first 8 of a shelf at its start),
  and only tagged by `enter()` at route load / games arrival, so paging a shelf or scrolling mounts cards without any
  animation. `.fade-in` (opacity, 420 ms) is added by class when data lands: Home host card and resume card content.
- Library search: the results (`.body`) cross-fade 0.3 -> 1 over 140 ms via WAAPI, at most once per 150 ms. Filter chips:
  a view transition (see Platform features), the same fade when it is skipped.
- Dialogs (command menu, Quit and Restart confirms): CSS transitions in base.css. Enter from `@starting-style` (opacity
  over `--dur-page`, a small rise/scale on `--spring-snappy`), leave faster from Bits' `data-ending-style`
  (`--dur-exit` 140 ms, `--ease-exit`), so they no longer vanish. Tips enter over `--dur-tab` and leave over `--dur-exit`.
- Also animates: Settings tab panels and the History list/detail swap (`.swap-in` / `.live`, 180 ms, only after a user action).
- Does not animate: /stream (except the ghost bar's CSS slide), virtualised items when they mount while scrolling, tall
  blocks such as an open shelf grid, the ghost bar's own menus (they enter but do not exit: /stream), the host pill (apart from its dot's one ring on a state change, below).
- Springs (CSS only, zero runtime): `web/scripts/gen-spring-tokens.mjs` solves three springs with anime.js offline and writes
  `--spring-soft|snappy|settle` (CSS `linear()`) and `--spring-*-dur` (settling time, use it as the duration) into the
  generated block at the end of tokens.css; `npm run springs:check` and `src/styles/tokens.test.ts` fail if it is stale.
  soft (400/40, no overshoot, 90 % at 195 ms, capped at 450 ms): Card and game card hover lift. snappy (380/34, 0.4 %, 90 % at
  169 ms, 700 ms): link-card press, glides. settle (150/19, 2 %, 90 % at 237 ms, 900 ms): ghost bar slide-in (it leaves
  faster, 300 ms `--ease`), confirm dialog rise (`.rs`). These CSS springs are allowed on /stream; anime.js is not.
- anime.js 4.5.0 (exact pin, MIT; a runtime dependency because `ui/motion.ts` loads it) is used only where CSS cannot do it,
  all through `web/src/ui/motion.ts`, lazily by sub-path (`animejs/animation`, `/easings`, `/svg`, `/scope`), each on its first
  real use: numbers that change after load (`count` attachment / `tweenNumber`: Home host temps and ring `--p`, Machines GPU/CPU,
  Library widget installed count; first paint never animates; uptime is too coarse to roll; History has no polled numbers)
  and the History chart line draw-in (`drawIn`, `createDrawable`, 700 ms `--dur-draw`, once per open, spread onto LayerChart's
  line path through an attachment key; LayerChart's own `draw` prop redraws on every resize). The fill under it fades in
  with native WAAPI (`fadeIn`). `scope()` / `motionScope` wrap `createScope` for future multi-part effects (reverted on destroy).
- Glides (no library): `ui/motion-base.ts` `glide()` is native `Element.animate` on `translate` + `scale` of the selected
  item's `::before` highlight (server-rendered, `transform-origin: 0 0`, `z-index: -1` inside the list's stacking context so
  it passes under the neighbours). Sidebar: the previous active item's box lives in module state, so after a client
  navigation the highlight glides from it across the Shell remount (not on first load, not onto /stream). Library filter
  chips and Settings default-quality chips: `glideSwap` (measure, change, `flushSync`, FLIP). The Settings tab underline keeps
  its CSS transition.
- Motion pass 2A (research/anime-feature-map.md #6 #5 #11 #15 #12; check: `node web/scripts/motion2a-check.mjs [graph stream ring
  morph split chips reduce fail path]`, port 4480, screenshots to `$OUT`; unit tests in `motion.test.ts` / `motion.fail.test.ts`):
  - Status-dot ring (`ui/StatusDot.svelte`, `ringPulse` in motion-base): host pill and Machines dot. When the state changes
    (ready / busy / down) one aria-hidden sibling span `.halo` scales 1.3 -> 2.4 and fades .6 -> 0 over 700 ms (native
    `Element.animate`, `--ease`, no loop). Not on first paint (an `off` before the first real state is "still loading"), not on
    reduced motion, and not on /stream even though the pill is mounted there (silent no-op, no timers, no library).
  - Play -> spinner (`PlayIcon.svelte`, `swapIcon`): the play icon of Hero, game card and Resume card turns out (-90 deg, fade),
    its path `d` becomes a 270 degree ring, it turns in; while the start is in flight one compositor `rotate` loop turns the
    ring, back to the triangle if the start fails. `svg.morphTo` was tried and dropped: triangle -> ring crosses itself in the
    first frames (the pair has no matching structure), so it is a native swap (180 ms in two halves, nothing loaded, so no chunk
    to fail). Mute and chevrons were not done: the only mute toggle is the ghost bar's (/stream), and the chevrons never toggle.
    `Icon.svelte` itself is unchanged and `motion.ts` is not in the Stream chunk (38627 B before and after).
  - Hero title (`splitIn` + `splitWords` in motion.ts, no library: anime's `splitText` cost 10.1 kB gzip plus a ResizeObserver and a 150 ms timer, so an in-house `Intl.Segmenter` splitter replaced it; createElement/textContent only, never innerHTML):
    `<h3 {@attach titleIn}><span>{game.name}</span></h3>`. The split words are a COPY (`.split-fx`, aria-hidden) laid over the
    real text, which only fades (opacity, so it stays in the accessibility tree) and keeps the layout; the words rise out of
    clip masks (`translate` 100% -> 0, 500 ms, 45 ms apart, native), starting 120 ms after the hero's own entrance animation has finished (the real title is hidden meanwhile). The copy is removed at the end, leaving the
    exact original markup. Once per Library mount (`titled` flag in Library: a refresh or a search that re-mounts the hero does
    not replay it). The game name is the hero title; the "Continue where you left off" line above it is a small label and stays.
  - Drift result (`popIn`, `countUp`): after "Check drift" the Verdict chips pop in 60 ms apart (opacity + translate + scale,
    native) and Offset, Noise and Min deadzone count up from 0 (`tweenNumber`, `--dur-count`). Only the result table: the stick dots,
    numbers, bars and mapping rows stay raw. A second run re-animates (the table is gone while it measures).
  - Connection path (`ConnPath.svelte`, `travel` in motion.ts): bawn -> laptop lane on the Machines card. Direct: solid line,
    steady dot (1.6 s); relay: dashed line through a relay node, amber, 3.2 s; unknown path: dim dashed line, no dot. The dot is
    ONE infinite native `translate` animation (percent of its own width: nothing measured), paused while the tab is hidden,
    cancelled on destroy and when reduced motion switches on (it does not restart when reduced motion is switched off live, nor does the spinner); Chrome trace: never `compositeFailed`. aria-hidden; the Network
    row beside it carries the words. (`GET /api/host` has no RTT, so no "2 ms" text.)
  - Rule reminder for new motion: motion-base stays library-free (the Shell imports it: every byte is on /stream), a lazy
    helper's chunk failing must end in the end state, and a name you add (`.halo`) must not collide with a selector another
    check already uses (the first `.ring` here was the Home CPU ring's).
- Motion pass 2, F3b (#13 #9; check: `node web/scripts/f3b-check.mjs [status stream picker reduce fail]`, port 4490,
  screenshots to `$OUT`; unit tests `motion.test.ts` (dotFade, fadeOnChange, flipDelta), browser test `AddModule.browser.test.ts`).
  Both are native Web Animations: no anime.js chunk is needed, so a failed chunk load changes nothing.
  - Status transitions: `StatusDot` keeps an aria-hidden `.was` copy of the dot (resting at opacity 0, on top of the ring); on a
    state change it takes the old state (`data-s`, written only by the component's effect) and fades 1 -> 0 over 360 ms
    (`dotFade` in motion-base): a cross-fade, opacity only, together with the ring. Same guards as the ring (not on first
    paint, not on /stream, not under reduced motion). On the Machines card the big text, the Sunshine word and the uptime fade
    in from .35 when their text changes (`fadeOnChange` attachment, opacity only; first value and repeats do nothing). Rows
    are monospace tabular numbers in a fixed grid: nothing moves (checked: rows and card height identical through ready ->
    busy -> down and 1 h -> 2 h). Chrome still scores ~0.00015 CLS when the right-aligned Sunshine word gets longer, with or
    without motion. No live region on the card, so polling never speaks.
  - Module picker: the "Add a module" card is now a Bits `Dialog.Trigger` for a small picker (installed modules and what each
    contributes; adding one is still a folder in web/src/modules, so it lists, it does not install). Opening: the panel is laid out
    centred (`.mp`, no `transform`, `transform-origin: 0 0`) and `glide()` FLIPs it from the card's box (translate + scale,
    `--spring-snappy`), its opacity a CSS `@starting-style` transition. Closing: the card's box is measured in the open-change
    handler and passed as `--mp-t` / `--mp-s` (Svelte `style`), and `.mp[data-ending-style]` transitions translate, scale and
    opacity there in `--dur-page` with `--ease-exit` (faster than the 700 ms swell), so Bits waits for it. A close mid-swell cancels
    the swell first. Esc, Tab trap, focus return to the card and axe are unchanged/clean; reduced motion = no rule, instant.
    Non-uniform scale squashes the text for the first frames (card 386x120 -> panel 420x398); accepted for a 700 ms swell.
- Motion pass 2, F3c1 (#10 Home card drag-to-reorder; check: `node web/scripts/f3c1-check.mjs [load nojs pointer touch pen
  keyboard cancel persist race nav reduce stream axe]`, port 4491, stub hostd keeps homeOrder across reloads, screenshots to
  `$OUT`; unit tests `home.test.ts` (reconcileOrder, moveItem), `prefs.test.ts` (refreshPrefs), browser test
  `drag.browser.test.ts` (blur/hidden cancel, key repeat), hostd `prefs.rs` + `tests/prefs.rs`).
  Native Web Animations only (`modules/home/drag.ts`, an attachment on the bento), no anime.js.
  - Each Home card sits in a `home/Slot.svelte` (no markup, sets a context); `ui/Card.svelte` then renders `data-card=<id>`
    and a 28 px handle (`[data-grip]`, top right, `touch-action: none`, `aria-pressed`, described by `#grip-hint`). Only the
    handle drags, so card links and buttons click as before. Ids: host, resume, quick, recent, then module ids (widgets, soon
    cards). "Add a module" is not a card in the order and always stays last.
  - Pointer (mouse, touch, pen: pointer events, capture on the bento because the card's node moves in the DOM): the card lifts
    (`[data-lifted]`: scale 1.03, shadow, opaque, z-index; CSS transition on the snappy spring), follows the pointer through a
    zero-length held `translate` animation (above the CSS reveal animation, no inline style), and when the pointer is over
    another card's layout box (`offsetLeft/Top`, so cards in flight do not confuse the hit test) it takes that card's place:
    the others FLIP from where they are drawn on `translate` only (`--spring-snappy`). No swap back with the same card until
    the pointer leaves it (cards differ in size). Release lands it on `--spring-settle` (`[data-landing]` keeps it above until
    it is down). Esc, pointercancel, window blur or the tab hiding put everything back in the pick-up order (the release
    may never come). A press without a move changes nothing.
  - Keyboard: focus the handle, Space picks up, arrows move one place in the order (Left/Up back, Right/Down on), Space or
    Enter drops, Esc puts it back, Tab away drops. Key repeats of Space/Enter are ignored (holding Space picks up once). Focus is put back on the handle after each move. One polite line on a drop
    ("Resume card dropped, position 4 of 9", `.sr[role=status]` on Home); pick-up, steps and cancel say nothing.
  - The first DOM move sets `data-sorted` on the bento, which turns off the time-based load entrance there: a moved node would
    replay it. Reduced motion: no lift transition, no FLIP, no drop spring (instant swap); the held card still follows the
    pointer (that is the user's own hand). Destroyed mid-drag (router navigation): follow and flights cancelled, attributes
    removed, nothing saved. CLS: the moves are input-driven shifts (`hadRecentInput`), CLS stays 0.
  - Order: `homeOrder` in hostd prefs (below, API); `reconcileOrder(saved, current)` keeps saved ids in order, appends new
    cards, drops gone ones. To keep a saved order from moving cards after first paint, the bento is `visibility: hidden`
    (`data-hold`, keeps its size) until the order and the widget chunks are in, at most 1.5 s; `enter()` (shell/reveal.ts)
    skips `[data-hold]` and runs again when it shows. If the Home chunk never runs (Astro retries the import once), a
    CSS-only fallback shows the server-rendered grid after 3 s (a 0 s `visibility` animation delayed 3 s, `!important` so
    the reduced-motion `animation: none` does not cancel it), as the head script does for data-enter-pending. Repeat
    visits use the store (no wait). A drop PUTs the whole order (a failure is a toast); only the latest PUT's reply may
    set the store (sequence counter), so a slow stale reply cannot bring an older order back. Reset: Settings > General > Home layout > "Reset layout" (DELETE, then Home is in the default order).
- Motion pass 2, F3c2 (#14 session cover grows into the detail header; check: `node web/scripts/f3c2-check.mjs [morph back
  pop rapid during fail reduce nav cross stream axe]`, port 4492, mocked /api and covers, seeked screenshots to `$OUT`; unit
  tests `motion.test.ts` (morph)). No anime.js, no new chunk.
  - The detail view already existed (in-page, `?session=<started>`, pushState); it now has a header: 112 px cover (fixed box,
    placeholder when there is no cover or it failed to load: a `SvelteSet` of failed URLs, used by the list too), name
    (`#session-title`), gate, and the when / duration / quality line. Nothing new is fetched or invented.
  - `morph(name, pick, update)` in motion.ts: a same-document view transition of type `morph` where ONLY the cover is named
    (`view-transition-name: session-cover`, inline, set on the old cover before and the new one after the update, removed
    when it ends, is skipped or fails; an emptied `style` attribute is removed too). base.css `:active-view-transition-type(morph)`
    keeps the root out of the capture (`view-transition-name: none`, `::view-transition { pointer-events: none }`), as for the
    Library chips: the rest of the page swaps live (the usual `.swap-in`) and a click 80 ms in reaches the back button.
    Group on `--spring-snappy`, the two snapshots cross-fade in `--dur-tab` so the sharp cover shows early. Chosen over a
    FLIP: the swap-in fade's opacity on the parent would fade a FLIPped cover with it; the snapshot sits above it. The
    group animates width/height (the browser's, one small snapshot): accepted, like the Library cards.
  - A second swap skips the running one and takes its name off synchronously (never two elements with one name); its update
    still runs and converges on the latest target (`want`). Skipped (plain swap) without support, under reduced motion
    (also switched on mid-flight: base.css drops the pseudo animations), on /stream, during the router's own transition.
    History's destroy calls `skipMorph()`.
  - Focus: open focuses the heading (`tabindex=-1`, after an instant scroll to the top); back focuses that session's link
    (scrolled into view, instant, before the new snapshot is taken, so the cover lands where the card now is).
  - Browser back/forward: before, the list entry (made by the router, with its state) made the router refetch and swap the
    whole page on Back, and our entries had null state, so leaving a session for another page and coming back left that
    page's content under the session URL. Now our entries copy the router's state (same index) and History listens to
    `astro:before-preparation`: a traverse to its own path gets a loader that never settles (the router's documented hook;
    the next router navigation aborts it), and History's own popstate listener morphs. Other pages stay the router's (`cross`).
- Not done: Library grid FLIP (`createLayout`): it works with the keyed list, but walks the whole subtree under its root and
  cost 50-61 ms per change on the 40-card "Show all" grid (over the 50 ms long-task budget while typing). Patch and numbers:
  `.superpowers/sdd/anime-pass-flip-experiment.patch`, `anime-check.mjs flip`.
- Motion rules: (1) nothing from anime.js is imported, fetched or run on /stream or by anything it loads (Shell, Sidebar,
  ghost bar, stats card, toasts, dialogs): the Shell imports only `motion-base.ts`, which has no library; `motion.ts` throws in
  dev and never resolves in prod there. (2) Pin animejs exactly; never 5.x beta. (3) anime.js ignores reduced motion: every
  helper checks it live and writes the end state; a switch mid-flight completes running tweens. (4) The Shell remounts on
  every navigation: whatever a component starts is cancelled or reverted in its cleanup (a leaked loop ticks forever).
  (5) Lazy only: dynamic `import()` on first need. (6) Compositor properties only for movement: `translate`, `scale`,
  `opacity`; never anime's `x`/`y` in waapi (they go through a custom property and fail compositing). (7) No anime `onScroll`
  (it holds a 60 Hz rAF loop alive) and no `loop: true`. (8) anime and Svelte never write the same thing on one element (the
  ring's `--p` is no longer a `style:` binding; `count` writes Svelte's own text node and ends on Svelte's value).
  (9) Nothing left behind: no `will-change`, no inline `transition`, no inline `--accent` on `<html>`. (10) Every lazy motion
  path falls back to its end state when its chunk fails to load (a number shows the new value, a chart line shows in full).
  (11) Exits are faster than entrances (`--dur-exit`, `--ease-exit`) and are CSS transitions the component library can see
  (`getAnimations()`), never a JS timer before unmounting. (12) A view transition never runs per keystroke.
- Smooth scroll: `html { scroll-behavior: smooth }` (off while the Astro router is navigating, so scroll restore does not
  glide), `scroll-padding-top: 64px` for the sticky Settings strip, `overscroll-behavior-x: contain` on the shelf and the tab
  strip. No JS smooth-scroll library and no wheel hijacking (the shelf's vertical-wheel-to-sideways stays as it was).
- Do not write the scroll reveal as an `animation:` shorthand: the minifier would fold `view()` into it and Chrome drops that.
- Check: `node web/scripts/motion-check-2.mjs` (load entrance, library data arrival with a delayed /api/games, no re-fire, reduced
  motion, stream, seeked screenshots to `$OUT`) and `node web/scripts/motion-check.mjs` (page fade, tabs, scroll reveal), both
  against `astro preview` (ports 4422 / 4421, mocked /api). Home shows a pre-existing layout shift of about 0.02-0.06 when the
  module widgets mount, with or without motion. anime.js pass: `node web/scripts/anime-check.mjs [graph stream count revert
  reduce glide chart tour spring]` (port 4423; /stream import graph + network, tweens, revert on navigation via rAF counts,
  reduced motion, glide seeked screenshots + Chrome trace, chart draw-in, long tasks). Unit tests: `web/src/ui/motion.test.ts`.

## Platform features
Chrome floor: 152 (Toby's laptop moves to 156). Everything below is in 152; each degrades to the old behaviour without it.
- Dialog enter/exit: `@starting-style` (117) + Bits UI 2.20.2 presence. Bits sets `data-starting-style` / `data-ending-style`
  and keeps the node mounted until `getAnimations()` on it has finished (one rAF after the state change), so plain CSS
  transitions are enough: no `forceMount`, no `child` snippet, no anime.js. Bits dialogs are portalled divs, not top
  layer, so `transition-behavior: allow-discrete` is not needed there. QuitDialog keeps its last text while it leaves.
  Without `@starting-style` they appear at once; reduced motion: no transition, unmount at once (base.css).
  While the backdrop fades, clicks reach the page: Bits keeps `pointer-events: none` inline on <body> (modal) and `auto`
  on the overlay until unmount, so base.css overrides both under `[data-ending-style]` (`body:has(...)`, !important).
  Focus return and Esc are unchanged (platform-check, browser tests).
- Tooltips: `ui/Tip.svelte`, placed right after its trigger (`<button aria-label=…>…</button><Tip label=… placement=…/>`).
  A `popover="manual"` span (114) in the top layer, `anchor-name`/`position-anchor`/`position-area` (125) with
  `position-try-fallbacks: flip-block, flip-inline` (128), `role="tooltip"`, the trigger gets `aria-describedby` (not when the tip only repeats its aria-label, as on the
  accent swatches).
  Hover 300 ms or keyboard focus (`:focus-visible`, so a click does not pop it); leave, blur, a press or Esc hides it.
  Exit uses `allow-discrete` on `display`/`overlay` (top layer). Without anchor positioning
  (`CSS.supports('position-anchor: --a')`) it is placed once from the trigger's rect, no flip. Used on: Library card
  Quit and saved-quality chips, Hero chip, History gate (focusable, `role="note"`), Settings accent swatches. The
  saved-quality chips cannot take focus (the card chip sits inside the card button), so their tip is hover-only and
  supplementary: the card's accessible name already says "saved quality …" and the chip text is visible. Not on the ghost bar: its CSS-only
  `.tip`s stay (/stream, no new JS there); not on the sidebar (it never collapses, items have visible labels).
  The disabled "Wake-on-LAN" buttons keep `title=` (disabled buttons get no pointer events; the text is visible).
- Library filter chips: same-document View Transition (111; `types` 125, `view-transition-class` 125). `Library.svelte`
  `pickFilter`: on-screen cards in an open "Show all" grid get `view-transition-name: vt-<appid>` (at most 60, removed
  when it ends, also after a skip or a failed update; virtualised shelf rows are never named). Type `library` scopes the
  CSS, so Astro's router transitions are untouched: `--dur-tab` (180 ms), `::view-transition { pointer-events: none }`
  and `view-transition-name: none` on the root. The root is not captured on purpose: with it captured, Chrome (headless
  1248) hit-tested <html> for the whole transition even with `pointer-events: none`, swallowing every click; now only
  the named cards are snapshots, the rest of the page stays live (it gets the usual .body fade) and a card clicked 100 ms
  in reacts (platform-check, with a control that re-captures the root and loses the click). The named cards themselves
  are not clickable while they glide (180 ms). With no grid open nothing is captured, so it is just the fade.
  A second chip click calls `skipTransition()` on the running one and starts a new one. Skipped (plain swap + fade)
  with reduced motion, without support, or while `html[data-astro-transition]` is set.
  Search has none: with a transition only when the result set changed and at most once per 300 ms, typing "game 1"
  (6 keys, 80 ms apart, both grids open, 139 games, headless Chrome) made the slowest key event 168 ms vs 40 ms without
  (136 ms even with no card names: the snapshot itself), no long tasks either way. It failed the bar (no slower than
  without), so search keeps the WAAPI fade. Chip click with both grids open: no long task.
- Screen Wake Lock (84, secure context; localhost counts): `stream/wakelock.ts` `keepAwake()`. While the stream is
  `live` and Settings > Streaming "Keep the screen awake while streaming" (`keepAwake`, default on) is set, it asks for
  a `screen` lock when the tab is visible and again on `visibilitychange` (Chrome drops it when hidden); released on
  failed, Disconnect, leaving /stream and the setting going off. Refusals (NotAllowedError, low battery, no API) are
  one `console.debug`. No UI, no timers. /stream JS +630 bytes uncompressed over its 24 files (Stream chunk +617:
  38010 -> 38627; settings chunk +13), no new file; vs main +706 in all (QuitDialog +76).
- Deliberately not used: `interestfor` (interest invokers). Checked 2026-10-26 against
  https://chromestatus.com/api/v0/features/4530756656562176: the ship stage (stage_type 160) starts at desktop 142 and
  the rollout plan says "Will ship enabled for all users", although the summary label still reads "Origin trial".
  So it is in 152, and research/ecosystem-frontend-2026-10.md is wrong to call it origin-trial only. Not used anyway:
  the plan ruled it out for this pass, and Tip needs the 300 ms delay and the click-does-not-pop rule it has today;
  revisit to drop Tip's listeners. `popover="hint"` (not needed: tips are manual popovers). Scroll-triggered
  animations (`animation-trigger`): https://chromestatus.com/api/v0/features/5181996801982464 has a ship stage from
  desktop 146 (label "Proposed"), so also in 152; not used because the scroll reveal on `animation-timeline: view()`
  already does what we need.
  cross-document view transitions (Brisk needs one persistent document, see research/ecosystem-frontend-2026-10.md).
- Check: `node web/scripts/platform-check.mjs [dialog tip vt wake]` against `astro preview` (port 4470, mocked /api):
  dialog still mounted and fading 50 ms after Esc then gone, focus back, reduced motion instant; tip hover/focus/Esc,
  flip at the top edge, the no-anchor fallback, axe with a tip open; the chip transition (spy on
  `startViewTransition`, pseudo-element animations, names removed, none under reduced motion, none while typing, a
  card click during it plus its control, a second chip click, a throwing update) and the typing numbers (information
  only); the backdrop letting a click through during a dialog exit; wake lock acquire/re-acquire/release with a stubbed `navigator.wakeLock` and a fake
  WebTransport, setting off, failed state, and /stream's JS file list. Screenshots to `$OUT`.

## Run
- Normal: hostd runs as a systemd user service and the laptop opens https://bawn.tail0.ts.net:8443
  (see Hosting below and docs/hosting-runbook.md).
- By hand (dev path, only while the service is not active; brisk-up and brisk-down refuse otherwise):
  `scripts/brisk-up.sh` (builds bridge + hostd in the capped scope, switches to the patched Sunshine, runs
  `target/release/brisk-hostd` in the foreground). It prints `page<TAB>http://localhost:8443/?s=<secret>`;
  the secret lives in `~/.config/brisk/secret`, so the link stays the same across runs.
- On the laptop: `ssh -N -L 8443:127.0.0.1:8443 bawn`, then open that URL in Chrome.
- Stop: Ctrl-C (hostd stops its bridge first), then `scripts/brisk-down.sh` (restores stock Sunshine).

## Hosting
```
 laptop Chrome ──https://bawn.tail0.ts.net:8443──▶ tailscaled on bawn (tailscale serve, TLS,
   │                                                     adds Tailscale-User-Login/-Name)
   │                                                         │ http://127.0.0.1:8443 (loopback)
   │                                                         ▼
   │                                    brisk-hostd.service (systemd --user, toby, linger)
   │                                     page + /api, Host allowlist, secret or tailnet login
   │                                         │ spawns at each stream start (own cgroup)
   │                                         ▼
   └──WebTransport https://<tailscale IP>:4433──▶ brisk-bridge ──▶ patched Sunshine (drop-in,
                (cert hash pinned, 13-day cert)                     permanent; plain RTP w/o bridge)
```
- The root address https://bawn.tail0.ts.net is Toby's `orch` (127.0.0.1:8481). Brisk only ever
  touches the :8443 mapping (`tailscale serve --bg --https=8443 http://127.0.0.1:8443` and
  `tailscale serve --https=8443 off`). hostd itself still binds 127.0.0.1:8443 only; tailscale serve is
  the only way in from the tailnet, so nothing outside the tailnet can reach it.
- Unit: deploy/brisk-hostd.service, installed by `scripts/brisk-service.sh install` into
  ~/.config/systemd/user (WantedBy=default.target; with linger it starts at boot without a login).
  Type=notify + NotifyAccess=main: hostd sends `READY=1` with `STATUS=serving 127.0.0.1:8443, stream idle`
  only once its own HTTP probe of 127.0.0.1:8443 gets an answer (TimeoutStartSec=30 fails a start that
  never gets there), `STOPPING=1` on SIGTERM before it stops the bridge, and with WatchdogSec=60 a
  `WATCHDOG=1` (plus a fresh STATUS with the stream state) every 15 s (a quarter, so three missed pings
  are tolerated), but only while a freshly spawned
  tokio task runs and the library and stream state locks can be taken within 1 s: a wedged runtime or a
  stuck lock stops the pings and systemd restarts hostd. Without NOTIFY_SOCKET (manual runs, brisk-up)
  none of this runs (crates/brisk-hostd/src/notify.rs, `sd-notify` crate).
  Restart=on-failure (also after a watchdog timeout) every 3 s (gives up after 10 starts in 2 min), TimeoutStopSec=20 (the bridge stop
  can take 12 s), KillMode=mixed (SIGTERM to hostd only, so it stops the bridge first; SIGKILL to anything
  left after 20 s), MemoryMax=1G, CPUWeight=50, Nice=5, Wants/After the Sunshine
  unit. Hardening is deliberately thin, each omission is commented in the unit: NoNewPrivileges would be
  inherited by a Steam that `steam steam://` cold-starts; ProtectSystem/ProtectHome/PrivateTmp/
  PrivateUsers/ProtectProc need a user+mount namespace in a user unit, which Steam, games and quit
  signals to processes outside it would inherit or cross; address-family limits would have to allow IPv4 and
  AF_UNIX (sway, Wayland, D-Bus, tailscaled) anyway. `BRISK_SPAWN_SCOPE=1` (set by the unit) puts each
  `steam steam://…` in its own `systemd-run --user --scope`, so a cold-started Steam and its games are
  not under the 1G cap or killed by a restart.
- Self-managed at each stream start (no shell script needed): the bridge address is `tailscale ip -4`
  :4433 unless `BRISK_LISTEN` is set (0.0.0.0 refused; tailscale down = a clear 500 "tailscale gave no
  IPv4 address"), and `~/.config/brisk/tls/bridge.pem` + `bridge.key.pem` (ECDSA P-256, 13 days, 0600)
  are made with openssl when missing, older than 10 days, dated in the future, or mismatched (key newer
  than cert; a new pair gets one mtime). Both happen before an old bridge is stopped,
  so a failure leaves a running stream alone. A new pair is written to `.bridge*.new` (0600 before
  openssl writes) and renamed over the old one; a failed openssl leaves the old pair.
- Scripts: `scripts/brisk-service.sh install | update | status | logs [-f] | link | uninstall |
  serve-on | serve-off | sunshine-permanent | doctor` (never sudo; a root step is printed as the
  `sudoq run ...` line). `update` = capped release build of hostd + bridge, `npm ci` only when the lock
  changed, and only when web sources are newer than web/dist a build into a new `web/dist.<stamp>`
  (`npm run build -- --outDir`), swapped in by renaming a `web/dist` symlink over the old one (`ln -sfn`
  + `mv -T`, atomic; a first plain dist is moved to `web/dist.plain-<time>-<pid>`). A failed or empty
  build is deleted and web/dist stays. Pruning never touches the build web/dist points at and keeps the
  most recent other build with an index.html by mtime (not by name, so a clock step cannot pick the
  live one). The swap happens only together with the restart, and the restart only when hostd's API
  answers `idle`: a live stream or no answer keeps the old page and binary running, drops the new
  build and fails (FORCE=1 overrides). `install`'s restart of a changed unit follows the same rule. hostd
  canonicalizes `dist` per request and only serves files that canonicalize inside it (a symlink out of
  the build is a 404); a missing path with an extension is a 404, never index.html (see Static files
  below). `update` does not
  re-sync the unit file: after a change to deploy/brisk-hostd.service run `install` again (it restarts
  the unit when the file changed). `install` = copy the unit (the previous one is kept as
  `brisk-hostd.service.bak`), daemon-reload, enable --now, and a restart if the unit changed while it ran
  (not over a live stream: then a changed unit is not even written and the manager is not reloaded, so
  the running hostd never gets the new unit's settings, e.g. a watchdog it would not ping; install exits
  non-zero, run it again when idle). A unit file newer than the running process (same content) counts as
  "installed but not restarted yet" and is restarted when idle. Whenever a changed unit was (re)started, `install` waits up to 45 s for
  `is-active` and an HTTP answer on 127.0.0.1:8443 (403 is fine); otherwise it copies the .bak back,
  daemon-reloads, restarts it, checks it the same way and exits non-zero with "new unit failed; previous
  unit restored" (or "... did not come up either"). No .bak, as on a first install: the unit is disabled
  (no restart loop at boot) and install exits non-zero. `sunshine-permanent` = sunshine-brisk.sh install + enable (100 s
  spaced restarts, refuses unless Sunshine is FREE). `doctor` checks linger, the unit (enabled, active,
  Type=notify, SubState running with a status text, watchdog 60 s), the listener
  (127.0.0.1:8443 required; tailscaled's own `…%tailscale0:8443` sockets in TUN mode are fine; 0.0.0.0, [::],
  * or any other address FAIL), the serve mapping (`<name>:8443` -> `http://127.0.0.1:8443` exactly), 403
  without and 200 with the secret over https, the certificate age and the Sunshine drop-in/binary/state.
  Tests: scripts/test-brisk-service.sh (all stubs).
- Under systemd (`INVOCATION_ID` set) hostd's startup line is `page<TAB>http://localhost:8443/ (link:
  scripts/brisk-service.sh link)`, without the secret, so the journal never holds it. The bridge's
  `page` line (its wt URL carries the session token, which grants stream and input) and any other bridge
  line with `wt=` or the token are re-printed as `bridge<TAB>page <redacted>` (hostd still parses them for
  GET/POST /api/stream). `logs` also masks any `?s=`/`&s=` and `?wt=`/`&wt=` value. brisk-up and manual runs still print the full link. brisk-up/brisk-down refuse
  while the unit is active or enabled.
- Owned games (`GetOwnedGames`) are fetched in-process with `ureq` over rustls with the pure-Rust
  `rustls-graviola` provider (no curl, no C or ring), certificates checked against the system roots
  (/etc/ssl/certs, read by `rustls-platform-verifier`, so it works offline-installed on Arch and follows
  `ca-certificates` updates). The key is read from its file per request and goes only into the query
  string (`key=`, which is how Steam's Web API takes it); error text is reduced to a failure kind ("TLS
  error", "HTTP 403", "i/o error (ConnectionRefused)"), because ureq's own messages can contain the URL.
  Connect 5 s, total 15 s, no redirects, no proxy, 8 MiB body cap, https only (plain http only to
  127.0.0.1/localhost, for the tests' mock). `cargo test -p brisk-hostd -- --ignored live_tls` checks the
  stack against Steam's public keyless endpoint and that a self-signed certificate is refused.
- A restart of the service ends hostd's `wl-copy`, so a clipboard set from the laptop is gone from bawn's
  clipboard after an update or restart.
- Rollback: `git checkout <good commit> && scripts/brisk-service.sh update`; or back to the tunnel
  workflow: `scripts/brisk-service.sh uninstall`, `serve-off`, then brisk-up/brisk-down as before.
- Root steps (Toby, via sudoq): only `tailscale serve --bg --https=8443 http://127.0.0.1:8443` (and its
  `off`), because toby is not the tailscale operator. Alternative once: `tailscale set --operator=toby`,
  after which brisk-service.sh runs serve itself. Linger is already on; setcap for patched Sunshine is
  already done (sunshine-brisk.sh install asks via sudoq if it ever is not).
- Troubleshooting: `scripts/brisk-service.sh doctor` first. Page shows "This host doesn't know this browser":
  open the link from `scripts/brisk-service.sh link` once (the page keeps the secret in localStorage) or
  allow the login. Unit restarting: `logs` (a manual brisk-up holding 8443 = "bind ... address in use").
  Streams fail with "tailscale gave no IPv4": tailscale is down on bawn. 403 on every request through
  https but fine on localhost: the Host header is not allowed (hostd learns the ts.net name from
  `tailscale status --json` at startup and again, at most every 30 s, when an unknown *.ts.net name
  arrives; or set `BRISK_ALLOWED_HOSTS`).

### Static files (web/dist)
`crates/brisk-hostd/src/site.rs`: tower-http 0.7.1 `ServeDir` behind a small backend and wrapper.
Static pages need no secret or login (only the Host check); /api/* is unchanged.

| Request | Answer |
|---|---|
| `/`, `/library`, `/library/`, `/library/index.html` | the page; a directory serves its index.html, no trailing-slash redirect |
| unknown path without an extension (`/stream/deep`, `/Library`, a directory without index.html) | root `index.html` (client-side routes) |
| missing path with an extension (`/_astro/gone.js`, `/INDEX.HTML`, `/favicon.ico`) | 404, never index.html |
| `..`, `.`, `%2e%2e`, encoded `/` `\` NUL (`%2f` `%5c` `%00`), a literal `\` or NUL, bad UTF-8 | 404 (before 2026-10-26 the encoded forms got index.html) |
| a path whose real file is outside the current build (symlink out, `/up/...`), a dangling symlink with an extension | 404 |
| a FIFO, socket or device with an extension, a directory named like a file (`x/index.html/`), a symlink loop, a name over 255 bytes | 404 (a FIFO without an extension is an unknown page: the index) |
| method other than GET/HEAD | 404 (as before) |
| `Range: bytes=a-b` | 206 + `Content-Range`; unsatisfiable 416; multi-range refused (416); `If-Range` is ignored by tower-http (a range is served even if the validator changed) |
| `If-None-Match` matching the ETag (weak comparison, lists and `*` handled), or `If-Modified-Since` >= Last-Modified | 304 (the ETag is strong, no `W/`: size + mtime in ns) |
| HEAD | headers and Content-Length, no body |
| `Accept-Encoding: br` / `gzip` and a `<file>.br` / `.gz` sibling exists | the sibling, `Content-Encoding`, original Content-Type, `Vary: accept-encoding` (nothing compresses at build time yet) |
| a `.br`/`.gz` sibling that is refused (escapes, FIFO) | 404 for the plain file too, for clients that accept that encoding: tower-http falls back to the plain file only on NotFound. Fail-closed on purpose |

Headers on every static answer: `Cache-Control: public, max-age=31536000, immutable` for a 2xx/304 of a
file with an extension under `_astro/` (Astro's hashed names; an extensionless miss there gets the index
with `no-cache`), `no-cache` (always revalidate, cheap with the 304) for everything else,
404s included; `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, `X-Frame-Options: DENY`,
`Cross-Origin-Resource-Policy: same-origin`. Content-Type from mime_guess, with `text/html; charset=utf-8`
and `text/javascript` for `.mjs` (woff2, svg, json, wasm are right as guessed). API answers (and every
403 from the guard) gain only `nosniff`. Measured 2026-10-26 on the real build in-process: `/` 200 with
ETag + Last-Modified, then 304 with the ETag; a JS chunk `Range: bytes=0-99` 206 `bytes 0-99/479`;
`HEAD /library` 200 with Content-Length 14753.

Containment: `web/dist` is canonicalized once per request (so a request never mixes two builds and the
`update` symlink swap needs no restart; tests/hosting.rs swaps it between requests), and the backend
canonicalizes every file it opens or stats, precompressed siblings included, refusing anything not inside
that root. It then opens the canonical path by name with `O_NONBLOCK | O_NOFOLLOW` (a FIFO cannot block a
thread, a final component turned into a symlink is ELOOP) and reads only if fstat says regular file;
stat answers report only regular files and directories. Honest limit: canonicalize-then-open is two steps,
so a writer with toby's uid swapping a directory component inside the build between them could redirect
the open. Builds are not written after the swap and such a writer can read everything anyway: accepted.
Cheap improvement when wanted: rustix with its `fs` feature, `openat2(root_fd, rel, RESOLVE_BENEATH |
RESOLVE_NO_MAGICLINKS)` + fstat `is_file` and our own File wrapper (already there: `DiskFile`), which
removes the race and the canonicalize. Tests: tests/static_files.rs.

**Embedded build (optional, cargo feature `embed`)**: compiles the web build into the binary with
`include_dir` 0.7.4 and serves it through the same `ServeDir` code (an in-memory tower-http `Backend`), so
the rules and headers above are identical; tests/static_files.rs runs every case against both backends.
- Single-binary release: `cd web && npm run build`, then
  `cargo build --release -p brisk-hostd --features embed` (copy just `target/release/brisk-hostd`).
  `BRISK_EMBED_DIR=<dir>` (absolute, or relative to the repo root) embeds another built dist. build.rs never
  runs npm; without an index.html it fails with "feature `embed`: ... has no index.html", and it refuses a
  tree with a dangling symlink, a symlink out of it, a symlinked directory, or a FIFO/socket/device (any
  non-directory must be a regular file, or include_dir would hang the compile). Last-Modified of every
  embedded file is `BRISK_EMBED_STAMP`, else `SOURCE_DATE_EPOCH` (reproducible builds; set it per commit,
  `git log -1 --format=%ct`, because the ETag is size + that time), else the time build.rs ran (it reruns
  whenever the tree changes).
- Runtime: `BRISK_STATIC=embed|disk`, default disk even when compiled in; embed when asked or when the
  dist dir is missing at startup (unless `BRISK_STATIC=disk`). hostd logs which on stderr ("static files
  from disk: ..." / "static files embedded in the binary (N entries)"); the missing-dist fallback adds a
  `WARNING: ... serving the build EMBEDDED in this binary until hostd restarts` line, because it sticks
  for the process lifetime (a later `update` swap is not seen). The systemd status text says which:
  `serving 127.0.0.1:8443, static=disk|embed, stream idle` (`brisk-service.sh doctor` prints it).
  Without the feature, `BRISK_STATIC=embed` warns and uses disk.
- Size: +1.34 MiB on a 1.6 MB build (release hostd 8.34 MB disk-only, 9.74 MB with the embed of the
  2026-10-26 build). The embedded build is memory-mapped with the binary, no copy per request.
- Not used by the service: deploy/brisk-hostd.service and `brisk-service.sh update` stay on disk mode,
  because update/rollback is the atomic `web/dist` swap. A changed page needs a new binary in embed mode.
- Tests: `BRISK_EMBED_DIR=crates/brisk-hostd/tests/fixtures/embed-dist cargo nextest run -p brisk-hostd --features embed`
  (any built dist works; the fixture keeps it runnable without npm).
- Why not rust-embed (what the research doc proposed): on 2026-10-26 its crates.io owner is a deleted
  GitHub account (`ghost_5546`), the repo moved off GitHub to a self-hosted forge, and 8.13.0
  (2026-10-07) has no matching release commit there (the newest says v8.12.0). include_dir is older
  (2024-06-17, MIT, owner and repo on GitHub still match) but small (proc-macro2 + quote, already in our
  graph) and has no runtime code we rely on beyond a static tree.

**CSP follow-up (not set yet, on purpose)**: a wrong policy blanks the page or the stream with nobody
awake to notice. Plan: (1) list what the build needs: Astro's inline `<script>`/hydration islands, inline
styles from Svelte/anime.js, WebGL shaders (no eval), workers (`worker-src 'self' blob:`?), wasm
(`'wasm-unsafe-eval'`), the WebTransport/HTTPS URL of the bridge on `<tailscale-ip>:4433`
(`connect-src 'self' https://<ip>:4433`, dynamic per stream) and fetch to /api. (2) Ship it as
`Content-Security-Policy-Report-Only` from site.rs `finish()` with hashes of the inline scripts (computed
at startup from the html files, or `'unsafe-inline'` for scripts first), check the browser console on every
page, the stream page in a real session and the UI smoke test. (3) Enforce once a week of use shows no
reports; then add `frame-ancestors 'none'` and drop X-Frame-Options.

### Auth model and its limits
- Host header (DNS rebinding): exactly `localhost:8443`, `127.0.0.1:8443`, `BRISK_ALLOWED_HOSTS`
  (comma list) and this machine's `Self.DNSName` from `tailscale status --json`, as `<name>:8443` only
  (no port-less, trailing-dot or IPv6 forms). Anything else is 403 before any route runs, the secret does not help.
- Every /api request needs one of: the `x-brisk-secret` header (128 random bits, `~/.config/brisk/secret`,
  0600, made once; `BRISK_SECRET` overrides for tests; `BRISK_SECRET_FILE` moves it), or a tailnet login:
  the peer is loopback, `Tailscale-User-Login` is in `BRISK_ALLOWED_USERS` (comma list) or
  `~/.config/brisk/allowed-users` (one per line, `#` comments; ignored, with a warning, unless it is a
  regular file (not a symlink) owned by hostd's uid and not group/world-writable; read at startup, so restart after editing), and the request carries `x-brisk-csrf` (the page
  always sends it). The allowlist is empty by default: feature off, secret required as before.
- Why `x-brisk-csrf`: tailscale serve attaches the laptop's identity to every request from the laptop,
  including one a hostile web page makes cross-site (a `text/plain` POST needs no preflight). A custom
  header forces a CORS preflight, which hostd never answers with CORS headers, so only same-origin pages
  can send it.
- The limit, honestly: tailscaled is not the only thing that can connect from loopback. Any local process
  on bawn can open 127.0.0.1:8443 and send `Tailscale-User-Login: <allowed login>` itself. That is the
  same trust level as being able to read `~/.config/brisk/secret` as toby (any process running as toby
  can do both), but it is weaker than the secret against other local users: on a multi-user box any
  user could spoof the header. bawn has one user, so it is acceptable there; do not enable the
  allowlist on a shared machine. [U] that tailscale serve strips client-sent `Tailscale-User-*` headers
  before adding its own and keeps the Host header (not checked on bawn yet; the doctor's 403 check and
  the runbook's spoof test cover it).
- Page: works with no secret (the tailnet login path); a captured `?s=` is kept in sessionStorage and
  localStorage (try/catch), so a bookmark of the bare URL keeps working. Any 403 sets the `denied` store
  (web/src/state/api.ts) and the Shell replaces the page with "Open this page from the link that
  includes ?s=… or ask hostd to allow your login"; the next successful call clears it.

## API (hostd, 127.0.0.1:8443 only; Host header checked; every /api request, GET included, needs the `x-brisk-secret` header or an allowed tailnet login (see Hosting), because GET /api/stream carries the bridge URL and session token; static pages and assets do not)
- GET /api/games, GET /api/host, GET /api/sessions (last 100, no series), GET /api/sessions/<started>,
  PUT /api/sessions/<started>/series (see Session history)
- GET/POST/DELETE /api/stream (state / start / stop the bridge)
- GET /api/prefs, PUT/DELETE /api/prefs/<appid> (see Per-game quality); GET also returns `homeOrder` (Home card ids,
  `[]` when unset); PUT /api/prefs/layout `{homeOrder: string[]}` (400 unless an array of strings; ids not 1..32 of
  `a-z 0-9 -`, repeats and anything past 32 ids are dropped; answers the stored order), DELETE /api/prefs/layout = Reset
  layout (204). In prefs.json as `"layout": {"v": 1, "homeOrder": [...]}`, absent when unset; another `v` loads as unset.
- POST /api/launch, POST /api/install, POST /api/sunshine/restart
- POST /api/quit `{appid, force?}` (see Quit game)
- GET/POST /api/clipboard, PUT /api/files/<name> (see Clipboard and file drop)
- anything else: the static page from `web/dist`

Guard: the Host header must be on the allowlist (`localhost:8443`, `127.0.0.1:8443`, the tailnet name; DNS rebinding).
There is no Origin check; that is safe because the secret (or the `x-brisk-csrf` header, for a tailnet login) is
required and hostd sends no CORS headers, so other origins cannot read or send them.
Disconnect (ghost bar) stops the host stream (DELETE /api/stream); leaving the Stream page only detaches the viewer and
the bridge keeps running. The game itself keeps running on bawn: GET /api/games marks it `running` (its Steam reaper,
`SteamLaunch AppId=<id>`, is in /proc), the page says Resume, and POST /api/stream with its appid skips
`steam://rungameid`. Either way, once the tap connects hostd polls up to 60 s for a sway window in the reaper's process
tree and runs `swaymsg '[con_id=N] focus, fullscreen enable'`; no window or no sway is only logged.

## Quit game
- UI: a Quit button on the Library hero and on cards of running games, per game under "Running apps" on Machines,
  and "Quit <name>" in Ctrl+K (running games only). All open one confirm (shell/QuitDialog.svelte, Bits Dialog:
  "Quit <name>? Unsaved progress may be lost.", Cancel / Quit; focus returns to the button). While hostd works
  (up to ~25 s) the buttons say "Quitting…" and are aria-disabled. Then the games store refreshes and a toast says
  "Closed <name>" or "Still running: <name> (try Force quit)" with a Force quit button (30 s), which asks again and
  retries with `force: true`; while a game is in that state Ctrl+K also offers "Force quit <name>". A 409
  "already quitting" (another tab or button got there first) is an info toast "Already closing <name>…" plus a
  refresh, not an error. User copy never says "kill". Logic and tests: web/src/state/quit.ts.
- hostd (running.rs `Quitter`): 400 unless the appid is a known game, 409 if it is not running or a quit of it
  is already under way (one per game; other games are not held up). Steps: `swaymsg '[con_id=N] kill'` on every
  sway window whose pid is a descendant of the game's reaper (sway's polite close, like the X button); poll 10 s
  for the reaper to exit; else SIGTERM to the reaper's descendants (not the reaper, never steam, the
  webhelper/Big Picture or another game's tree; the list is rescanned just before each signal); poll 10 s.
  `force` skips the window close: SIGTERM, 3 s, SIGKILL to the same set, 3 s. A reaper with nothing left
  under it gets the signals itself (never its parent, Steam) with 3 s waits. Answer
  `{state: "closed" | "still-running"}`; past 25 s it answers still-running and the quit carries on. The quit runs
  in its own task, so a client hanging up does not stop it halfway. Nothing touches the bridge or Sunshine, so
  the Desktop / Big Picture stream keeps going.
- Signals go through pidfds (`rustix`, no `kill` process, no kill(2) by number): for each scanned pid hostd
  calls `pidfd_open`, then re-reads `/proc/<pid>/stat` starttime and cmdline and compares them with the scan;
  only a match is signalled, with `pidfd_send_signal` on that fd, so a pid recycled between scan and signal
  is skipped (the ppid is not compared: helpers are reparented when run.sh exits mid-quit). ESRCH = already
  gone (fine); EPERM and skipped pids are a stderr warning with counts; any other error (ENOSYS on a kernel
  before 5.3) stops that round with an error and no fallback.
- sway's close on an XWayland window sends WM_DELETE_WINDOW; for an X11 window without that protocol wlroots
  disconnects the X client instead (`xcb_kill_client`), which ends that client only, still one of the game's.
- hostd must run as toby, never root: as euid 0 it logs a warning at startup and POST /api/quit answers 503.
- Tests use a fake Runner (`swaymsg`) and a fake `Signaller` over a fake /proc shaped on bawn's real tree;
  nothing real is signalled (`cargo test -p brisk-hostd --test quit`; tokio time is paused, so no real waits).
  They cover a recycled pid (not signalled), ESRCH, EPERM and no pidfd support. running.rs has real-process
  tests: a `sleep 60` in its own process group SIGTERMed through the real pidfd Signaller, and refused
  when its starttime does not match the scan.
- [U] Not tried on bawn: whether Proton/XWayland games exit on sway's close (Pepper Grinder is native XWayland; a
  Proton game may show a "really quit?" prompt and fall through to SIGTERM), that Steam's overlay
  (gameoverlayui, a child of steam) and the Running badge clear by themselves once the reaper exits, that sway
  focuses Big Picture again after the last game window closes, and the confirm's focus trap/Esc in Chrome.

## Per-game quality
- Each game remembers its own quality (preset, codec, and width/height/fps/bitrate for `custom`), with no extra
  clicks. Changing quality during a stream of a game (ghost bar chips, Advanced Apply, Ctrl+K presets) saves it for
  that appid after 1 s of quiet (last change wins; leaving the page flushes a pending one). The Desktop / Big
  Picture stream (no appid) and a failed switch save nothing. The ghost bar's Quality menu then shows "Saved for
  <game>" and "Forget for this game" (only while saved).
- Starting a game (Library click, Resume card) sends its saved quality, else Settings' default preset on h264; the
  Stream page's own Start (the Desktop) uses the default. One pure function decides: `prefsFor` in
  web/src/state/prefs.ts, used through `startGame`. Library cards and the hero show a chip of the saved quality
  ("Crisp", "HEVC · 40 Mbps", "Custom · 8 Mbps") only when saved. The store refreshes on focus (at most once a minute).
- hostd (prefs.rs): `<data_dir>/prefs.json`, `{"<appid>": {preset, codec, width?, height?, fps?, bitrateKbps?, updated}}`,
  written to a 0600 temp file in the same directory, fsynced and renamed; a missing or corrupt file (or a bad entry) loads
  as empty. `PUT /api/prefs/<appid>` validates like StreamReq (preset fast|balanced|crisp|custom, codec h264|hevc, 320..3840 px,
  24..144 fps, 1000..150000 kbps; custom numbers are kept only for `custom`), 400 otherwise and for the Desktop app id;
  `DELETE` is idempotent (204). Capped at 500 entries, oldest `updated` dropped. All need the secret.
- Not done: Ctrl+K "Launch <game>" only starts the game on the host (`/api/launch`, no stream), so it has no quality to
  apply; the saved quality is used when the game is then opened from Library or Resume. A saved HEVC on a browser
  without HEVC decode starts in H.264 for that start (toast "This browser cannot decode HEVC, starting in H.264");
  the saved entry stays.
- Saving: Forget waits for a save still in flight, then DELETEs, and that save's answer is dropped, so Forget
  cannot be undone by a late PUT. A pending change is flushed on `pagehide` too (the PUT uses fetch `keepalive`).
- [U] Not tried on hardware: saved quality across a real Library -> stream -> quality change -> Disconnect -> Library round trip.

## Session history
- `/history` (sidebar "History", chart icon, `--c-hist` orange): one card per stream (last 100), with game, date and
  time, length, quality ("Crisp · HEVC · 40 Mbps"; older records only have the preset), a delay sparkline and the
  bridge's PASS/FAIL gate (the sparkline comes with the list: `spark`, 24 mean-binned delays per row, so the list
  makes one request). A card opens `/history?session=<started>` in place (pushState; Back returns): delay, frame
  rate and bitrate over the session as LayerChart area charts with a hover tooltip (m:ss, one decimal; each chart
  has a visually hidden min/average/max and peak-time summary, the plot is aria-hidden), and the bridge report as a table
  (p50 / p99 latencies, docs/m0-baseline.md). Home's "Recent sessions" title and rows link here. Empty, unreachable
  and old-record (no series, no report) states say so.
- Recording: the Stream page passes each 500 ms stats sample with frames (delay estimate, fps > 0, Mbit/s; the same
  numbers as the stats card) to `SeriesUploader` (web/src/modules/stream/series.ts), which averages them to one point
  per 5 s and PUTs them every 30 s and on Disconnect, leaving /stream and `pagehide` (fetch keepalive). Samples stay
  pending until a PUT of them succeeds (at most 4000 samples, ~33 min, oldest dropped), so a failed PUT is simply
  re-sent; the current, partial bucket is sent again until complete (hostd keeps the later point for a t). A 404 or
  413 ends uploads for that session quietly. Uploads are never awaited.
- Time base: t is hostd's seconds since `started`, not the laptop's wall clock. GET/POST /api/stream carry
  `elapsedS` (hostd's monotonic Instant since the session's record was made; null when idle); the page adds its own
  `performance.now()` since that answer, and the uploader then counts on from its first sample. A reload of /stream
  mid-session continues from hostd's elapsed and skips the rest of the 5 s bucket it lands in, so earlier points are
  never rewritten. Only while the page shows the stream: a bridge running with no page records no series.
- hostd: the bridge's end-of-session report (stdout, see stream.rs) is parsed line by line into the record's `stats`
  `{durationS, frames, sent, dropped, resets, idrRequests, bridgeP50/P99, hostP50/P99, audioLost, audioErrors,
  inputReceived/Sent/Dropped, gate}`; parsing starts at the report's first line (`tap_frames`), and a missing or
  garbled line leaves its field null (never clearing a value already parsed). A quality change restarts the
  bridge inside one session: counts add up, p50 is frame-weighted, p99 the worst, any FAIL fails. Records also keep
  `codec` and `bitrateKbps` (the last ones of the session); old lines without them still read. The live session's id
  is `started` in GET/POST /api/stream.
- Series: `<data_dir>/sessions/<started>.series.json`, 0600, written to a temp file and renamed.
  `PUT /api/sessions/<started>/series {points: [[t, delayMs, fps, mbps], ...]}` merges by t; 400 for a point that is
  not 4 finite numbers in range (t 0..1 week, delay 0..60000, fps 0..1000, 0..10000 Mbit/s; JSON has no NaN), 413
  for more than 17280 points in the request or once merged (5 s each: 24 h; then nothing more is stored) or a body
  over 1 MiB, 404 for a session hostd has no record of. PUTs are serialized. Only the series of the newest 200
  sessions are kept; leftover `.*.tmp` files of a crashed write are swept then.
  `GET /api/sessions/<started>` = `{session, series}`. All need the secret.
- Charts: `layerchart` 2.6.1 (MIT, Svelte 5 native; its d3 deps are ISC/BSD-3/MIT), the `layerchart/svg` entry,
  no Tailwind: it styles itself from CSS variables (`--color-primary`, `--color-surface-100/200/300`,
  `--color-surface-content`), mapped to our tokens in modules/history/Charts.svelte, plus `layerchart/core.css`
  (only its cascade-layer order). No chart motion is configured, so nothing animates in. Charts.svelte is a dynamic
  import inside the detail view: Home, Library and Stream fetch none of it (headless-checked), the History list
  neither; opening a session fetches ~390 KiB of JS uncompressed (~120 KiB gzip; LayerChart + d3).
- [U] Not tried with a real stream: the series upload during a live session on the laptop, the pagehide flush
  when the tab is closed mid-stream, and the report parse against a real bridge exit (the parser is tested on a real
  report from a brisk-up log; the fake bridge in tests prints it).

## Clipboard and file drop
- Clipboard sync (on by default; ghost bar clipboard panel or Settings, key `clipboardSync`) carries text and PNG
  images both ways while the stream is live and the page is focused. Laptop -> bawn: on window focus, tab visible,
  pointer lock and stream start the page reads `navigator.clipboard` and POSTs it if its SHA-256 differs from the
  last synced one. bawn -> laptop: the page polls `GET /api/clipboard?since=<seq>` every second (204 = unchanged,
  JSON for text, the PNG body for an image; `x-brisk-seq` on every answer; no `since` = only the seq, so a fresh
  page never pulls bawn's old clipboard) and writes it with `navigator.clipboard`.
- hostd uses wl-clipboard (`/usr/bin/wl-paste`, `wl-copy`, installed by Toby; wl-clipboard-rs fails
  scripts/check-pure-rust.sh: `cc` and `wayland-sys` via wayland-backend) in the sway session (WAYLAND_DISPLAY from
  the steam process like stream.rs, else the first `wayland-*` socket in the runtime dir). A watcher thread runs
  `wl-paste --watch echo` (dies with hostd, restarted if it exits) and on each event reads `--list-types` then the
  PNG or text, each under `timeout 5`. A seq bumps when the content hash changes. A POST records the hash before
  `wl-copy --foreground --type <mime>` takes the selection, so the watcher does not report it back; the page also
  keeps the last hash (and reads an image back after writing it, as Chrome re-encodes PNGs), so nothing ping-pongs.
  The previous wl-copy is given 1 s to exit by itself, then killed and reaped.
- Limits: 16 MiB per clipboard both ways (bigger is skipped with one toast); other types (files, HTML only) are
  ignored. If Chrome refuses clipboard access the page shows one toast and the panel says how to allow it; while the
  page is unfocused nothing is read and a pending write waits for focus.
- Privacy: with sync on, every copy on the laptop is sent to bawn while the stream has focus, so a password copied
  from a password manager lands on bawn's clipboard (and stays there until something else is copied, even after
  hostd exits). Turn sync off for that. Clipboard contents are never logged.
- Files go laptop -> bawn only: drop files on the Stream page (a "Drop to send" overlay; needs a free pointer, not
  while it is locked) or "Send files…" in the panel. One upload at a time, XHR with progress, a toast with the saved
  path. `PUT /api/files/<name>` streams the body into a hidden temp file in `~/Downloads/brisk` (created on first
  use) and hard-links it to a free name: basename only, no control characters, no leading dots, at most 200 bytes,
  `name (1).ext` if taken. Max 8 GiB (413); with a Content-Length it refuses when less than that plus 256 MiB is
  free (507), and re-checks free space every 64 MiB written; the same name twice at once is a 409; a cut-off
  upload leaves no temp file (and hostd removes `.*.part` leftovers at startup). Files are 0600, the directory 0700
  when hostd creates it. Bidi and zero-width characters are stripped from names.
- [U] Chrome's clipboard permission prompt and `navigator.clipboard.read()` on the pointer-locked canvas, and a
  real image copy both ways, were not tried on hardware; the hostd side was checked live against bawn's sway
  (`cargo test -p brisk-hostd -- --ignored live_`: sets text, reads it with wl-paste, clears it).

## Screenshot and replay clip
- All in the browser, nothing on the host: files download to the laptop (`a[download]`) as
  `brisk-<game or desktop>-<YYYYMMDD-HHMMSS>.png|.mp4` (local time; the game name slugged). Nothing shows on
  screen while playing except the toast ("Saved <name> · copied", "Saved <name> · 30 s · 148.2 MB").
- Ghost bar: Screenshot, and "Save last N s" (N from Settings; disabled when Off). Hotkeys Ctrl+Alt+S
  (screenshot) and Ctrl+Alt+R (clip) are caught on `window` in the capture phase (`reserveHotkeys`,
  capture.ts), so input.ts (window, bubble phase) never sees them and they never reach the game; their keyup
  is swallowed too. The Ctrl and Alt presses before them do reach the game. Chrome has no Ctrl+Alt+S/R
  shortcut on Linux, Windows or macOS. Shift, Meta or AltGr held = not ours (GNOME's Ctrl+Alt+Shift+R
  recorder is untouched; on Windows, where AltGr reports Ctrl+Alt, an AltGr+S character such as Polish ś
  still goes to the game).
- Screenshot: `canvas.toBlob('image/png')` of the video canvas, which is the stream's native size (video.ts
  sizes it to each frame) and holds the last drawn frame (frames are drawn on arrival). Downloaded as soon as
  the PNG is ready; also written to the laptop clipboard when Chrome allows it (ClipboardItem with the PNG
  promise, inside the key/click gesture). ClipSync adopts that image (`adopt`), so clipboard sync does not
  send it to bawn.
- Replay buffer (Settings: Off / 15 / 30 / 60 s, default Off until measured at Crisp, key `replaySeconds`;
  read at each connect): web/src/modules/stream/replay.ts. video.ts hands every access unit that goes to the
  decoder (Annex B, key flag, the frame header) and audio.ts every parsed Opus packet (ts_48k) to one optional
  callback each, by reference (no copy, no await; when Off no callback is passed). Video times are capture_ns,
  or bridge_ns when the tap left it 0. The ring keeps N s plus up to one key interval: the front GOP goes only
  once the next key frame is at or before now - N s, so the latest key frame is never evicted. Byte cap 160 MiB,
  counting each entry's whole buffer (what it keeps alive) + 64 B, oldest first; with one GOP left its newest
  delta goes and recording pauses until the next key frame. One ring per connection, cleared and killed on
  disconnect, reconnect, quality change and leaving /stream.
- Sunshine sends no periodic IDR, so while replay is on (never when Off) the page sends `0x02`
  (MSG_REFRESH_KEY, docs/m2-notes.md) every 10 s without a key frame: the bridge asks for an IDR, at most once
  a second, and drops nothing (unlike `0x01`, which drops deltas until the IDR). Cost: one IDR per 10 s.
- Clip (in the worker, clip-worker.ts): from the last key frame at or before now - N s (else the earliest)
  among those whose SPS/PPS(/VPS) equal the newest key frame's (a resolution change mid-buffer clips only the
  last config), so a clip is N to N+10 s. Audio: packets overlapping that video, placed by their own ts_48k (no
  drift, u32 wrap and reordering handled), mapped to the video clock by the least-queued packet and frame (min
  arrival - send of each); gaps (lost, or a silent host that stops sending) are filled with 1-byte Opus packets
  (the TOC only = a lost frame: the decoder conceals, then silence). Opus pre-skip 120 (libopus lookahead in
  RESTRICTED_LOWDELAY, 2.5 ms). Muxed with mediabunny 1.61.4 (MPL-2.0; mp4-muxer is deprecated in its favour):
  given Annex B and no description it builds the avcC (with the High-profile chroma/bit-depth fields) / hvcC
  from the key frame's in-band parameter sets and rewrites the NAL units length-prefixed; moov at the end,
  written to Blob parts, each packet dropped once written. The main thread only snapshots the ring (0.1 ms)
  and posts it in 2 MB batches (one copy each, transferred, a yield between batches); 30 s timeout.
- Checks: `npm test` (replay.test.ts: ring eviction and retention, key frame choice, config change, audio
  gaps, and a mux of the ffmpeg fixtures in fixtures/ (gen.sh: H.264 Baseline and High, HEVC, Opus) probed
  with ffprobe and decoded by ffmpeg). `node web/scripts/clip-check.mjs [x.mp4 ...]`: headless Chrome screenshot
  of the latest frame, a Crisp-size ring (155 MB) clipped while a 60 fps canvas loop runs (no long task over
  50 ms; Chrome RSS +400 MB at the peak: the copy in the worker, the MP4 parts, the Blob), the hotkeys against
  input.ts with real key events, and MP4s played in a `<video>`.
- [U] Not tried with a real stream: clip A/V sync on bawn, the refresh-IDR cost at Crisp, Chrome's download
  prompt and clipboard permission while pointer-locked/fullscreen, and HEVC clip playback (headless Chromium
  has no HEVC decoder: the HEVC MP4 loads with duration 3 s but 0x0 video; ffmpeg decodes it clean).

## Env vars
BRISK_APP (default 870779643, Desktop), BRISK_LISTEN (bridge address; default the tailscale IPv4:4433, asked at each stream start),
BRISK_DIST, BRISK_DATA_DIR, BRISK_BRIDGE_BIN, BRISK_STEAM_DIR, BRISK_STEAM_KEY_FILE
(`~/.config/brisk/steam-api-key`, never printed or committed), BRISK_STEAM_API_BASE, BRISK_SECRET_FILE
(default `~/.config/brisk/secret`), BRISK_SECRET (tests), BRISK_TLS_DIR (default `~/.config/brisk/tls`),
BRISK_ALLOWED_HOSTS, BRISK_ALLOWED_USERS, BRISK_SPAWN_SCOPE (1 = `steam` in its own systemd-run scope; set by the unit),
BRISK_STATIC (`disk` default, `embed` with the embed feature; see Static files). Build time only: BRISK_EMBED_DIR.

## Rust dev tooling
- **Toolchain pin**: `rust-toolchain.toml` names an exact release (1.99.1) with `profile = "minimal"` and
  `components = ["clippy", "rustfmt"]`. rustup can install a missing pinned toolchain on the next
  `cargo` call, but `scripts/brisk-service.sh update` does not rely on that: before the capped build it runs
  `rustup toolchain install` (reads rust-toolchain.toml, 900 s timeout, needs network once after a bump) and
  aborts before any swap or restart if that fails.
  Bump deliberately about every 6 weeks: edit the version, `cargo update`, run the checks below.
  Docs and build tools are not in `minimal`; add a component to the list if a build needs it.
- **Edition 2024** for all crates (`edition.workspace`); `publish = false` on all of them.
- **Lints**: `[workspace.lints]` in the root `Cargo.toml`, inherited with `[lints] workspace = true`.
  `unsafe_op_in_unsafe_fn = deny`; clippy `dbg_macro`, `todo`, `unimplemented`, `undocumented_unsafe_blocks`
  = warn (every `unsafe` needs a `// SAFETY:` comment right above the statement). No print lints (bridge and
  hostd print on purpose); `unwrap_used` is skipped on purpose: src has 112 `unwrap()`s today, so the lint would only add noise.
  nextest runs one process per test, which also avoids races on the env vars one `tap_live.rs` test sets.
- **Checks** (all must pass; use `. scripts/capped.sh; capped cargo ...` for builds on bawn):
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo nextest run --workspace` (profile `ci` = no fail-fast, config in `.config/nextest.toml`),
  `cargo test --doc --workspace` (nextest does not run doc-tests), `cargo deny check`,
  `scripts/check-pure-rust.sh`, `scripts/test-*.sh`. The optional `embed` feature needs a built dist at
  compile time, so its checks set one: `BRISK_EMBED_DIR=crates/brisk-hostd/tests/fixtures/embed-dist cargo
  clippy --workspace --all-targets --features brisk-hostd/embed -- -D warnings` and the same for
  `cargo nextest run -p brisk-hostd --features embed`.
- **cargo-deny** (`deny.toml`): graph limited to the six native targets (quinn-proto's wasm-only `ring` edge is
  out of scope); allowed licences MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0, Zlib, CC0; bans `proc-macro1`
  (the Aug 2026 typosquat), `openssl-sys`, `aws-lc-sys`, `ring`, wildcards; sources crates.io only. Seven advisories
  are ignored on purpose, each with a reason and a TODO in `deny.toml`: five `rustls-webpki 0.102.8` (from
  `rustls-rustcrypto`, only `alg_id` constants are used: unreachable), `rsa` Marvin (a direct dependency of
  brisk-proto and brisk-client; private-key use only in the client, so it needs a local attacker; kept until a
  fix or replacement) and `crypto-bigint` 0.5.5 (via p256/p384 from `rustls-rustcrypto`; REACHABLE: every
  bridge handshake signs with P-256 for any tailnet peer, accepted because remote timing is impractical).
  `exclude-dev = true` means advisories in dev-only crates are not reported.
  Duplicate-version warnings are expected. Install: `cargo install --locked cargo-nextest cargo-deny`
  (cargo-deny's `zstd-sys` needs a C compiler and free space in /tmp; the tool is not part of the product).
- **Purity allowlist**: `scripts/check-pure-rust.sh` fails on any `*-sys`/`*_sys` crate or `cc`, `cmake`,
  `bindgen`, `pkg-config`, `vcpkg`, `cxx-build` in the x86_64-linux normal+build graph unless
  `scripts/pure-rust-allow.txt` has a line `crate | reason | date`. One entry today: `linux-raw-sys`
  (rustix's syscall tables, pure Rust, no C; added 2026-10-26 with the pidfd work); read the header of that
  file for why `libc`, `autocfg` and `windows-sys` are not listed. graviola (Steam HTTPS) passes without an
  entry: its verified assembly is Rust `global_asm!`, no C compiler runs.
- **New crates: 14-day rule.** Pin new dependencies exactly (`=x.y.z`) to the newest release at least 14
  days old (crates.io publish date), including new transitive crates (`cargo update -p <crate> --precise`),
  unless it is a security fix. 2026-10-26: ureq 3.4.2 (3.5.0 too new), rustls-graviola 0.4.0, graviola
  0.4.1, rustls-platform-verifier 0.7.1, rustix 1.1.5, sd-notify 0.5.0, tower-http 0.7.1 (2026-08-31,
  feature `fs` only), include_dir 0.7.4 (optional, `embed`); new transitive with tower-http: tokio-util
  0.7.20 (2026-10-09), unicase 2.10.0 (2026-10-05), mime_guess 2.0.5, http-range-header 0.4.2,
  futures-sink 0.3.34, all MIT or MIT/Apache; transitive ureq-proto 0.6.4,
  rustls-native-certs 0.8.4, webpki-root-certs 1.0.9, combine 4.6.8, schannel 0.1.29 held back. `scripts/test-check-pure-rust.sh`
  proves the script with a stub `cargo`.

## Fuzzing and property tests
Every parser of bytes Brisk did not produce (network, tap, browser, Sunshine HTTP/RTSP/SDP, control,
input, audio) is listed with its trust level and coverage in `docs/fuzz-inventory.md`, which also
records what this found (three bugs fixed, among them a key press lost after a send timeout; one
behaviour described and left; one bounded worst case noted).
- **Property tests** (proptest =1.12.0, dev-dependency, `default-features = false, features = ["std"]`:
  no fork/timeout, inputs are kept small instead) run with everything else: `cargo nextest run --workspace`.
  Files: brisk-proto `tests/prop_parsers.rs` (any input, no panic, peak heap under 16 B per input byte
  + 64 KiB, measured by a counting `#[global_allocator]` in that test binary) and `tests/prop_roundtrip.rs`
  (round trips, Reed-Solomon under random loss, AES wrappers), brisk-bridge `tests/prop_input.rs` and
  `tests/prop_audio.rs`, and `#[cfg(test)]` modules in brisk-client `session.rs`, `tap.rs`, `rtsp.rs`.
  Default 256 cases per property (41 tests, about 4 s of test time in parallel; the suite's wall
  time stays about 10 s, set by a 10 s control timeout test); longer local runs:
  `PROPTEST_CASES=5000 cargo nextest run -p brisk-proto` (or `-p brisk-bridge`). A failure prints
  the shrunk input and writes `tests/<file>.proptest-regressions`; commit that file only for a real bug.
- **cargo-fuzz** (0.13.2, `cargo install --locked cargo-fuzz`, nightly: `rustup toolchain install
  nightly --profile minimal`) in `fuzz/`, eight targets: `rtsp`, `rtp_fec`, `control_enet`, `input`,
  `http_sdp`, `tap`, `audio`, `bridge_input` (list: `cargo +nightly fuzz list`). Run from `fuzz/`:
  `cargo +nightly fuzz run <target> corpus/<target> seeds/<target> -- -max_total_time=300 -rss_limit_mb=1024`
  (on bawn wrap it in `capped` and keep `CARGO_TARGET_DIR`/`TMPDIR` off the small /tmp: the
  ASan build of libFuzzer needs a few hundred MB). Stateful targets split the input into datagrams
  (`len u16 LE` + bytes, `fuzz_targets/common.rs`). `fuzz/seeds/` holds small hand-built valid
  samples (committed, about 5 KB); the generated `fuzz/corpus/`, `fuzz/artifacts/` (crash inputs)
  and `fuzz/target/` are git-ignored. Reproduce a crash: `cargo +nightly fuzz run <target> artifacts/<target>/crash-...`.
  Every real crash gets a minimal unit test in the crate first, then the fix.
- **Purity exception**: libfuzzer-sys compiles libFuzzer (C++, via `cc`) and needs nightly, which
  the pure-Rust rule forbids for the product. So `fuzz/` is its own workspace (empty `[workspace]`
  table, own `Cargo.lock`), not a member of the root workspace: the product build,
  `scripts/brisk-service.sh update`, `scripts/check-pure-rust.sh` and `cargo deny check` all read
  the root workspace graph and never see it. Never add `fuzz` to the root `members`.
  For the same reason deny's licence check never sees libfuzzer-sys, which is
  `(MIT OR Apache-2.0) AND NCSA` (libFuzzer's LLVM licence); NCSA is deliberately not in
  deny.toml's allow list, since nothing in the product uses it.
- **Keep `fuzz/Cargo.lock` in step** with the root `Cargo.lock`: after a `cargo update` or bump,
  copy the root lock over it and run `cargo +nightly fuzz build` (cargo adds only the fuzz crates),
  so the fuzzed code builds against the versions the product ships.

## [U]
`[U]` in code comments marks an unverified assumption about bawn (for example the graphical
environment taken from a running steam process).

## Shaders
- Background smoke only (Home, accent colour, drifts on its own). No pointer interaction and no card shaders; cards only lift and brighten their border on hover. Off on /stream, when the shaders setting is off, and without WebGL2 (CSS gradient instead). Toby's steer, 2026-10-25.

## How to run the smoke test
`cd web && npm run build && node scripts/ui-smoke.mjs` (needs `target/release/brisk-hostd`, or set `BRISK_HOSTD`; port 8443 free).
It starts hostd on fixtures, the fake bridge, a fake `steam` and a mock Steam API (sentinel key, 139 games) in a temp dir,
drives headless Chrome through Home, Library, Machines, History, Settings (every tab), the restart dialog, Ctrl+K,
accent, the 403 guard, a launch to `live` and the ghost bar, runs axe (see Dev tooling) on each of those views, then
prints PASS/FAIL (exit 0 only if all pass) and screenshot paths. It cannot prove video: WebTransport to the fake bridge
fails, so Stream is only checked for degrading to "Lost the connection" without page errors. hostd's listen port and Host
check are fixed at 8443, so while the live service holds 8443 the smoke prints `SMOKE SKIPPED (exit 3)` and exits 3
(exit codes: 0 pass, 1 fail, 3 skipped; `BRISK_SMOKE_ALLOW_SKIP=1` makes a skip exit 0). It is not part of `npm run ci`.
Use `web/scripts/axe-check.mjs` for the a11y part meanwhile.

## Dev tooling
- Commands (in web/): `npm run ci` = `npm run check` + `npm test` + `npm run build` (run it before merging);
  `npm run check` = `astro check` + `svelte-check` (fails on errors; one known svelte-check warning is left: GhostBar's
  `fileInput` is set by `bind:this` and only read in click handlers, never in the template or an effect, so it does not
  need `$state`; making it `$state` would only add reactivity nothing uses); `npm test` = the node unit project only (fast, no
  DOM); `npm run test:browser` = Svelte component tests in real Chrome (separate, not in `ci`).
- Pinning: `web/.npmrc` has `save-exact=true` and every dependency in package.json is an exact version. New packages and
  bumps take the newest version that is at least 14 days old on npm (`npm view <pkg> time --json`; for new transitive
  deps `npm install --before=<date>` where the tree allows it), unless the newer one is a security fix (vitest 5.0.5 and its
  browser packages, fast-uri 3.1.9). Check `npm audit`, `npm ls` and `hasInstallScript` in the lock after a change
  (today only `fsevents`, optional, macOS).
- TypeScript is pinned to 6.0.3: TypeScript 7 (the Go port) is out, but `svelte-check` and `@astrojs/check` declare
  `typescript ^5 || ^6`. Retry when they widen. `web/tsconfig.json` extends `astro/tsconfigs/strict` and includes src,
  `scripts/**/*.mjs` and `vitest.config.ts`; vite defines are declared in `src/env.d.ts`. The scripts are included without
  `checkJs`: turning it on gives about 450 errors, mostly the untyped `window.__x` probes inside `page.evaluate`. `@ts-expect-error` only with a one-line reason; no blanket `any`.
- Accessibility: `web/scripts/axe.mjs` (`@axe-core/playwright`, MPL-2.0) runs the WCAG 2.0/2.1/2.2 A and AA rules and fails
  on serious or critical violations; exceptions live in its `EXCEPTIONS` list with a one-line reason (today: the command
  menu listbox, scrolled from the input through aria-activedescendant). Used by ui-smoke and by
  `node web/scripts/axe-check.mjs` (against `npx astro preview --host 127.0.0.1 --port 4424`, mocked /api; Home, Library,
  Machines, History list + session, every Settings tab, Stream idle and failed, command menu, Quit and restart dialogs).
- Browser tests: `*.browser.test.ts` next to the component, run by the `browser` project in `web/vitest.config.ts`
  (`@vitest/browser-playwright` + `vitest-browser-svelte`, Playwright's own chrome-headless-shell; `BRISK_CHROMIUM`
  overrides it, and a missing one prints how to install it). Reduced motion is forced in the test browser, so no dialog or
  menu animation can make a test wait or flake. Its API server is pinned to `127.0.0.1:4460` (strictPort): Vitest browser mode had critical advisories
  for an exposed API, so never 0.0.0.0. `astro:transitions/client` is aliased to `src/test/astro-transitions.ts` (a `navigate`
  spy). Covered: CommandMenu (Ctrl+K, filter, Enter navigates, Esc returns focus), the Quit confirm (focus in, Tab trap,
  Esc, focus back; and with a test-only transition: still mounted with its text mid-exit, then unmounted), Settings tabs (arrows/Home/End and the hash), GameCard (a click on a not-installed card never installs).

## HEVC
- Select: ghost bar, Quality, Advanced..., Codec: HEVC, Apply. It reconnects like any custom quality
  (`POST /api/stream` with `codec: "hevc"`, hostd starts the bridge with `--codec hevc`, the SDP asks
  Sunshine for `bitStreamFormat` 1). H.264 stays the default for fresh launches; a preset change (chips, Ctrl+K) keeps the live codec.
- HEVC is offered only when `VideoDecoder.isConfigSupported({codec: 'hvc1.1.6.L120.B0'})` says yes, else
  the option is disabled ("not supported on this browser").
- The page does not trust the request: it reads the codec from the first key frame (H.264 SPS -> `avc1.*`,
  HEVC VPS/SPS/PPS -> `hvc1.*` from the SPS profile_tier_level, ISO 14496-15 E.3). The stats card shows it.
- The bridge refuses a tap whose codec field differs from the one it asked for (error in the bridge log), and
  with `--codec hevc` stops at once if Sunshine's serverinfo `ServerCodecModeSupport` lacks the HEVC bit 0x100
  ("Sunshine reports no HEVC support on this GPU").
- [U] Chrome decodes Annex B HEVC with no `description` (the WebCodecs HEVC registration says Annex B when it
  is absent; Sunshine sends VPS/SPS/PPS in-band on every IDR). [U] Chrome's key-frame check accepts the IDR
  that VAAPI hevc makes on an IDR request (it should be NAL type 19/20; the flag is Sunshine's key-frame flag,
  nothing in Brisk parses slice types). [U] `hvc1` vs `hev1` makes no difference to Chrome for Annex B.
  [U] the level Sunshine's VAAPI SPS carries (1080p60 needs 4.1, `L123`) is within the laptop decoder's limit.
- Toby to measure: the HEVC vs H.264 rows in docs/m0-baseline.md (same bitrate), and whether the stats card
  says `hvc1.*` with hw decode and 0 decode errors over a few quality switches.

## Controller test (Settings)
- Settings has a "Controller" card below the other settings (web/src/modules/settings/ControllerTest.svelte; pure logic and
  tests in controllertest.ts / controllertest.test.ts). It shows the pad picker (index, id, mapping, button and axis counts),
  a rolling poll rate (rAF ticks per second), an SVG pad diagram (aria-hidden; pressed buttons light up in the accent,
  triggers are fill bars), per-stick circle with crosshair, display-only deadzone ring (0-25%, default 8%), x/y as -1..1 and %,
  and a 2 s trail; a range test (32 angle bins, max extent drawn as the gate polygon, Reset), a 3 s drift check (mean offset,
  peak-to-peak noise, OK <2% / Minor 2-6% / Drift >6%, suggested minimum deadzone = ceil(offset + noise); cancelled with
  "You moved it, try again" if a button, a trigger over 30% or a stick over 30% moves), a mapping table (index, label, the
  Moonlight flag from `BUTTON_FLAGS` in lib/gamepad.ts, `analog u8` for triggers, `i16` for sticks with the Y-inverted note;
  the live row is highlighted) and a Weak / Strong / Both rumble test (`dual-rumble`, 400 ms).
- Lifecycle: the rAF loop runs only while the card is in view (IntersectionObserver), the document has focus and is visible;
  unmount cancels it, removes every listener (one AbortController) and resets the actuators. It reads `navigator.getGamepads()`
  only: no fetch, no hostd, nothing reaches the bridge, and it is not on the Stream route. Reduced motion needs nothing special
  (no animations; the live view is data). The polite status line (role=status, visually hidden, mirrored on screen) speaks only
  for connect/disconnect and the drift outcome.
- Chrome shows a pad only after its first button press, hence the hint when none is listed. Readings use the raw axes (y down
  is positive on screen); the table says what Brisk sends (y flipped). Headless-checked with a scripted fake
  `navigator.getGamepads`; [U] not tried with the real 8BitDo Ultimate 2 (rumble feel, real poll rate, real stick noise).
- The diagram (PadDiagram.svelte, geometry in pad.svg.ts, 1 unit = 1 mm, viewBox -80 -60 160 120) is the 8BitDo Ultimate 2
  Wireless to scale: body 147 x 103 mm (official spec), every position and size scaled from the official manual's front
  diagram (vector line art, body box set to 147 mm; outline within about 0.6 mm of it, controls within 0.3 mm). Our own
  geometry, no 8BitDo artwork or logo. Triggers, L4/R4 and the USB-C are a top-down projection above the top edge (from the
  manual's top view). L4/R4, PL/PR and Square/Star/Profile are drawn faint with a caption: Chrome's standard mapping only
  reports the 17 standard buttons. Sources and open questions: research/8bitdo-ultimate-2-dimensions.md. pad.svg.test.ts
  checks the body aspect, one element per standard button 0..16, and unique ids.

## Stream idle smoke (2026-10-26)
The Stream page's "Nothing is streaming" state shows the same accent smoke (opacity .5 on black). It is mounted only while `page === 'idle'`, so the WebGL context is destroyed before any session starts; boot, live session, reconnect and unsupported stay plain black. `smokeAllowed(path, shaders, idle)` still blocks /stream unless `idle` is set; the shaders setting still turns it off.
