# Racer

An arcade street racer on sunny asphalt. Russo One italic type for headings, nav and buttons; plain system sans for body text.

- Sidebar is the pit lane: red and white rumble strip on its edge, nav items are numbered gears, the current page is a lit,
  slanted gear in the accent colour.
- `Layer.svelte` draws a fixed top dashboard (not on `/stream`): chequered flag, lap counter (counts page changes), lap clock,
  a short call-out line, start lights (red, yellow, green run on every page change) and a speed and tach gauge. Speed follows
  scrolling and gets a kick on each page change. It also draws slow speed-line streaks behind the page.
- Cards are position boards: a big position number behind the content, a gold, silver and bronze lane stripe, a cut corner.
  Page titles are yellow slanted plates. Buttons are slanted with a tyre-black underside. History durations read like lap times.
  Dialogs and menus get a rumble strip on top. The Library hero has a chequered band.
- The accent setting still colours the current gear and the primary buttons. The tach stays sun yellow.
- Reduced motion: no streaks, no rev loop, lights show at once, the clock ticks once a second.
- Narrow windows (under 800px): the pit lane shrinks to icons.

Known gaps: game cards in the Library have no position numbers (their markup has no safe spot for one). The Layer's bar
text is decorative and hidden from screen readers. axe is clean except `label-content-name-mismatch` on game cards, which
the original look also fails.
