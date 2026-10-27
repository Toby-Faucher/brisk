<script lang="ts" module>
  // 16x16 glyphs, one path each (evenodd for holes), filled with currentColor. Names: TAG_SYMBOLS in state/notes.ts.
  export const GLYPHS: Record<string, string> = {
    star: 'M8 1.2l2 4.3 4.6.5-3.4 3.2 1 4.6L8 11.5l-4.2 2.3 1-4.6L1.4 6l4.6-.5z',
    heart: 'M8 14.2S1.5 10.3 1.5 5.9A3.4 3.4 0 0 1 8 4.3a3.4 3.4 0 0 1 6.5 1.6c0 4.4-6.5 8.3-6.5 8.3z',
    flag: 'M2.8 1.5h1.6v13H2.8zM4.4 2h9l-2.2 3.2 2.2 3.3h-9z',
    check: 'M6.2 12.2 2 8l1.4-1.4 2.8 2.8 6.4-6.4L14 4.4z',
    clock: 'M8 1a7 7 0 1 0 0 14A7 7 0 0 0 8 1zm0 1.7a5.3 5.3 0 1 1 0 10.6A5.3 5.3 0 0 1 8 2.7zM7.2 4h1.6v3.6l2.6 1.5-.8 1.4-3.4-2z',
    fire: 'M8 .8s5 3.8 5 8.6a5 5 0 0 1-10 0c0-2.3 1.2-3.8 1.2-3.8s.4 2.4 2 2.4C6.2 4.7 8 .8 8 .8z',
    bookmark: 'M3.5 1.5h9v13L8 11.3l-4.5 3.2z',
    trophy: 'M4 1.5h8v3.7a4 4 0 0 1-3.2 3.9V12H11v2.5H5V12h2.2V9.1A4 4 0 0 1 4 5.2zM12.6 2.5H15v1.8a3 3 0 0 1-2.6 3l.2-1.6a1.4 1.4 0 0 0 .8-1.3V4h-.8zM3.4 2.5H1v1.8a3 3 0 0 0 2.6 3l-.2-1.6a1.4 1.4 0 0 1-.8-1.3V4h.8z',
    skull: 'M8 1.2A5.8 5.8 0 0 0 2.2 7c0 1.9.9 3.3 2.1 4.1v2.7h7.4v-2.7c1.2-.8 2.1-2.2 2.1-4.1A5.8 5.8 0 0 0 8 1.2zM5.7 6a1.3 1.3 0 1 1 0 2.6 1.3 1.3 0 0 1 0-2.6zm4.6 0a1.3 1.3 0 1 1 0 2.6 1.3 1.3 0 0 1 0-2.6z',
    gamepad: 'M4.6 3.8h6.8a3.6 3.6 0 0 1 3.6 3.4l.3 3.8a1.9 1.9 0 0 1-3.3 1.4l-1.7-1.9H5.7L4 12.4A1.9 1.9 0 0 1 .7 11L1 7.2a3.6 3.6 0 0 1 3.6-3.4zM4 5.8V7H2.8v1.4H4v1.2h1.4V8.4h1.2V7H5.4V5.8zm7.2.2a.9.9 0 1 0 0 1.8.9.9 0 0 0 0-1.8zm1.5 1.7a.9.9 0 1 0 0 1.8.9.9 0 0 0 0-1.8z',
    users: 'M6 2.2a2.6 2.6 0 1 1 0 5.2 2.6 2.6 0 0 1 0-5.2zM.8 14c0-3 2.3-4.8 5.2-4.8s5.2 1.8 5.2 4.8zM11.3 2.8a2.3 2.3 0 1 1 0 4.6 2.3 2.3 0 0 1 0-4.6zM12.2 9.3c1.9.4 3 2 3 4.7h-2.6c0-1.9-.5-3.4-1.6-4.5z',
    bolt: 'M9.6.8 2.8 9h4.3l-1 6.2L13.2 7H8.9z',
    moon: 'M10.2 1.2a6.8 6.8 0 1 0 4.6 10.3A5.8 5.8 0 0 1 10.2 1.2z',
    gift: 'M1.5 5h13v3.2h-13zM2.5 8.2h11v6.3h-11zM7.2 5h1.6v9.5H7.2zM8 5C6.2 1.6 3.4 2.2 4.4 4.3 4.9 5 8 5 8 5zm0 0c1.8-3.4 4.6-2.8 3.6-.7C11.1 5 8 5 8 5z',
    question: 'M8 1a7 7 0 1 0 0 14A7 7 0 0 0 8 1zm-.1 2.8c1.7 0 2.9 1 2.9 2.4 0 1-.6 1.7-1.5 2.2-.4.3-.5.5-.5 1v.4H7.2v-.6c0-.9.4-1.5 1.1-2 .5-.3.8-.6.8-1 0-.5-.5-.9-1.2-.9-.7 0-1.2.4-1.3 1.1H5c.1-1.6 1.2-2.6 2.9-2.6zM8 10.7a1 1 0 1 1 0 2 1 1 0 0 1 0-2z',
  };
</script>

<script lang="ts">
  // A tag: its symbol in its color, then its name (unless `bare`). Purely visual: callers give the name to assistive tech.
  import { TAG_COLORS, type TagColor } from '../../state/notes';

  let { name = '', color, symbol, bare = false, size = 14 }: { name?: string; color: TagColor; symbol: string; bare?: boolean; size?: number } = $props();
  const d = $derived(GLYPHS[symbol]);
</script>

<span class="tc" class:bare style:--tc={TAG_COLORS[color] ?? TAG_COLORS.gray}>
  <svg width={size} height={size} viewBox="0 0 16 16" aria-hidden="true" fill="currentColor">
    {#if d}<path fill-rule="evenodd" {d} />{:else}<circle cx="8" cy="8" r="4" />{/if}
  </svg>
  {#if !bare}<span class="n">{name}</span>{/if}
</span>

<style>
  .tc { display: inline-flex; align-items: center; gap: 5px; padding: 2px 9px 2px 7px; border-radius: 999px; font-size: 12.5px; color: var(--fg); background: color-mix(in srgb, var(--tc) 14%, var(--panel)); border: 1px solid color-mix(in srgb, var(--tc) 55%, transparent); white-space: nowrap; max-width: 100%; }
  .tc svg { color: var(--tc); flex: none; }
  .n { overflow: hidden; text-overflow: ellipsis; }
  .tc.bare { padding: 0; border: 0; background: none; gap: 0; }
</style>
