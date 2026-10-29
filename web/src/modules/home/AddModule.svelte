<script lang="ts">
  import { Dialog } from 'bits-ui';
  import { loadModules } from '../../shell/registry';
  import Icon from '../../ui/Icon.svelte';
  import { box, flipDelta, glide } from '../../ui/motion-base';

  // The picker lists what is installed: a module is a folder with a module.ts, so "adding" one happens in the source tree.
  const mods = loadModules().filter((m) => m.id !== 'home');
  const kind = (m: (typeof mods)[number]) => (m.soon ? 'Coming soon' : m.widget ? 'Card on Home' : 'Page');

  let open = $state(false);
  let card = $state<HTMLElement | null>(null);
  let panel: HTMLElement | null = null;
  let toCard = $state(''); // where the exit shrinks to (base.css `.mp[data-ending-style]`), as custom properties
  let grow: Animation | null = null;

  // FLIP: the panel is laid out where it ends, then glides from the card's box to there (translate + scale, native).
  const swell = (el: HTMLElement) => {
    panel = el;
    if (card) grow = glide(el, box(card), box(el));
    return () => (grow?.cancel(), (panel = null));
  };
  // Closing: measured now (the window may have been resized), before Bits sets data-ending-style. A close mid-swell
  // cancels the swell, so the shrink starts from where the panel is laid out.
  const change = (v: boolean) => {
    if (!v) {
      grow?.cancel();
      const d = card && panel && flipDelta(box(card), box(panel));
      toCard = d ? `--mp-t: ${d.dx}px ${d.dy}px; --mp-s: ${d.sx} ${d.sy}` : '';
    }
    open = v;
  };
</script>

<Dialog.Root bind:open={() => open, change}>
  <Dialog.Trigger class="ghostcard reveal" bind:ref={card}>
    <Icon name="plus" size={20} />
    <span>Add a module</span>
  </Dialog.Trigger>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content class="mp glass" style={toCard} {@attach swell}>
      <Dialog.Title class="rs-t">Modules</Dialog.Title>
      <Dialog.Description class="lab">To add one, drop a folder with a module.ts into web/src/modules and rebuild.</Dialog.Description>
      <ul class="mp-l">
        {#each mods as m (m.id)}
          <li><Icon name={m.icon} color={m.color} size={16} /><span>{m.title}</span><span class="lab">{kind(m)}</span></li>
        {/each}
      </ul>
      <div class="rs-a"><Dialog.Close class="btn">Done</Dialog.Close></div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.ghostcard) { grid-column: span 2; border: 1px dashed var(--line); border-radius: var(--r-card); background: transparent; color: var(--muted); display: grid; place-content: center; justify-items: center; gap: 4px; cursor: pointer; transition: transform .25s var(--ease), border-color .25s, color .25s; }
  :global(.ghostcard:hover) { transform: translateY(-3px); border-color: var(--line-hover); color: var(--fg); }
  @media (prefers-reduced-motion: reduce) { :global(.ghostcard:hover) { transform: none; } }
  /* centred without `transform`, so the FLIP's translate + scale (origin top left) map the boxes exactly */
  :global(.mp) { position: fixed; left: 0; right: 0; top: 18%; margin-inline: auto; width: min(420px, calc(100vw - 32px)); padding: 20px; z-index: 41; display: grid; gap: 6px; transform-origin: 0 0; }
  :global(.mp-l) { list-style: none; margin: 10px 0 0; padding: 0; display: grid; gap: 8px; }
  :global(.mp-l li) { display: grid; grid-template-columns: 16px 1fr auto; align-items: center; gap: 10px; font-size: 14px; }
</style>
