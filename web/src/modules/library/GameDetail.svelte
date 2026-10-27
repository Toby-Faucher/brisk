<script lang="ts">
  // The detail panel a Library card (or the Hero's Details) opens. Wide screens: two panes, GameSide (art, actions,
  // quick facts, notes and tags) stays put and the right pane scrolls (description, screenshots, the host's screenshots
  // and saves, achievements, news, stream history). Under 760 px it is one scrolling column.
  import { Dialog } from 'bits-ui';
  import { api, type Game, type GameInfo, type Achievements, type News, type GameLocal } from '../../state/api';
  import { savesLine } from './library';
  import Icon from '../../ui/Icon.svelte';
  import GameSide from './GameSide.svelte';
  import StreamHistory from './StreamHistory.svelte';
  import Lightbox from './Lightbox.svelte';
  import AchievementsDialog from './AchievementsDialog.svelte';

  let { game, busy, onclose, onplay, oninstall }: { game: Game; busy: boolean; onclose: () => void; onplay: (g: Game) => void; oninstall: (g: Game) => void } = $props();

  // Mounted open; closing plays the exit, then onclose unmounts it. Focus goes back to what opened it (the card).
  let open = $state(true);
  const back = document.activeElement instanceof HTMLElement ? document.activeElement : null;

  // Store facts load when the panel opens (never for the grid); keyed by appid so a games refresh does not refetch.
  const appid = $derived(game.appid);
  let info = $state<GameInfo | null | 'none'>(null);
  $effect(() => {
    const id = appid;
    info = null;
    api.gameInfo(id).then(
      (v) => id === appid && (info = v.available ? v : 'none'),
      () => id === appid && (info = 'none'),
    );
  });

  // Achievements load alongside; a failure or total 0 just hides the section.
  let ach = $state<Achievements | null>(null);
  $effect(() => {
    const id = appid;
    ach = null;
    api.gameAchievements(id).then(
      (v) => id === appid && (ach = v),
      () => {},
    );
  });

  // Patch notes: same pattern; no items hides the section.
  let news = $state<News | null>(null);
  $effect(() => {
    const id = appid;
    news = null;
    api.gameNews(id).then(
      (v) => id === appid && (news = v),
      () => {},
    );
  });

  // Refresh: re-ask Steam (refresh=1) for info, achievements and news. Ten seconds between clicks so Steam is not hammered.
  let reloading = $state(false);
  let cool = $state(false);
  let coolTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => () => clearTimeout(coolTimer));
  async function refresh() {
    if (cool) return;
    cool = reloading = true;
    coolTimer = setTimeout(() => (cool = false), 10_000);
    const id = appid;
    const mine = <T,>(p: Promise<T>, set: (v: T) => void) => p.then((v) => id === appid && set(v), () => {}); // a failure keeps what is shown
    await Promise.all([
      mine(api.gameInfo(id, true), (v) => (info = v.available ? v : 'none')),
      mine(api.gameAchievements(id, true), (v) => (ach = v)),
      mine(api.gameNews(id, true), (v) => (news = v)),
    ]);
    reloading = false;
  }

  // Screenshots and saves from the host's Steam folder. Images need the auth headers, so each is fetched to an
  // object URL as its tile scrolls into view (at most 12 fetches at once); all URLs are revoked on unmount or game change.
  let local = $state<GameLocal | null>(null);
  let shotUrl = $state<Record<string, string>>({});
  let gen = 0;
  let made: string[] = [];
  let queue: string[] = [];
  let active = 0;
  const asked = new Set<string>();
  $effect(() => {
    const id = appid;
    const g = ++gen;
    local = null;
    api.gameLocal(id).then(
      (v) => g === gen && (local = v),
      () => {},
    );
    return () => {
      gen++;
      queue = [];
      asked.clear();
      made.forEach((u) => URL.revokeObjectURL(u));
      made = [];
      shotUrl = {};
    };
  });
  function pump() {
    while (active < 12 && queue.length) {
      const name = queue.shift()!;
      const g = gen;
      active++;
      api
        .screenshot(appid, name)
        .then(
          (u) => (g === gen ? (made.push(u), (shotUrl[name] = u)) : URL.revokeObjectURL(u)),
          () => {},
        )
        .finally(() => (active--, pump()));
    }
  }
  const want = (name: string) => {
    if (asked.has(name)) return;
    asked.add(name);
    queue.push(name);
    pump();
  };
  const lazy = (name: string) => (node: Element) => {
    const io = new IntersectionObserver(
      (es) => {
        if (!es.some((e) => e.isIntersecting)) return;
        io.disconnect();
        want(name);
      },
      { rootMargin: '200px' },
    );
    io.observe(node);
    return () => io.disconnect();
  };
  const saves = $derived(savesLine(local?.saves ?? null));


  // The lightbox: which list and where in it. Store shots use the full-size URL when the server sent one; the host's
  // own reuse their object URL (fetched now if the tile has not scrolled into view yet).
  let box = $state<{ kind: 'store' | 'local'; i: number } | null>(null);
  const storeShots = $derived.by(() => {
    const v = info && info !== 'none' ? info : null;
    return (v?.screenshots ?? []).map((src, i) => ({ src: v?.screenshotsFull?.[i] || src, alt: `${game.name} screenshot ${i + 1}` }));
  });
  const localShots = $derived((local?.screenshots ?? []).map((name, i) => ({ src: shotUrl[name] ?? '', alt: `${game.name} screenshot ${i + 1} from this PC` })));
  function show(kind: 'store' | 'local', i: number) {
    box = { kind, i };
    if (kind === 'local') local?.screenshots.forEach((n) => want(n));
  }
  let allAch = $state(false);
  const PREVIEW = 6;
</script>

<Dialog.Root bind:open onOpenChangeComplete={(o) => !o && onclose()}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content
      class="rs gd glass"
      onCloseAutoFocus={(e) => {
        if (back?.isConnected) {
          e.preventDefault();
          back.focus();
        }
      }}
    >
      <div class="gd-side"><GameSide {game} {busy} {info} {ach} {onplay} {oninstall} /></div>

      <div class="gd-main">
        <div class="rbar"><button class="btn ghost rf" aria-label="Refresh Steam data" aria-busy={reloading} aria-disabled={cool} title={cool ? 'Steam data was just refreshed. Wait 10 seconds to refresh again.' : 'Ask Steam for fresh store details, achievements and news'} onclick={refresh}><Icon name="replay" size={14} /> Refresh</button></div>
        <!-- store description and screenshots -->
        <section class="sec" aria-busy={info === null}>
          {#if info === null}
            <p class="lab">Loading store details…</p>
          {:else if info === 'none'}
            <p class="lab">No store details</p>
          {:else}
            {#if info.description}<p class="desc">{info.description}</p>{/if}
            {#if storeShots.length}
              <h3>Screenshots</h3>
              <div class="shots">
                {#each storeShots as s, i (s.src)}<button class="shot" aria-label="Open {s.alt}" aria-haspopup="dialog" onclick={() => show('store', i)}><img src={info.screenshots[i]} alt={s.alt} loading="lazy" decoding="async" /></button>{/each}
              </div>
            {/if}
          {/if}
        </section>

        <!-- the host's own screenshots and cloud-save summary -->
        {#if local && (local.screenshots.length || saves)}
          <section class="sec">
            {#if local.screenshots.length}
              <h3>Your screenshots</h3>
              <div class="shots">
                {#each local.screenshots as name, i (name)}
                  <button class="shot" aria-label="Open {localShots[i].alt}" aria-haspopup="dialog" onclick={() => show('local', i)}>
                    {#if shotUrl[name]}<img src={shotUrl[name]} alt={localShots[i].alt} decoding="async" />{:else}<span class="ph" {@attach lazy(name)}></span>{/if}
                  </button>
                {/each}
              </div>
            {/if}
            {#if saves}<p class="lab">{saves}</p>{/if}
          </section>
        {/if}

        <!-- achievements: the first few, the rest in their own modal -->
        {#if ach && ach.total > 0}
          <section class="sec ach">
            <div class="h"><h3>Achievements</h3>{#if ach.list.length}<button class="btn ghost more" aria-haspopup="dialog" aria-label="View all {ach.list.length} achievements" onclick={() => (allAch = true)}>View all</button>{/if}</div>
            <ul class="alist">
              {#each ach.list.slice(0, PREVIEW) as a (a.id)}
                <li class:locked={a.unlockedAt === null}>
                  {#if a.icon}<img src={a.icon} alt="" loading="lazy" decoding="async" />{:else}<span class="ph"></span>{/if}
                  <span><b>{a.name}</b>{#if a.description}<small>{a.description}</small>{/if}</span>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        <!-- patch notes: titles are text, only https URLs become links -->
        {#if news && news.items.length}
          <section class="sec">
            <h3>Latest news</h3>
            <ul class="news">
              {#each news.items as n, i (i)}
                <li>{#if n.url.startsWith('https://')}<a href={n.url} target="_blank" rel="noopener noreferrer">{n.title}</a>{:else}<span>{n.title}</span>{/if}{#if n.patch}<span class="pb">Patch</span>{/if}</li>
              {/each}
            </ul>
          </section>
        {/if}

        <StreamHistory {appid} />
      </div>

      {#if box}
        {@const list = box.kind === 'store' ? storeShots : localShots}
        {#if list.length}<Lightbox shots={list} bind:index={box.i} onclose={() => (box = null)} />{/if}
      {/if}
      {#if allAch && ach}<AchievementsDialog {ach} name={game.name} onclose={() => (allAch = false)} />{/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  /* .rs gives the dialog motion (base.css) and position; .gd makes it a wide two-pane panel: the side stays, the main
     pane scrolls. Under 760 px: one column, the whole panel scrolls. */
  :global(.rs.gd) { top: 6vh; width: min(1080px, calc(100vw - 32px)); height: min(88vh, 900px); padding: 0; gap: 0; display: grid; grid-template-columns: minmax(300px, 380px) 1fr; overflow: hidden; }
  .gd-side { overflow: auto; overscroll-behavior: contain; padding-bottom: 22px; border-right: 1px solid var(--line); }
  .gd-main { overflow: auto; overscroll-behavior: contain; padding-bottom: 22px; min-width: 0; }
  @media (max-width: 759px) {
    :global(.rs.gd) { display: block; height: auto; max-height: 88vh; overflow: auto; }
    .gd-side, .gd-main { overflow: visible; padding-bottom: 0; border: 0; }
    .gd-main { padding-bottom: 22px; }
  }
  .sec { padding: 14px 22px 0; display: grid; gap: 10px; }
  .rbar { display: flex; justify-content: flex-end; padding: 14px 22px 0; }
  .rf { padding: 4px 10px; font-size: 12.5px; min-height: 24px; display: inline-flex; align-items: center; gap: 6px; }
  .rf[aria-busy='true'] :global(svg) { animation: rf-spin 1s linear infinite; }
  @keyframes rf-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .rf[aria-busy='true'] :global(svg) { animation: none; } }
  .gd-main > .sec:first-child { padding-top: 8px; }
  .desc { line-height: 1.5; margin: 0; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
  .shots { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 8px; }
  .shot { padding: 0; border: 0; background: none; cursor: zoom-in; border-radius: 10px; display: block; }
  .shot img, .shot .ph { display: block; width: 100%; aspect-ratio: 16 / 9; object-fit: cover; border-radius: 10px; background: var(--panel); transition: filter .2s; }
  .shot:hover img { filter: brightness(1.12); }
  .h { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .more { padding: 4px 10px; font-size: 12.5px; min-height: 24px; }
  .alist { list-style: none; padding: 0; margin: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 8px; }
  .alist li { display: flex; gap: 10px; align-items: center; }
  .alist li.locked { opacity: .6; }
  .alist img, .alist .ph { width: 40px; height: 40px; flex: none; border-radius: 6px; background: var(--panel); }
  .alist span:not(.ph) { display: grid; min-width: 0; }
  .alist small { color: var(--muted); }
  .news { list-style: none; padding: 0; margin: 0; display: grid; gap: 8px; }
  .news li { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap; line-height: 1.4; }
  .news a:hover { text-decoration: underline; }
  .pb { flex: none; font-size: 11px; font-weight: 600; padding: 1px 8px; border-radius: 999px; color: var(--muted); border: 1px solid var(--line); background: var(--panel); }
</style>
