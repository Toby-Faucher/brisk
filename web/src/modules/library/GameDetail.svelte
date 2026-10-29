<script lang="ts">
  // The detail panel a Library card (or the Hero's Details) opens. Wide screens: two panes, GameSide (art, actions,
  // quick facts, notes and tags) stays put and the right pane scrolls (description, screenshots, the host's screenshots
  // and saves, achievements, news, stream history). Under 760 px it is one scrolling column.
  import { Dialog } from 'bits-ui';
  import { api, type Game, type GameInfo, type Achievements, type RaAnswer, type News, type GameLocal, type GameSaves } from '../../state/api';
  import { savesLine, saveRow } from './library';
  import { gameCaps, gameKey } from '../../state/gameId';
  import { raView, TITLE_NOTE, startPoll, REFRESH_COOLDOWN_MS } from '../../state/ra';
  import { toast } from '../../shell/Toasts.svelte';
  import Icon from '../../ui/Icon.svelte';
  import GameSide from './GameSide.svelte';
  import StreamHistory from './StreamHistory.svelte';
  import Lightbox from './Lightbox.svelte';
  import AchievementsDialog from './AchievementsDialog.svelte';
  import ControlsSummary from './ControlsSummary.svelte';
  import GameLaunch from '../launch/GameLaunch.svelte';

  let { game, busy, onclose, onplay, oninstall }: { game: Game; busy: boolean; onclose: () => void; onplay: (g: Game) => void; oninstall: (g: Game) => void } = $props();

  // Mounted open; closing plays the exit, then onclose unmounts it. Focus goes back to what opened it (the card).
  let open = $state(true);
  const back = document.activeElement instanceof HTMLElement ? document.activeElement : null;

  // Store facts load when the panel opens (never for the grid); keyed by key so a games refresh does not refetch.
  const key = $derived(gameKey(game));
  const caps = $derived(gameCaps(game));
  let info = $state<GameInfo | null | 'none'>(null);
  $effect(() => {
    const id = key;
    info = null;
    api.gameInfo(id).then(
      (v) => id === key && (info = v.available ? v : 'none'),
      () => id === key && (info = 'none'),
    );
  });

  // Achievements load alongside; a failure or total 0 just hides the section. An emulator game (no Install) asks
  // RetroAchievements: the answer carries a `state`, shown as `ach` (ok) or `raNote` (anything else but off).
  const emu = $derived(!caps.install);
  let ach = $state<Achievements | null>(null);
  let raNote = $state<string | null>(null);
  const showRa = (v: Achievements | RaAnswer) => ((({ ach, note: raNote } = raView(v))), v);
  $effect(() => {
    const id = key;
    ach = null;
    raNote = null;
    if (caps.achievements)
      api.gameAchievements(id).then(
        (v) => id === key && showRa(v),
        () => {},
      );
  });
  // While an emulator game runs, re-read its progress every 45 s (read-only; stops with the panel or the game).
  $effect(() => {
    const id = key;
    if (!(caps.achievements && emu && game.running)) return;
    return startPoll(() => api.gameAchievements(id).then((v) => (id === key ? showRa(v) : v)));
  });
  // The RetroAchievements Refresh: 30 s between clicks, as the host allows.
  let raCool = $state(false);
  let raTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    key;
    raCool = false;
    return () => clearTimeout(raTimer);
  });
  function raRefresh() {
    if (raCool) return;
    raCool = true;
    raTimer = setTimeout(() => (raCool = false), REFRESH_COOLDOWN_MS);
    const id = key;
    api.gameAchievements(id, true).then(
      (v) => id === key && showRa(v),
      () => id === key && toast('Could not refresh RetroAchievements', { error: true }),
    );
  }

  // Patch notes: same pattern; no items hides the section.
  let news = $state<News | null>(null);
  $effect(() => {
    const id = key;
    news = null;
    api.gameNews(id).then(
      (v) => id === key && (news = v),
      () => {},
    );
  });

  // Refresh: re-ask Steam (refresh=1) for info, achievements and news. Ten seconds between clicks so Steam is not hammered.
  let reloading = $state(false);
  let cool = $state(false);
  let coolTimer: ReturnType<typeof setTimeout> | undefined;
  let rgen = 0; // bumped per game: a refresh of the previous game neither cools nor blocks this one
  $effect(() => {
    key;
    rgen++;
    clearTimeout(coolTimer);
    cool = reloading = false;
    return () => clearTimeout(coolTimer);
  });
  async function refresh() {
    if (cool || reloading || info === null) return; // not while the plain load runs: its slower answer could overwrite ours
    cool = reloading = true;
    coolTimer = setTimeout(() => (cool = false), 10_000);
    const id = key;
    const rg = rgen;
    let fails = 0;
    const mine = <T,>(p: Promise<T>, set: (v: T) => void) => p.then((v) => id === key && set(v), () => void fails++); // a failure keeps what is shown
    await Promise.all([
      mine(api.gameInfo(id, true), (v) => (info = v.available ? v : 'none')),
      ...(caps.achievements && !emu ? [mine(api.gameAchievements(id, true), (v) => (ach = v as Achievements))] : []),
      mine(api.gameNews(id, true), (v) => (news = v)),
    ]);
    if (rg === rgen) reloading = false;
    if (fails && id === key) toast('Could not refresh Steam data', { error: true });
  }

  // Screenshots and saves from the host's Steam folder. Images need the auth headers, so each is fetched to an
  // object URL as its tile scrolls into view (at most 12 fetches at once); all URLs are revoked on unmount or game change.
  let local = $state<GameLocal | null>(null);
  let shotUrl = $state<Record<string, string>>({});
  let shotBad = $state<Record<string, true>>({}); // failed fetches: the lightbox says so; the next want() tries again
  let gen = 0;
  let made: string[] = [];
  let queue: string[] = [];
  let active = 0;
  const asked = new Set<string>();
  $effect(() => {
    const id = key;
    const g = ++gen;
    local = null;
    if (caps.screenshots)
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
      shotBad = {};
    };
  });
  function pump() {
    while (active < 12 && queue.length) {
      const name = queue.shift()!;
      const g = gen;
      active++;
      api
        .screenshot(key, name)
        .then(
          (u) => (g === gen ? (made.push(u), (shotUrl[name] = u)) : URL.revokeObjectURL(u)),
          () => g === gen && (asked.delete(name), (shotBad[name] = true)),
        )
        .finally(() => (active--, pump()));
    }
  }
  const want = (name: string) => {
    if (asked.has(name)) return;
    asked.add(name);
    delete shotBad[name];
    queue.push(name);
    pump();
  };
  // Retry removes itself, so focus moves to the tile's own button first.
  const retryShot = (e: MouseEvent, name: string) => {
    (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>('.shot')?.focus();
    want(name);
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
  // Stored stream screenshots (emulator games) can be deleted, after a confirm on the tile.
  let delAsk = $state<string | null>(null);
  async function deleteShot(name: string) {
    delAsk = null;
    try {
      await api.deleteShot(key, name);
      if (shotUrl[name]) URL.revokeObjectURL(shotUrl[name]);
      delete shotUrl[name];
      if (local) local = { ...local, screenshots: local.screenshots.filter((n) => n !== name) };
      toast('Screenshot deleted');
    } catch (e) {
      toast(`Couldn't delete the screenshot: ${(e as Error).message}`, { error: true });
    }
  }
  const saves = $derived(savesLine(local?.saves ?? null));

  // Emulator saves and save states (read-only). Only games with caps.saves ask; an old hostd never does.
  let emuSaves = $state<GameSaves | null>(null);
  $effect(() => {
    const id = key;
    let live = true;
    emuSaves = null;
    if (caps.saves)
      api.gameSaves(id).then(
        (v) => live && (emuSaves = v),
        () => {},
      );
    return () => (live = false);
  });


  // The lightbox: which list and where in it. Store shots use the full-size URL when the server sent one; the host's
  // own reuse their object URL; the shown one and its two neighbours are fetched now if their tiles have not scrolled
  // into view (never the whole list: up to 60 files of 25 MiB, held until the panel closes).
  let box = $state<{ kind: 'store' | 'local'; i: number } | null>(null);
  const storeShots = $derived.by(() => {
    const v = info && info !== 'none' ? info : null;
    return (v?.screenshots ?? []).map((src, i) => ({ src: v?.screenshotsFull?.[i] || src, alt: `${game.name} screenshot ${i + 1}` }));
  });
  const localShots = $derived((local?.screenshots ?? []).map((name, i) => ({ src: shotUrl[name] ?? '', failed: !!shotBad[name], alt: `${game.name} screenshot ${i + 1} from this PC` })));
  const show = (kind: 'store' | 'local', i: number) => (box = { kind, i });
  $effect(() => {
    if (box?.kind !== 'local' || !local?.screenshots.length) return;
    const l = local.screenshots;
    for (const d of [-1, 0, 1]) want(l[(box.i + d + l.length) % l.length]);
  });
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
      <Dialog.Close class="gd-x" aria-label="Close"><Icon name="x" size={16} /></Dialog.Close>
      <div class="gd-side"><GameSide {game} {busy} {info} {ach} {onplay} {oninstall} onremoved={() => (open = false)} /></div>

      <div class="gd-main">
        <div class="rbar"><button class="btn ghost rf" aria-label="Refresh Steam data" aria-busy={reloading} aria-disabled={cool || reloading || info === null} aria-describedby={cool ? 'rf-why' : undefined} title={cool ? 'Steam data was just refreshed. Wait 10 seconds to refresh again.' : 'Ask Steam for fresh store details, achievements and news'} onclick={refresh}><Icon name="replay" size={14} /> Refresh</button>{#if cool}<span id="rf-why" class="vh">Steam data was just refreshed. Wait 10 seconds to refresh again.</span>{/if}</div>
        <!-- emulator games: a note and a link to the mappings in Settings; hides itself when hostd has no Dolphin -->
        {#if caps.controls}{#key key}<ControlsSummary />{/key}{/if}
        <!-- Steam and emulator games: wrappers, environment and arguments, per field default or override -->
        {#if caps.launchOptions}{#key key}<GameLaunch {game} />{/key}{/if}
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
                {#each storeShots as s, i (i)}<button class="shot" aria-label="Open {s.alt}" aria-haspopup="dialog" onclick={() => show('store', i)}><img src={info.screenshots[i]} alt={s.alt} loading="lazy" decoding="async" /></button>{/each}
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
                  <div class="tile">
                    <button class="shot" aria-label="Open {localShots[i].alt}" aria-haspopup="dialog" onclick={() => show('local', i)}>
                      {#if shotUrl[name]}<img src={shotUrl[name]} alt={localShots[i].alt} decoding="async" />{:else}<span class="ph" {@attach lazy(name)}></span>{/if}
                    </button>
                    {#if emu}
                      {#if delAsk === name}
                        <div class="del" role="group" aria-label="Delete screenshot {i + 1}?">
                          <button class="btn ghost" onclick={() => deleteShot(name)}>Delete</button>
                          <button class="btn ghost" onclick={() => (delAsk = null)}>Keep</button>
                        </div>
                      {:else}
                        <button class="btn ghost delbtn" aria-label="Delete screenshot {i + 1}" onclick={() => (delAsk = name)}>Delete</button>
                      {/if}
                    {/if}
                    {#if shotBad[name]}<button class="btn ghost retry" aria-label="Retry loading screenshot {i + 1}" onclick={(e) => retryShot(e, name)}>Retry</button>{/if}
                  </div>
                {/each}
              </div>
            {/if}
            {#if saves}<p class="lab">{saves}</p>{/if}
          </section>
        {/if}

        <!-- emulator saves and save states, read-only -->
        {#if caps.saves && emuSaves}
          <section class="sec">
            <h3>Saves</h3>
            {#if emuSaves.items.length}
              <ul class="alist saves" aria-label="Saves">
                {#each emuSaves.items as it (it.path)}
                  {@const r = saveRow(it)}
                  <li>
                    <Icon name={r.icon} size={20} />
                    <span><b>{r.title}</b><small>{r.detail}</small>{#if r.note}<small>{r.note}</small>{/if}</span>
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="lab">Save states and saves show up here after you play.</p>
            {/if}
          </section>
        {/if}

        <!-- achievements: the first few, the rest in their own modal -->
        {#snippet raBtn()}<button class="btn ghost rf" aria-disabled={raCool} title={raCool ? 'Just refreshed. Wait 30 seconds to refresh again.' : 'Ask RetroAchievements for fresh progress'} onclick={raRefresh}><Icon name="replay" size={14} /> Refresh</button>{/snippet}
        {#if emu && raNote}
          <section class="sec ach"><div class="h"><h3>Achievements</h3>{@render raBtn()}</div><p class="lab" role="status">{raNote}</p></section>
        {/if}
        {#if ach && ach.total > 0}
          <section class="sec ach">
            <div class="h"><h3>Achievements</h3>{#if ach.list.length > PREVIEW}<button class="btn ghost more" aria-haspopup="dialog" aria-label="View all {ach.list.length} achievements" onclick={() => (allAch = true)}>View all</button>{/if}</div>
            {#if ach.source === 'retroachievements'}
              <p class="lab ra">{ach.unlocked} of {ach.total} unlocked, {ach.pointsUnlocked ?? 0} of {ach.pointsTotal ?? 0} points · <a href={ach.url} target="_blank" rel="noopener noreferrer">RetroAchievements</a> {@render raBtn()}</p>
              {#if ach.matched === 'title'}<p class="lab" role="note">{TITLE_NOTE}</p>{/if}
            {/if}
            <ul class="alist">
              {#each ach.list.slice(0, PREVIEW) as a (a.id)}
                <li class:locked={a.unlockedAt === null}>
                  {#if a.icon}<img src={a.icon} alt="" loading="lazy" decoding="async" />{:else}<span class="ph"></span>{/if}
                  <span><b>{a.name}</b>{#if a.hardcore}<small class="hc">Hardcore</small>{/if}{#if a.description}<small>{a.description}</small>{/if}</span>
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

        <StreamHistory {game} />
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
    /* one scrolling column: the X stays in view at the top while the panel scrolls */
    :global(.gd-x) { position: sticky; top: 8px; margin: 8px 14px -44px auto; }
  }
  .sec { padding: 14px 22px 0; display: grid; gap: 10px; }
  .rbar { display: flex; justify-content: flex-end; padding: 14px 82px 0 22px; }
  /* the close X sits top-right, 30px in so it clears the right column's scrollbar (~15px); .rbar leaves room for it. Red on hover and focus. */
  :global(.gd-x) { position: absolute; top: 10px; right: 30px; z-index: 3; width: 36px; height: 36px; display: grid; place-items: center; padding: 0; border: 0; border-radius: 999px; cursor: pointer; color: var(--muted); background: color-mix(in srgb, var(--panel2) 70%, transparent); transition: color .15s, background .15s; }
  :global(.gd-x:hover), :global(.gd-x:focus-visible) { color: var(--bad); background: color-mix(in srgb, var(--bad) 16%, var(--panel2)); }
  @media (pointer: coarse) { :global(.gd-x) { width: 44px; height: 44px; top: 6px; right: 30px; } .rbar { padding-right: 90px; } }
  .rf { padding: 4px 10px; font-size: 12.5px; min-height: 24px; display: inline-flex; align-items: center; gap: 6px; }
  .rf[aria-busy='true'] :global(svg) { animation: rf-spin 1s linear infinite; }
  @keyframes rf-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .rf[aria-busy='true'] :global(svg) { animation: none; } }
  .vh { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .rbar + .sec { padding-top: 8px; }
  .desc { line-height: 1.5; margin: 0; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
  .shots { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 8px; }
  .tile { position: relative; }
  .delbtn, .del { position: absolute; inset: 8px 8px auto auto; padding: 4px 8px; min-height: 24px; background: var(--panel2); display: flex; gap: 6px; }
  .del { padding: 4px; }
  .retry { position: absolute; inset: auto 8px 8px auto; padding: 4px 10px; font-size: 12.5px; min-height: 24px; background: var(--panel2); }
  .shot { width: 100%; padding: 0; border: 0; background: none; cursor: zoom-in; border-radius: 10px; display: block; }
  .shot img, .shot .ph { display: block; width: 100%; aspect-ratio: 16 / 9; object-fit: cover; border-radius: 10px; background: var(--panel); transition: filter .2s; }
  .shot:hover img { filter: brightness(1.12); }
  .h { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .more { padding: 4px 10px; font-size: 12.5px; min-height: 24px; }
  .alist { list-style: none; padding: 0; margin: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 8px; }
  .alist li { display: flex; gap: 10px; align-items: center; }
  .alist.saves { grid-template-columns: 1fr; }
  .alist li.locked { opacity: .6; }
  .alist img, .alist .ph { width: 40px; height: 40px; flex: none; border-radius: 6px; background: var(--panel); }
  .alist span:not(.ph) { display: grid; min-width: 0; }
  .alist small { color: var(--muted); }
  .news { list-style: none; padding: 0; margin: 0; display: grid; gap: 8px; }
  .news li { display: flex; align-items: baseline; gap: 8px; flex-wrap: wrap; line-height: 1.4; }
  .news a:hover { text-decoration: underline; }
  .hc { font-size: 11px; font-weight: 600; color: var(--accent, var(--fg)); }
  .ra { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .pb { flex: none; font-size: 11px; font-weight: 600; padding: 1px 8px; border-radius: 999px; color: var(--muted); border: 1px solid var(--line); background: var(--panel); }
</style>
