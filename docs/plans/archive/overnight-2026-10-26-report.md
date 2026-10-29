# Overnight report, 2026-10-26

Done and merged. Archived; kept for history.

Everything in docs/plans/archive/2026-10-25-queue-tooling-hardening.md is built, reviewed, merged to local main (no remote, nothing pushed) and deployed (`scripts/brisk-service.sh update` at 05:20, doctor all OK, stream idle).

## Shipped
- Pass 1 tooling: FE (exact pins, bumps, TS 6 + svelte-check, axe, variable Geist, Vitest browser mode) and Rust (toolchain 1.99.1, edition 2024 + lints, nextest, cargo-deny, stricter purity script).
- Pass 2 Rust hardening: ureq + graviola (curl gone), tower-http ServeDir with containment, rustix pidfd for Quit game, sd-notify (Type=notify, watchdog), cargo-fuzz + proptest (found a quadratic XML parser and a gamepad index overflow, both fixed), optional `embed` feature.
- Pass 3 platform: dialog exits, popover/anchor tooltips, Library filter View Transition, wake lock on Stream.
- Motion pass 2: status-dot ring, icon morphs, hero split-text (in-house splitter), controller chips, connection path dot, Machines status transitions, Add-a-module picker swell, session cover -> detail header (View Transition), Home drag-to-reorder (handle, keyboard path, spring drop, order saved in hostd prefs `layout.homeOrder`, Reset layout in Settings > General).
- Last checks on main: nextest 457, clippy -D warnings, cargo deny, purity ok; web check 0 errors, 334 unit, 13 browser, npm audit 0.

## Rulings (see also 'Overnight rulings' in the queue plan)
- Merged reviewed green branches to local main without asking.
- rust-embed is optional (`embed`), default stays disk (symlink-swap deploys).
- Hero split-text: own Intl.Segmenter splitter instead of anime's splitText (saved a 10 kB chunk).
- "Add a module" had no picker; built a minimal list-only one (installs nothing).
- Reset layout lives in Settings, not Home (a button on Home would shift layout).
- Kept the Back/Forward router fix in History (copies router state, never-resolving loader on before-preparation); depends on Astro 7.4.2 internals.
- Cover transition uses View Transition, which animates group width/height (accepted, same as Library cards).

## Needs Toby
- ntfy topic/server (ntfy not installed on bawn); phone push is inactive (Remote Control not connected).
- Try on the laptop's real Chrome: drag-reorder (mouse/touch), cover transition, Add-a-module picker. Only headless Chrome 1248 was tested.
- Optional, need you nearby: login-allowlist spoof test, reboot test.
- Your files-module idea.

## Known gaps / follow-ups
- Re-run `web/scripts/f3c2-check.mjs pop cross rapid` after any Astro upgrade.
- Drag: no edge auto-scroll; older hostd rewriting prefs.json drops `layout` on rollback.
- Minor review leftovers: misleading comment in History.svelte (~83), "Session" placeholder heading while loading, 3 svelte-check warnings (GhostBar fileInput etc.), missing component tests for StatusDot cross-fade and the real base.css picker exit.
- CSP not set yet; openat2 RESOLVE_BENEATH hardening for static serving; LayerChart x-axis labels on real data; HEVC vs H.264 latency numbers; Library a11y leftovers; phone layout.
