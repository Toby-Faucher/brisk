<script lang="ts">
  let { path }: { path: string } = $props();
  // Off on /stream: the picture there is the product, no overlay may touch it.
  const off = $derived(path === '/stream' || path.startsWith('/stream/'));
  const SAYINGS = [
    'RADICAL',
    'CTRL+K OPENS THE COMMAND MENU',
    'TUBULAR',
    'ALT+U FLIPS THE STYLE',
    '88 MPH',
    'NEON NIGHTS',
    'THE HOST PILL SWITCHES MACHINES',
    'GNARLY',
    'DRIVE FAST, STREAM FASTER',
  ];
  const PALM = [
    'M60 230 C 62 180, 70 130, 92 84',
    'M92 84 C 62 62, 30 70, 8 100',
    'M92 84 C 70 50, 40 40, 14 48',
    'M92 84 C 98 50, 122 30, 150 30',
    'M92 84 C 126 62, 152 76, 172 106',
    'M92 84 C 88 58, 96 40, 112 20',
  ];
</script>

{#if !off}
  <div class="ox" aria-hidden="true">
    <div class="ox-stars"></div>
    <div class="ox-stars b"></div>
    <div class="ox-sun"></div>
    <div class="ox-floor">
      <div class="ox-plane"><div class="ox-lines"></div></div>
      <div class="ox-hz"></div>
    </div>
    {#each ['l', 'r'] as side (side)}
      <svg class="ox-palm {side}" viewBox="0 0 180 240" width="180" height="240" focusable="false">
        {#each PALM as d, i (i)}
          {#if i === 0}
            <path {d} class="trunk" />
          {:else}
            <path {d} class="frond" />
            <path {d} class="leaf" />
          {/if}
        {/each}
      </svg>
    {/each}
  </div>
  <div class="ox-strip" aria-hidden="true">
    <svg class="ox-car" viewBox="0 0 64 20" width="64" height="20" focusable="false">
      <path d="M2 14 L8 9 L20 7 L30 3 L44 3 L52 8 L62 10 L62 14 Z" class="body" />
      <path d="M31 4.5 L43 4.5 L49 8 L29 8 Z" class="glass" />
      <circle cx="15" cy="15" r="4" class="wheel" /><circle cx="49" cy="15" r="4" class="wheel" />
      <rect x="0" y="9" width="3" height="3" class="tail" />
    </svg>
    <div class="ox-ticker">
      {#each [0, 1] as copy (copy)}
        <span class="ox-run">{#each SAYINGS as s (s)}<b>{s}</b><i>&#9670;</i>{/each}</span>
      {/each}
    </div>
  </div>
{/if}
