<script lang="ts">
  // "Choose different art": the images SteamGridDB has for this game, as thumbnails (served by the machine, so the page
  // never talks to SteamGridDB). Tabs for Cover / Hero / Logo; click or Enter selects, "Use this art" (or Enter on the
  // selected one) applies. Arrow keys move through tabs and thumbnails. Esc closes only this dialog.
  import { Dialog } from 'bits-ui';
  import { api, ApiError, type ArtKindName, type ArtList, type Game } from '../../state/api';
  import { gameKey } from '../../state/gameId';
  import { artSrc } from '../../state/art';
  import { refreshGames } from '../../state/games';
  import { toast } from '../../shell/Toasts.svelte';

  let { game, onclose, onsearch }: { game: Game; onclose: () => void; onsearch: () => void } = $props();

  const KINDS: { id: ArtKindName; label: string }[] = [
    { id: 'cover', label: 'Cover' },
    { id: 'hero', label: 'Hero' },
    { id: 'logo', label: 'Logo' },
  ];
  type Slot = { state: 'loading' } | { state: 'error'; msg: string } | { state: 'ok'; list: ArtList };

  let open = $state(true);
  let kind = $state<ArtKindName>('cover');
  let slots = $state<Partial<Record<ArtKindName, Slot>>>({});
  let sel = $state<number | null>(null);
  let applying = $state(false);
  const back = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const key = $derived(gameKey(game));
  const slot = $derived(slots[kind]);
  const list = $derived(slot?.state === 'ok' ? slot.list : null);

  const WHY: Record<number, string> = {
    409: 'Art needs a SteamGridDB key on the machine.',
    404: 'Brisk has not matched this game on SteamGridDB yet. Use Search again.',
    503: 'SteamGridDB refused a request a moment ago. Try again in a while.',
  };
  function load(k: ArtKindName) {
    slots[k] = { state: 'loading' };
    api.artCandidates(key, k).then(
      (list) => (slots[k] = { state: 'ok', list }),
      (e) => (slots[k] = { state: 'error', msg: (e instanceof ApiError && WHY[e.status]) || 'Could not load the images.' }),
    );
  }
  $effect(() => {
    if (!slots[kind]) load(kind);
  });
  const pick = (k: ArtKindName) => ((kind = k), (sel = null));

  async function apply() {
    if (!list || sel === null || applying) return;
    applying = true;
    try {
      await api.artPick(key, kind, sel);
      toast(`Changed the ${kind} for ${game.name}`);
      void refreshGames({ force: true });
      open = false;
    } catch (e) {
      toast((e instanceof ApiError && WHY[e.status]) || 'Could not use that image.', { error: true });
    }
    applying = false;
  }

  // Arrows inside the tab row and the thumbnail grid move focus (roving tabindex); Enter on the selected thumbnail applies.
  function arrows(e: KeyboardEvent, group: string) {
    const items = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>(`[data-g="${group}"]`)];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    let to = step === undefined ? -1 : (at + step + items.length) % items.length;
    if (e.key === 'Home') to = 0;
    if (e.key === 'End') to = items.length - 1;
    if (to < 0 || !items.length) return;
    e.preventDefault();
    items[to].focus();
    if (group === 'tab') items[to].click();
  }
  const tile = (e: KeyboardEvent, id: number) => {
    if (e.key === 'Enter' && sel === id && id !== list?.current) (e.preventDefault(), void apply());
  };
</script>

<Dialog.Root bind:open onOpenChangeComplete={(o) => !o && onclose()}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim ap-dim" />
    <Dialog.Content
      class="rs glass ap"
      onCloseAutoFocus={(e) => {
        if (back?.isConnected) {
          e.preventDefault();
          back.focus();
        }
      }}
    >
      <Dialog.Title class="rs-t">Choose different art</Dialog.Title>
      <Dialog.Description class="lab">Images for {game.name} from SteamGridDB. The teal outline is what shows now.</Dialog.Description>
      <div class="tabs" role="tablist" aria-label="Kind of art" tabindex="-1" onkeydown={(e) => arrows(e, 'tab')}>
        {#each KINDS as k (k.id)}
          <button class="tab" role="tab" data-g="tab" aria-selected={kind === k.id} tabindex={kind === k.id ? 0 : -1} onclick={() => pick(k.id)}>{k.label}</button>
        {/each}
      </div>
      <div class="body" role="tabpanel" aria-label={kind}>
        {#if !slot || slot.state === 'loading'}
          <p class="lab msg" role="status">Loading images…</p>
        {:else if slot.state === 'error'}
          <p class="lab msg" role="alert">{slot.msg}</p>
          <button class="btn ghost" onclick={() => load(kind)}>Try again</button>
        {:else if !slot.list.items.length}
          <p class="lab msg" role="status">SteamGridDB has no {kind} images for this game.</p>
        {:else}
          <div class="grid {kind}" role="radiogroup" aria-label="{kind} images" tabindex="-1" onkeydown={(e) => arrows(e, 'img')}>
            {#each slot.list.items as it, i (it.id)}
              {@const on = sel === it.id}
              <button
                class="th"
                class:cur={it.id === slot.list.current}
                class:on
                role="radio"
                data-g="img"
                aria-checked={on}
                aria-label="{kind} {i + 1}{it.style ? `, ${it.style}` : ''}, {it.width} by {it.height}{it.id === slot.list.current ? ', in use' : ''}"
                tabindex={on || (sel === null && i === 0) ? 0 : -1}
                onclick={() => (sel = it.id)}
                onkeydown={(e) => tile(e, it.id)}
              >
                {#if it.thumb}<img {@attach artSrc(it.thumb)} alt="" loading="lazy" decoding="async" />{:else}<span class="lab">No preview</span>{/if}
                {#if it.id === slot.list.current}<b class="use">In use</b>{/if}
              </button>
            {/each}
          </div>
          {#if slot.list.total > slot.list.items.length}<p class="lab">Showing {slot.list.items.length} of {slot.list.total}.</p>{/if}
        {/if}
      </div>
      <div class="rs-a">
        <button class="btn ghost" onclick={() => ((open = false), onsearch())}>Search again</button>
        <Dialog.Close class="btn ghost">Cancel</Dialog.Close>
        <button class="btn" aria-disabled={sel === null || sel === list?.current || applying} aria-busy={applying} onclick={() => (sel === list?.current ? null : apply())}>{applying ? 'Saving…' : 'Use this art'}</button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.cm-dim.ap-dim) { z-index: 42; }
  :global(.rs.ap) { position: fixed; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(760px, calc(100vw - 32px)); max-height: calc(100dvh - 32px); z-index: 43; display: flex; flex-direction: column; gap: 10px; }
  .tabs { display: flex; gap: 6px; }
  .tab { font: inherit; font-size: 13px; padding: 5px 14px; min-height: 32px; border-radius: 999px; border: 1px solid var(--line); background: var(--panel2); color: var(--fg); cursor: pointer; }
  .tab[aria-selected='true'] { background: var(--accent); color: var(--accent-fg); border-color: var(--accent); font-weight: 600; }
  .body { overflow: auto; overscroll-behavior: contain; min-height: 180px; display: grid; gap: 10px; align-content: start; padding: 4px; }
  .msg { padding: 28px 0; text-align: center; margin: 0; }
  .grid { display: grid; gap: 10px; grid-template-columns: repeat(auto-fill, minmax(104px, 1fr)); }
  .grid.hero { grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); }
  .grid.logo { grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); }
  .th { position: relative; padding: 0; border: 2px solid transparent; border-radius: 10px; overflow: hidden; background: var(--panel); cursor: pointer; aspect-ratio: 2 / 3; display: grid; place-items: center; color: inherit; }
  .hero .th { aspect-ratio: 31 / 10; }
  .logo .th { aspect-ratio: 16 / 9; background: repeating-conic-gradient(var(--panel) 0 25%, var(--panel2) 0 50%) 0 0 / 16px 16px; }
  .th img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .logo .th img { object-fit: contain; padding: 8px; box-sizing: border-box; }
  .th.cur { border-color: var(--accent); }
  .th.on { border-color: var(--fg); box-shadow: 0 0 0 2px var(--bg) inset; }
  .th.on.cur { border-color: var(--accent); }
  .th:hover { filter: brightness(1.12); }
  .th:focus-visible { outline: 2px solid var(--fg); outline-offset: 2px; }
  .use { position: absolute; left: 6px; bottom: 6px; padding: 1px 8px; border-radius: 999px; font-size: 11px; background: var(--accent); color: var(--accent-fg); }
  .rs-a { margin-top: 4px; display: flex; gap: 8px; flex-wrap: wrap; }
  .rs-a > :first-child { margin-right: auto; }
  @media (prefers-reduced-motion: reduce) { .th { transition: none; } }
</style>
