# Overworld (top-down adventure)

The sidebar becomes an item-select row under a HUD strip. Pages sit on a pixel meadow (Home, Library) or dungeon stone
(everything else). Cards are dark item boxes with a gold frame, menus and dialogs are parchment.

## How it works

- `Layer.svelte` draws the HUD (hearts, a gem counter, a dotted room map, an old sage with a hint per screen) and sets
  the ground zone through `.ow-zone[data-z]`. All of it is `aria-hidden` and `pointer-events: none`. Not mounted on `/stream`.
- Every click anywhere adds a gem (kept in `sessionStorage` for the tab). Every 25th, the sage comments.
- `.side` is `position: fixed` under the HUD. Nav links are 88px item boxes, the active one gets a blinking dashed
  cursor and a bobbing arrow, "soon" items are dashed and dim. The host pill moves to the right, its menu opens downward.
- Machines: the grid is a dotted dungeon map, the connection line is a dotted hallway between two square rooms.
- Press Start 2P for headings, nav, buttons and big numbers. Body text stays in the app font for reading.
- `.glass` (dialogs, menus, command box, toasts) switches its colour tokens to parchment, so children adapt. The stream
  bar stays dark.

## Accent

Kept. Primary buttons, the item cursor and the selected-chip ring use `--accent`. Gold and wood are fixed.

## Motion

Stepped: blinking cursor, bobbing arrow, heart throb, typed sage line, gem hop. All behind `prefers-reduced-motion: no-preference`.

## Known gaps

- Hearts and the room map are decorative. Hearts do not reflect host health.
- Dungeon stone is a repeating tile, so it can look busy behind sparse pages.
- Below about 760px the item row scrolls sideways and the HUD drops the map and sage.
- `label-content-name-mismatch` on game cards fails in every style, including the original.
