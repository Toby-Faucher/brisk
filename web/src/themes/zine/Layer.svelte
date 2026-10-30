<script lang="ts">
  // Marker notes, staples and a corner sticker over the page. Static, no JS layout work. Never on /stream; never catches a click.
  let { path }: { path: string } = $props();
  const on = $derived(!/^\/stream/.test(path));
  const NOTES: [RegExp, string][] = [
    [/^\/library/, 'so many games!!'],
    [/^\/history/, 'receipts'],
    [/^\/machines/, 'the big boxes'],
    [/^\/settings/, 'knobs + dials'],
  ];
  const note = $derived(NOTES.find(([re]) => re.test(path))?.[1] ?? 'your stuff, glued down');
</script>

{#if on}
  <div class="layer" aria-hidden="true">
    <div class="note">
      <svg viewBox="0 0 60 70" width="52" height="60"><path d="M40 66C8 56 6 30 30 8M30 8 18 16M30 8l4 15" fill="none" stroke="#17140f" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
      <span>{note}</span>
    </div>
    <svg class="staple a" viewBox="0 0 40 14" width="38" height="13"><path d="M2 12V4q0-2 2-2h32q2 0 2 2v8" fill="none" stroke="#6b6a68" stroke-width="3" stroke-linecap="round" /></svg>
    <svg class="staple b" viewBox="0 0 40 14" width="38" height="13"><path d="M2 12V4q0-2 2-2h32q2 0 2 2v8" fill="none" stroke="#6b6a68" stroke-width="3" stroke-linecap="round" /></svg>
    <div class="stamp">Alt+U<br />new zine</div>
  </div>
{/if}

<style>
  .layer { position: fixed; inset: 0; z-index: 21; pointer-events: none; overflow: hidden; }
  .note { position: absolute; left: 22px; bottom: 96px; width: 170px; display: flex; align-items: flex-end; gap: 4px; rotate: -4deg; color: #17140f; font-family: 'Permanent Marker', cursive; font-size: 19px; line-height: 1.05; }
  .staple { position: absolute; }
  .a { left: 14px; top: 10px; rotate: -12deg; }
  .b { right: 14px; top: 8px; rotate: 8deg; }
  .stamp { position: absolute; left: 122px; bottom: 172px; rotate: 7deg; padding: 5px 9px 3px; border: 3px double #17140f; color: #17140f; background: #ffe93b; font: 17px/1 'Bebas Neue', Impact, sans-serif; letter-spacing: 0.08em; text-align: center; opacity: 0.92; }
  @media (max-width: 900px) { .note, .stamp { display: none; } }
  @media (max-height: 760px) { .note, .stamp { display: none; } }
</style>
