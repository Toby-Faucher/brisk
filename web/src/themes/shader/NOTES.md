# Shader

A full-screen WebGL fragment shader behind dark frosted panels. Files: `theme.css` (tokens, glass) and `Layer.svelte`.

- **Shader**: raw WebGL, one program, four presets (lava plasma, metaball pond, warp tunnel, moire grid), each with its own
  cosine palette. The preset changes on every page change (with a brief brightness flash). A corner label names it.
- **Pointer**: eased into a uniform; each preset bends around it. A click sends a ripple (three at once, ring buffer).
- **Cost**: canvas at 0.5 scale, 30 fps cap, loop stops when the tab is hidden, low-power GPU hint.
- **Fallbacks**: CSS gradient behind the canvas when WebGL is missing or the program fails; under reduced motion one frame
  is drawn (and again on resize, on a page change the loop is off so `redraw` paints the new preset).
- **Not mounted on /stream.** Layer is `pointer-events: none`.
- **Contrast**: panels are 90% opaque near-black, and the shader output is scaled to at most 40% brightness (peaks only during a page-change flash or a ripple), so even bare
  headings and muted text on it keep AA contrast.
  axe cannot read pixels under a canvas, so this is by construction plus the axe pass on the opaque tokens.
- **Fonts**: `@fontsource/space-grotesk` and `@fontsource-variable/geist-mono` (already pinned).

## Known gaps

- Layout is the original one; only skin, glass and the live background change.
- Accent only tints titles, hover borders and focus rings; shader palettes are fixed.
- One shared fragment shader with a mode branch, so low-end GPUs pay for the branch uniform (cheap in practice).
