<script lang="ts">
  // The detail panel a Library card (or the Hero's Details) opens. Sections top to bottom: header art, actions,
  // your notes and tags, store facts, the host's screenshots and saves, achievements, patch notes.
  import { Dialog } from 'bits-ui';
  import { api, type Game, type GameInfo, type Achievements, type News, type GameLocal } from '../../state/api';
  import Icon from '../../ui/Icon.svelte';
  import PlayIcon from '../../ui/PlayIcon.svelte';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';
  import { hue } from './Cover.svelte';
  import { badgeLabel, installLabel, playLabel, savesLine, smartTags } from './library';
  import GameNotes from './GameNotes.svelte';
  import TagChip from './TagChip.svelte';
  import StreamHistory from './StreamHistory.svelte';

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
  const lazy = (name: string) => (node: Element) => {
    const io = new IntersectionObserver(
      (es) => {
        if (!es.some((e) => e.isIntersecting) || asked.has(name)) return;
        io.disconnect();
        asked.add(name);
        queue.push(name);
        pump();
      },
      { rootMargin: '200px' },
    );
    io.observe(node);
    return () => io.disconnect();
  };
  const saves = $derived(savesLine(local?.saves ?? null));

  let heroBad = $state('');
  let logoLoaded = $state('');
  const logoOk = $derived(!!game.logo && logoLoaded === game.logo);
  const hrs = $derived(game.playtimeMin < 60 ? `${game.playtimeMin} min` : `${Math.round(game.playtimeMin / 60)} h`);
  const q = $derived($quitting.has(game.appid));
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
      <!-- header -->
      <div class="head" style="background:radial-gradient(120% 140% at 85% 0%, hsl({hue(game)} 62% 48% / .67), #12151c 62%)">
        {#if game.hero && heroBad !== game.hero}<img class="art" src={game.hero} alt="" onerror={() => (heroBad = game.hero)} />{/if}
        {#if game.logo}<img class="logo" class:on={logoOk} src={game.logo} alt="" onload={() => (logoLoaded = game.logo)} />{/if}
        <Dialog.Title class={logoOk ? 'gd-t vh' : 'gd-t'}>{game.name}</Dialog.Title>
        <Dialog.Description class="gd-d">{badgeLabel(game)}{game.playtimeMin ? ` · ${hrs} played` : ''}</Dialog.Description>
      </div>

      <!-- actions -->
      {#if game.installed && game.update}<p class="upd" role="note">Steam still has to update this game. Starting it now may wait for the download.</p>{/if}
      <div class="acts">
        {#if game.installed}
          <button class="btn" aria-disabled={busy} onclick={() => onplay(game)}><PlayIcon {busy} size={14} /> {busy ? 'Starting…' : playLabel(game)}</button>
        {:else}
          <button class="btn" aria-disabled={busy} onclick={() => oninstall(game)}><Icon name="dl" size={14} /> {busy ? 'Asking Steam…' : installLabel(game)}</button>
        {/if}
        {#if game.running}
          <button class="btn ghost" aria-disabled={q} onclick={() => askQuit(game)} aria-label={q ? `Quitting ${game.name}` : `Quit ${game.name}`}><Icon name="power" size={14} /> {q ? 'Quitting…' : 'Quit'}</button>
        {/if}
        <Dialog.Close class="btn ghost close">Close</Dialog.Close>
      </div>

      <StreamHistory {appid} />

      <!-- your note and tags (keyed: a fresh draft per game) -->
      {#key game.appid}<GameNotes appid={game.appid} name={game.name} />{/key}
      {#if smartTags(game).length}
        <section class="sec" aria-label="Automatic tags">
          <h3>Automatic tags</h3>
          <div class="auto">{#each smartTags(game) as t (t.id)}<TagChip name={t.name} color={t.color} symbol={t.symbol} />{/each}</div>
        </section>
      {/if}

      <!-- store facts -->
      <section class="sec" aria-busy={info === null}>
        {#if info === null}
          <p class="lab">Loading store details…</p>
        {:else if info === 'none'}
          <p class="lab">No store details</p>
        {:else}
          {#if info.description}<p class="desc">{info.description}</p>{/if}
          {#if info.controller}<p class="ctl"><Icon name="pad" size={15} /> {info.controller === 'full' ? 'Full' : 'Partial'} controller support</p>{/if}
          <ul class="chips" aria-label="Genres and modes">
            {#each info.genres as g (g)}<li>{g}</li>{/each}
            {#if info.coop}<li class="mode">Co-op</li>{/if}
            {#if info.remotePlayTogether}<li class="mode">Remote Play Together</li>{/if}
          </ul>
          {#if info.release || info.metacritic}<p class="lab">{[info.release && `Released ${info.release}`, info.metacritic && `Metacritic ${info.metacritic}`].filter(Boolean).join(' · ')}</p>{/if}
          {#if info.screenshots.length}
            <h3>Screenshots</h3>
            <div class="shots">
              {#each info.screenshots as src, i (src)}<img {src} alt="{game.name} screenshot {i + 1}" loading="lazy" decoding="async" />{/each}
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
                {#if shotUrl[name]}<img src={shotUrl[name]} alt="{game.name} screenshot {i + 1} from this PC" decoding="async" />{:else}<span class="ph" {@attach lazy(name)}></span>{/if}
              {/each}
            </div>
          {/if}
          {#if saves}<p class="lab">{saves}</p>{/if}
        </section>
      {/if}

      <!-- achievements -->
      {#if ach && ach.total > 0}
        <section class="sec ach">
          <h3>Achievements</h3>
          <p class="lab">{ach.unlocked} of {ach.total} unlocked</p>
          <div class="bar" role="progressbar" aria-label="{ach.unlocked} of {ach.total} achievements" aria-valuemin={0} aria-valuemax={ach.total} aria-valuenow={ach.unlocked}><i style="width:{(ach.unlocked / ach.total) * 100}%"></i></div>
          <ul class="alist">
            {#each ach.list as a (a.id)}
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
              <li>{#if n.url.startsWith('https://')}<a href={n.url} target="_blank" rel="noopener noreferrer">{n.title}</a>{:else}{n.title}{/if}{#if n.patch} <small>patch</small>{/if}</li>
            {/each}
          </ul>
        </section>
      {/if}
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  /* .rs gives the dialog motion (base.css) and position; .gd makes it a wide, scrolling panel */
  :global(.rs.gd) { top: 6vh; width: min(720px, calc(100vw - 32px)); max-height: 88vh; overflow: auto; padding: 0; gap: 0; }
  .head { position: relative; overflow: hidden; min-height: 170px; padding: 22px; display: flex; flex-direction: column; justify-content: flex-end; gap: 4px; }
  .head > :global(*:not(.art)) { position: relative; }
  .art { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; object-position: center top; opacity: .55; mask-image: linear-gradient(0deg, transparent, #000 70%); pointer-events: none; }
  .logo { display: none; max-width: 260px; max-height: 84px; object-fit: contain; object-position: left; }
  .logo.on { display: block; }
  :global(.gd-t) { margin: 0; font-size: 24px; letter-spacing: -.03em; }
  :global(.gd-t.vh) { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  :global(.gd-d) { color: #c9d1e0; font-size: 13.5px; }
  .upd { margin: 14px 22px 0; font-size: 13px; color: var(--muted); }
  .acts { display: flex; gap: 8px; flex-wrap: wrap; padding: 14px 22px 0; }
  .acts :global(.close) { margin-left: auto; }
  .sec { padding: 14px 22px 22px; display: grid; gap: 10px; }
  .desc { line-height: 1.5; }
  .ctl { display: flex; gap: 6px; align-items: center; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; list-style: none; padding: 0; margin: 0; }
  .chips li { padding: 3px 10px; border-radius: 999px; font-size: 12.5px; border: 1px solid var(--line); background: var(--panel); }
  .chips li.mode { border-color: color-mix(in srgb, var(--accent) 50%, transparent); }
.auto { display: flex; flex-wrap: wrap; gap: 6px; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
  .shots { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 8px; }
  .shots img, .shots .ph { width: 100%; aspect-ratio: 16 / 9; object-fit: cover; border-radius: 10px; background: var(--panel); }
  .sec.ach { padding-top: 0; }
  .bar { height: 6px; border-radius: 3px; background: var(--line); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); }
  .alist { list-style: none; padding: 0; margin: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 8px; max-height: 320px; overflow: auto; }
  .alist li { display: flex; gap: 10px; align-items: center; }
  .alist li.locked { opacity: .6; }
  .alist img, .alist .ph { width: 40px; height: 40px; flex: none; border-radius: 6px; background: var(--panel); }
  .news { list-style: none; padding: 0; margin: 0; display: grid; gap: 6px; }
  .news small { color: var(--muted); }
  .alist span:not(.ph) { display: grid; min-width: 0; }
  .alist small { color: var(--muted); }
</style>
