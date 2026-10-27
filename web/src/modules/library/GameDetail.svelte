<script lang="ts">
  // The detail panel a Library card (or the Hero's Details) opens. Sections top to bottom: header art, actions,
  // store facts; later sections (achievements, patch notes, local screenshots, saves) slot in below the store ones.
  import { Dialog } from 'bits-ui';
  import { api, type Game, type GameInfo, type Achievements, type News } from '../../state/api';
  import Icon from '../../ui/Icon.svelte';
  import PlayIcon from '../../ui/PlayIcon.svelte';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';
  import { hue } from './Cover.svelte';
  import { badgeLabel, installLabel, playLabel } from './library';

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
  .acts { display: flex; gap: 8px; flex-wrap: wrap; padding: 14px 22px 0; }
  .acts :global(.close) { margin-left: auto; }
  .sec { padding: 14px 22px 22px; display: grid; gap: 10px; }
  .desc { line-height: 1.5; }
  .ctl { display: flex; gap: 6px; align-items: center; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; list-style: none; padding: 0; margin: 0; }
  .chips li { padding: 3px 10px; border-radius: 999px; font-size: 12.5px; border: 1px solid var(--line); background: var(--panel); }
  .chips li.mode { border-color: color-mix(in srgb, var(--accent) 50%, transparent); }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
  .shots { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 8px; }
  .shots img { width: 100%; aspect-ratio: 16 / 9; object-fit: cover; border-radius: 10px; background: var(--panel); }
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
