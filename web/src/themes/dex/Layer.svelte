<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));
  let lit = $state<string | null>(null);
  let off: ReturnType<typeof setTimeout>;

  // Up and down walk the entry list, left and right are Back and Forward, A opens the command menu, B is Escape.
  function press(id: string) {
    lit = id;
    clearTimeout(off);
    off = setTimeout(() => (lit = null), 160);
    if (id === 'a') window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', code: 'KeyK', ctrlKey: true, bubbles: true, cancelable: true }));
    else if (id === 'b') (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true, cancelable: true }));
    else if (id === 'left') history.back();
    else if (id === 'right') history.forward();
    else {
      const links = [...document.querySelectorAll<HTMLElement>('.side a.nav')];
      const at = links.indexOf(document.activeElement as HTMLElement);
      const cur = at >= 0 ? at : links.findIndex((l) => l.getAttribute('aria-current') === 'page');
      links[Math.max(0, Math.min(links.length - 1, cur + (id === 'down' ? 1 : -1)))]?.focus();
    }
  }
</script>

{#if !stream}
  <div class="dx-pad" role="group" aria-label="Handheld buttons" onmousedown={(e) => e.preventDefault()}>
    <div class="dx-dpad">
      {#each ['up', 'left', 'right', 'down'] as d}
        <button type="button" tabindex="-1" class={d} class:on={lit === d} aria-label="D-pad {d}" onclick={() => press(d)}></button>
      {/each}
    </div>
    <div class="dx-ab">
      <button type="button" tabindex="-1" class="b" class:on={lit === 'b'} aria-label="B button (Escape)" onclick={() => press('b')}>B</button>
      <button type="button" tabindex="-1" class="a" class:on={lit === 'a'} aria-label="A button (command menu)" onclick={() => press('a')}>A</button>
    </div>
  </div>
{/if}
