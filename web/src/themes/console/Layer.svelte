<script lang="ts">
  // Bottom hint bar: button chips for keys that really exist. Display only, so it never takes clicks.
  let { path }: { path: string } = $props();
  const onStream = $derived(/^\/stream/.test(path));
  const HINTS: [string, string, string][] = [
    ['Enter', 'Open', 'a'],
    ['Esc', 'Back', 'b'],
    ['/', 'Search', 'y'],
    ['Ctrl K', 'Commands', 'x'],
    ['Alt U', 'Style', 'l'],
  ];
</script>

{#if !onStream}
  <div class="cs-hints" aria-hidden="true">
    {#each HINTS as [k, label, tone] (label)}
      <span class="cs-hint"><i class="cs-chip {tone}">{k}</i>{label}</span>
    {/each}
  </div>
{/if}

<style>
  .cs-hints { position: fixed; left: 0; right: 0; bottom: 0; z-index: 30; display: flex; justify-content: center; gap: 28px; padding: 12px 24px 14px; pointer-events: none; background: linear-gradient(to top, #070a22ee 30%, transparent); font-family: 'Fredoka Variable', system-ui, sans-serif; font-weight: 500; font-size: 15px; color: #d3d8ff; }
  .cs-hint { display: inline-flex; align-items: center; gap: 9px; }
  .cs-chip { font-style: normal; min-width: 30px; height: 30px; padding: 0 10px; border-radius: 999px; display: grid; place-items: center; font-size: 12px; font-weight: 600; color: #0b1030; background: #e8ebff; box-shadow: 0 2px 0 #7f88c8, 0 0 14px #8f9bff55; }
  .cs-chip.a { background: #7ef0c4; box-shadow: 0 2px 0 #3a9c7a, 0 0 14px #7ef0c455; }
  .cs-chip.b { background: #ff8fa8; box-shadow: 0 2px 0 #b04560, 0 0 14px #ff8fa855; }
  .cs-chip.y { background: #ffe27a; box-shadow: 0 2px 0 #b09a2c, 0 0 14px #ffe27a55; }
  .cs-chip.x { background: #8fb8ff; box-shadow: 0 2px 0 #4a6fb8, 0 0 14px #8fb8ff55; }
  .cs-chip.l { background: #d3a8ff; box-shadow: 0 2px 0 #7f55b8, 0 0 14px #d3a8ff55; }
  @media (max-width: 700px) { .cs-hints { display: none; } }
  @media (prefers-reduced-motion: no-preference) { .cs-hints { animation: cs-up 0.7s var(--spring-settle) backwards; } }
  @keyframes cs-up { from { translate: 0 100%; opacity: 0; } }
</style>
