# Command first

A top strip replaces the sidebar: wordmark, page links (icons, the current page keeps its label), a centred command field
(`Layer.svelte`, opens the Command menu with a synthesized Ctrl+K), and the host pill at the right. Pages are single-column
lists with kbd chips. The one gradient (violet to pink) is on the primary button, the field and tab underline, and the focus
rings. Everything else is grey with 1px borders and 6px corners. Motion is 90ms.

## Fonts

- `@fontsource-variable/inter` 5.3.0 (text)
- `@fontsource-variable/jetbrains-mono` 5.3.0 (kbd chips, numbers, tooltips, toasts)

## Behaviour notes

- Accent colour setting is ignored here on purpose (the style is monochrome); the Accent card stays in Settings for the other styles.
- "Coming soon" modules are hidden in the strip (they are not controls). Home, Library and the rest are all reachable.
- Library shelves keep their horizontal scroller (the virtual list needs it); "Show all" turns into a one-row-per-game list.
- axe: `label-content-name-mismatch` on Library game cards and the controller pad buttons comes from existing markup
  (the card's aria-label does not include its badge text), not from this style.

## Needs shared change

Nothing required. Optional: `title` attributes on the Sidebar links would give the icon-only links a mouse tooltip.
