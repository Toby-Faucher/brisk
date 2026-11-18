# Bento

A light, cool grey ground with white 28px tiles on a 10px gap. Home cards for Host, Resume, Recent sessions, Library and
Machines carry vivid colour (blue, violet, tangerine, mint). The nav is a floating pill dock at the bottom centre: icons only,
and the hovered, focused or current item grows a label. The host pill is the last item in the dock and its menu opens upward.
Settings tabs are a segmented pill. The Library hero, the game detail header and /stream keep dark tokens inside.

Motion: tiles pop in on the settle spring, hover lifts and tilts a little, the dock and the tab indicator move on the snappy
spring. All of it is off under prefers-reduced-motion (base.css).

Font: `@fontsource-variable/nunito` 5.3.0 (published 2026-07-19), imported at the top of theme.css.

## Needs shared change

Nothing. The default accent (mint) is mapped to a vivid blue for this style, so its swatch in Settings is teal and does not
match. The dock is not visible on /stream in the headless check (the stream page covers it); the rules for it are harmless.
