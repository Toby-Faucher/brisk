<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { navigate } from 'astro:transitions/client';
  import { Dialog } from 'bits-ui';
  import { api, type Game, type SessionRec } from '../../state/api';
  import { ensureReady, machineHue, machines, pings, setMachineColor, powerMachine, selected, selectMachine, startPings, wake } from '../../state/machines';
  import { startGame } from '../../state/prefs';
  import QuitLabel from '../../ui/QuitLabel.svelte';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';
  import { toast } from '../../shell/Toasts.svelte';
  import Dropdown from '../../ui/Dropdown.svelte';
  import Icon from '../../ui/Icon.svelte';
  import LatencyLayers from '../../ui/LatencyLayers.svelte';
  import StatusDot from '../../ui/StatusDot.svelte';
  import { box, flipDelta, glide, motionAllowed, cssVar } from '../../ui/motion-base';
  import Cover from '../library/Cover.svelte';
  import { fmtSize } from '../library/library';
  import { DASH, fmtAgo, fmtTemp, fmtUptime } from '../home/home';
  import { fmtWhen } from '../history/history';
  import { PALETTE, normalizeHex } from '../../ui/color';
  import { COLOR_NAMES, dotState, powerWhy, restartError, STATE_LABEL } from './machines';
  import { fmtPlayed, fmtPlaytime, gamesHeader, layoutFor, listGames, loadSort, saveSort, SORTS, stepIndex, type SortKey } from './modal';

  let { machineId, onclose, origin }: { machineId: string; onclose: () => void; origin?: DOMRect } = $props();

  const m = $derived($machines.find((x) => x.id === machineId) ?? null);
  const here = $derived($selected === machineId);
  const ready = $derived(m?.state === 'ready');
  const host = $derived(m?.host ?? null);
  const why = $derived(m ? powerWhy(m.state) : null);
  const waking = $derived($wake?.id === machineId && $wake.phase !== 'failed');

  let open = $state(true);
  let width = $state(typeof innerWidth === 'number' ? innerWidth : 1280);
  const layout = $derived(layoutFor(width));
  let tab = $state<'machine' | 'games' | 'details'>('machine'); // narrow: the one pane shown
  let shutting = $state(false);
  let panel: HTMLElement | null = null;
  let toOrigin = $state('');

  // ---- data: only ever asked of a machine that is ready ----
  type Load<T> = { state: 'idle' | 'loading' | 'error'; data: T | null; error: string };
  let gl = $state<Load<Game[]>>({ state: 'idle', data: null, error: '' });
  let sl = $state<Load<SessionRec[]>>({ state: 'idle', data: null, error: '' });
  let gseq = 0, sseq = 0;
  async function loadGames(refresh = false) {
    const n = ++gseq;
    gl = { ...gl, state: 'loading' };
    try {
      const r = await api.forMachine(machineId).games(refresh);
      if (n === gseq) gl = { state: 'idle', data: r.games.filter((g) => g.installed), error: '' };
    } catch (e) {
      if (n === gseq) gl = { ...gl, state: 'error', error: (e as Error).message };
    }
  }
  async function loadSessions() {
    const n = ++sseq;
    sl = { ...sl, state: 'loading' };
    try {
      const r = await api.forMachine(machineId).sessions();
      if (n === sseq) sl = { state: 'idle', data: r.sessions.slice(0, 8), error: '' };
    } catch (e) {
      if (n === sseq) sl = { ...sl, state: 'error', error: (e as Error).message };
    }
  }
  $effect(() => {
    if (ready) untrack(() => (void loadGames(), void loadSessions()));
    else (gseq++, sseq++); // an off machine gets nothing sent, and a late reply is dropped
  });
  // an off machine's lists are stale: show the Start prompt instead
  const gamesOk = $derived(ready && gl.data !== null);

  const SORT_OPTS = SORTS.map((s): [SortKey, string] => [s.key, s.label]);
  let q = $state('');
  let sort = $state<SortKey>(loadSort());
  let pick = $state<number | null>(null);
  const list = $derived(listGames(gl.data ?? [], q, sort));
  const game = $derived(pick === null ? null : ((gl.data ?? []).find((g) => g.appid === pick) ?? null));
  const recent = $derived(sl.data ?? []);
  const choose = (g: Game) => ((pick = g.appid), (tab = 'details'));
  const unpick = () => ((pick = null), (tab = 'games'));

  function onkeys(e: KeyboardEvent) {
    const rows = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('[data-row]')];
    const i = stepIndex(rows.indexOf(document.activeElement as HTMLElement), e.key, rows.length);
    if (i === null) return;
    e.preventDefault();
    rows[i].focus();
  }

  // ---- actions ----
  const failed = (what: string, e: unknown) => toast(`Could not ${what}: ${(e as Error).message}`, { error: true });
  const start = () => m && powerMachine(m.id, 'start').catch((e) => failed(`start ${m.name}`, e));
  async function shutDown() {
    shutting = false;
    if (m) await powerMachine(m.id, 'shutdown').catch((e) => failed(`shut down ${m.name}`, e));
  }
  let restarting = $state(false);
  async function restartSunshine() {
    restarting = true;
    try {
      await api.restartSunshine();
      toast('Sunshine is restarting');
    } catch (e) {
      toast(restartError(e), { error: true });
    } finally {
      restarting = false;
    }
  }
  let playing = $state(false);
  async function play(g: Game) {
    playing = true;
    try {
      if (!here) await selectMachine(machineId);
      else if (!ready) await ensureReady(machineId);
      await startGame(g.appid, toast); // readyFirst wakes a sleeping machine
      open = false;
      onclose();
      await navigate('/stream');
    } catch (e) {
      failed(`start ${g.name}`, e);
    } finally {
      playing = false;
    }
  }

  // ---- motion: FLIP from the clicked card, else a centred spring; reduced motion = none ----
  const swell = (el: HTMLElement) => {
    panel = el;
    let a: Animation | null = null;
    if (origin) a = glide(el, box({ getBoundingClientRect: () => origin } as Element), box(el));
    else if (motionAllowed()) a = el.animate([{ scale: '.94' }, { scale: '1' }], { duration: 400, easing: cssVar('--spring-snappy') || 'ease-out' });
    return () => (a?.cancel(), (panel = null));
  };
  function change(v: boolean) {
    if (!v && origin && panel) {
      const d = flipDelta(box({ getBoundingClientRect: () => origin } as Element), box(panel));
      toOrigin = d ? `--mp-t: ${d.dx}px ${d.dy}px; --mp-s: ${d.sx} ${d.sy}` : '';
    }
    open = v;
  }

  onMount(() => startPings());
  const samples = $derived($pings[machineId] ?? []);
  const cpu = $derived(host?.cpuTempC != null ? fmtTemp(host.cpuTempC) : m?.vm ? `${Math.round(m.vm.cpuPct)} % load` : null);
  const gb = (b: number) => `${(b / 2 ** 30).toFixed(1)} GB`;
  // ---- card colour: applies at once (the store is optimistic); a failed save puts the old colour back and says so here ----
  const hue = $derived($machineHue[machineId] ?? null);
  let colorErr = $state('');
  async function paint(c: string | null) {
    colorErr = '';
    try {
      await setMachineColor(machineId, c);
    } catch (e) {
      colorErr = `Could not save the colour: ${(e as Error).message}`;
    }
  }
  const SUN = { ready: 'Ready', busy: 'Streaming', down: 'Down' } as const;
</script>

<svelte:window bind:innerWidth={width} />

<Dialog.Root bind:open={() => open, change} onOpenChangeComplete={(v) => !v && onclose()}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content class="mp mm glass" style={toOrigin} {@attach swell} aria-describedby={undefined}>
      <header class="mm-h">
        <Dialog.Title class="mm-t">{#if hue}<i class="tsw" style:background={hue} aria-hidden="true"></i>{/if}{m?.name ?? 'Machine'}</Dialog.Title>
        <Dialog.Close class="btn ghost sm" aria-label="Close">Close</Dialog.Close>
      </header>

      {#if layout === 'narrow'}
        <div class="tabs" role="group" aria-label="Sections">
          {#each [['machine', 'Machine'], ['games', 'Games'], ['details', 'Details']] as const as [k, label] (k)}
            <button class="tab" aria-pressed={tab === k} onclick={() => (tab = k)}>{label}</button>
          {/each}
        </div>
      {/if}

      <div class="panes {layout}" class:over={game !== null}>
        <!-- pane 1 -->
        <section class="pane" hidden={layout === 'narrow' && tab !== 'machine'} aria-labelledby="mm-p1">
          <h3 id="mm-p1">Machine</h3>
          {#if !m}
            <p class="lab">This machine is no longer listed. Close this and pick another.</p>
          {:else}
            <p class="state"><StatusDot state={dotState({ state: m.state, sunshine: host?.sunshine ?? null })} /> <b>{STATE_LABEL[m.state]}</b>{#if here}<span class="sel">Selected</span>{/if}</p>
            <dl>
              {#if host}
                <dt>Sunshine</dt><dd>{SUN[host.sunshine]}</dd>
                <dt>Uptime</dt><dd>{fmtUptime(host.uptimeS)}</dd>
              {/if}
              {#if cpu}<dt>CPU</dt><dd>{cpu}</dd>{/if}
              {#if m.vm}<dt>RAM</dt><dd>{gb(m.vm.memUsedBytes)} / {gb(m.vm.memTotalBytes)}</dd>{/if}
              {#if host?.gpuTempC != null}<dt>GPU</dt><dd>{fmtTemp(host.gpuTempC)}</dd>{/if}
              {#if host}<dt>Network</dt><dd>{host.path === 'relay' ? `Relay${host.relay ? ` · ${host.relay}` : ''}` : host.path === 'direct' ? 'Direct' : DASH}</dd>{/if}
            </dl>
            <div class="pick" role="group" aria-label="Card colour">
              <span class="lab">Card colour</span>
              <div class="sws">
                {#each PALETTE as c, k (c)}
                  <button type="button" class="sw" style:background={c} aria-label={COLOR_NAMES[k]} aria-pressed={normalizeHex(hue ?? '') === normalizeHex(c)} onclick={() => void paint(c)}></button>
                {/each}
              </div>
              <label class="cust">Custom <input type="color" value={hue ?? PALETTE[0]} onchange={(e) => void paint(e.currentTarget.value)} /></label>
              <button type="button" class="btn ghost sm" onclick={() => void paint(null)}>Reset</button>
              {#if colorErr}<p class="lab err" role="status">{colorErr}</p>{/if}
            </div>
            {#if !host}<p class="lab">{m.state === 'off' ? `${m.name} is switched off. Start it when you want to play.` : m.state === 'starting' ? `${m.name} is waking up. This takes a minute or two.` : m.state === 'stopping' ? `${m.name} is shutting down.` : `Brisk cannot reach ${m.name} right now.`}</p>{/if}
            <h4 class="lab">Connection speed</h4>
            <LatencyLayers {samples} height={110} />
            <div class="actions">
              {#if !here}<button class="btn" onclick={() => void selectMachine(m.id)}>Use</button>{/if}
              {#if m.canPower && m.state === 'off'}<button class="btn" onclick={() => void start()}>Start</button>{/if}
              {#if m.canPower && m.state !== 'off'}
                <button class="btn ghost" aria-disabled={!!why || m.state !== 'ready'} aria-describedby={why ? 'mm-why' : undefined} onclick={() => m.state === 'ready' && (shutting = true)}>Shut down</button>
              {/if}
              {#if host && here}<button class="btn ghost" aria-disabled={restarting} onclick={() => !restarting && void restartSunshine()}>Restart Sunshine</button>{/if}
              {#if why}<span class="lab" id="mm-why">{why}</span>{/if}
            </div>
          {/if}
        </section>

        <!-- pane 2 -->
        <section class="pane" hidden={layout === 'narrow' && tab !== 'games'} aria-labelledby="mm-p2">
          <h3 id="mm-p2">Installed games</h3>
          {#if !m}
            <p class="lab">Nothing to show for a machine that is gone.</p>
          {:else if !ready}
            <div class="empty">
              <p class="lab">{m.state === 'off' && m.canPower ? `Start ${m.name} to see its games.` : m.state === 'starting' || waking ? `${m.name} is waking up. Its games appear when it is ready.` : `${m.name} is not reachable, so its games cannot be listed.`}</p>
              {#if m.state === 'off' && m.canPower}<button class="btn" onclick={() => void start()}>Start {m.name}</button>{/if}
            </div>
          {:else if gl.state === 'error' && !gl.data}
            <div class="empty"><p class="lab">Could not load the games: {gl.error}</p><button class="btn" onclick={() => void loadGames(true)}>Retry</button></div>
          {:else if !gamesOk}
            <div class="skel" role="status" aria-label="Loading games">{#each [0, 1, 2, 3, 4] as i (i)}<span></span>{/each}</div>
          {:else}
            <div class="tools">
              <label class="search"><Icon name="search" size={14} /><input type="search" placeholder="Search games" aria-label="Search installed games" bind:value={q} /></label>
              <Dropdown bind:value={sort} options={SORT_OPTS} label="Sort games" onchange={saveSort} />
            </div>
            <p class="lab" aria-live="polite">{gamesHeader(list)}</p>
            {#if gl.state === 'error'}<p class="lab">Could not refresh: {gl.error} <button class="link" onclick={() => void loadGames(true)}>Retry</button></p>{/if}
            {#if list.length}
              <ul class="games" role="listbox" aria-label="Installed games" tabindex="-1" onkeydown={onkeys}>
                {#each list as g (g.appid)}
                  <li role="presentation">
                    <button data-row class="row" role="option" aria-selected={pick === g.appid} onclick={() => choose(g)}>
                      <Cover game={g} style="width:34px;border-radius:7px" />
                      <span class="nm">{g.name}</span>
                      <span class="meta">{fmtSize(g.sizeBytes) ?? DASH} · {fmtPlayed(g.lastPlayed)}</span>
                      {#if g.running}<span class="tag run">Running</span>{/if}
                      {#if g.update}<span class="tag">Update</span>{/if}
                    </button>
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="lab">{q.trim() ? `No installed game matches “${q.trim()}”.` : 'No games are installed on this machine yet.'}</p>
            {/if}
          {/if}
        </section>

        <!-- pane 3 -->
        <section class="pane p3" hidden={layout === 'narrow' && tab !== 'details'} aria-labelledby="mm-p3">
          {#if game}
            <h3 id="mm-p3">Details</h3>
            {#if layout !== 'wide'}<button class="btn ghost sm back" onclick={unpick}>Back to games</button>{/if}
            <Cover game={game} style="width:100%;aspect-ratio:16/9;border-radius:12px" />
            <h4 class="gn">{game.name}</h4>
            <p class="tags">
              {#if game.running}<span class="tag run">Running</span>{/if}
              {#if game.update}<span class="tag">Update {game.update === 'running' ? 'in progress' : 'waiting'}</span>{/if}
              {#if game.controller}<span class="tag">{game.controller === 'full' ? 'Full controller support' : 'Partial controller support'}</span>{/if}
              {#if game.coop}<span class="tag">Co-op</span>{/if}
            </p>
            <dl>
              <dt>Size</dt><dd>{fmtSize(game.sizeBytes) ?? DASH}</dd>
              <dt>Playtime</dt><dd>{fmtPlaytime(game.playtimeMin)}</dd>
              <dt>Last played</dt><dd>{fmtPlayed(game.lastPlayed)}</dd>
              {#if game.achTotal}<dt>Achievements</dt><dd>{game.achUnlocked ?? 0} of {game.achTotal}</dd>{/if}
            </dl>
            <div class="actions">
              <button class="btn" aria-disabled={playing} onclick={() => !playing && void play(game)}>{playing ? 'Starting…' : here ? 'Play here' : `Play on ${m?.name ?? 'this machine'}`}</button>
              {#if game.running && here}
                {@const qd = $quitting.has(game.appid)}
                <button class="btn ghost" aria-disabled={qd} aria-busy={qd} onclick={() => !qd && askQuit(game)}><QuitLabel busy={qd} label="Quit" /></button>
              {/if}
            </div>
          {:else}
            <h3 id="mm-p3">Recent sessions on this machine</h3>
            {#if !ready}
              <p class="lab">Sessions show up here once the machine is on.</p>
            {:else if sl.state === 'error' && !sl.data}
              <p class="lab">Could not load sessions: {sl.error} <button class="link" onclick={() => void loadSessions()}>Retry</button></p>
            {:else if !sl.data}
              <div class="skel" role="status" aria-label="Loading sessions">{#each [0, 1, 2] as i (i)}<span></span>{/each}</div>
            {:else if recent.length}
              <ul class="sess">
                {#each recent as s (s.started)}
                  <li><b>{s.name}</b><span class="lab">{fmtWhen(s.started)} · {fmtAgo(s.started)}</span><span class="lab">{s.preset}{s.codec ? ` · ${s.codec}` : ''}</span></li>
                {/each}
              </ul>
            {:else}
              <p class="lab">No streams from this machine yet. Pick a game and press Play.</p>
            {/if}
            <p class="lab hint">Pick a game to see its details.</p>
          {/if}
        </section>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={() => shutting, (v) => (shutting = v)}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content class="rs glass">
      <Dialog.Title class="rs-t">Shut down {m?.name}?</Dialog.Title>
      <Dialog.Description class="lab">Running games will close and a live stream will end. Unsaved progress may be lost.</Dialog.Description>
      <div class="rs-a">
        <Dialog.Close class="btn ghost">Cancel</Dialog.Close>
        <button class="btn" onclick={() => void shutDown()}>Shut down</button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.mm.mp) { position: fixed; left: 0; right: 0; top: 6vh; margin-inline: auto; width: min(1180px, calc(100vw - 32px)); max-height: 88vh; padding: 22px 26px; z-index: 41; display: flex; flex-direction: column; gap: 12px; overflow: hidden; }
  .mm-h { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  :global(.mm-t) { font-size: 18px; font-weight: 700; letter-spacing: -.02em; }
  :global(.mm-t) { display: inline-flex; align-items: center; gap: 8px; }
  .tsw { width: 12px; height: 12px; border-radius: 50%; box-shadow: 0 0 0 1px color-mix(in srgb, #fff 30%, transparent); }
  .pick { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 10px; margin-top: 4px; }
  .sws { display: flex; gap: 6px; }
  .sw { width: 24px; height: 24px; padding: 0; border: 0; border-radius: 50%; cursor: pointer; box-shadow: 0 0 0 1px color-mix(in srgb, #fff 25%, transparent) inset; }
  .sw[aria-pressed='true'] { outline: 2px solid var(--fg); outline-offset: 2px; }
  .sw:focus-visible, .cust:focus-within { outline: 2px solid var(--accent); outline-offset: 2px; }
  .cust { display: inline-flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--muted); border-radius: 6px; }
  .cust input { width: 28px; height: 24px; padding: 0; border: 0; background: none; cursor: pointer; }
  .err { flex-basis: 100%; margin: 0; color: var(--bad); }
  .sm { padding: 4px 10px; font-size: 13px; }
  .tabs { display: flex; gap: 6px; }
  .tab { flex: 1; padding: 7px; border: 0; border-radius: 9px; background: var(--panel2); color: var(--fg); font: inherit; cursor: pointer; }
  .tab[aria-pressed='true'] { background: var(--accent); color: var(--accent-fg); }
  .panes { display: grid; gap: 14px; margin-top: 2px; min-height: 0; flex: 1; position: relative; overflow: hidden; }
  .panes.wide { grid-template-columns: 1fr 1.3fr 1fr; }
  .panes.medium { grid-template-columns: 1fr 1.2fr; }
  .panes.narrow { grid-template-columns: 1fr; }
  .panes.wide .pane + .pane, .panes.medium .pane + .pane { border-left: 1px solid var(--line); padding-left: 14px; }
  .hint { margin-top: 8px; opacity: .7; }
  .pane { overflow-y: auto; min-height: 0; padding-right: 4px; display: grid; gap: 8px; align-content: start; }
  .pane[hidden] { display: none; }
  h3 { font-size: 13px; font-weight: 600; color: var(--muted); margin: 0; }
  h4 { margin: 8px 0 0; font-size: 12.5px; font-weight: 500; }
  .gn { font-size: 17px; font-weight: 700; color: var(--fg); margin: 4px 0 0; overflow-wrap: anywhere; }
  .state { display: flex; align-items: center; gap: 8px; margin: 0; font-size: 20px; }
  .sel { margin-left: auto; font-size: 12px; font-weight: 600; color: var(--accent); }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 8px 18px; margin: 0; }
  dt { color: var(--muted); font-size: 13px; }
  dd { margin: 0; font-family: var(--mono); font-variant-numeric: tabular-nums; font-size: 13px; text-align: right; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 10px; }
  .empty { display: grid; gap: 10px; justify-items: start; }
  .tools { display: flex; gap: 8px; align-items: center; }
  .search { flex: 1; display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-radius: 10px; background: var(--panel); border: 1px solid var(--line); color: var(--muted); }
  .search input { background: transparent; border: 0; outline: 0; color: var(--fg); font: inherit; width: 100%; min-width: 0; }
  .games, .sess { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  .row { display: grid; grid-template-columns: 34px 1fr auto auto auto; align-items: center; gap: 10px; width: 100%; padding: 5px 8px; min-height: 44px; border: 0; border-radius: 10px; background: transparent; color: var(--fg); font: inherit; text-align: left; cursor: pointer; }
  .row:hover, .row[aria-selected='true'] { background: var(--panel2); }
  .row[aria-selected='true'] { box-shadow: 0 0 0 1px var(--accent) inset; }
  .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { color: var(--muted); font-size: 12px; white-space: nowrap; }
  .run { color: var(--accent); }
  .tags { display: flex; gap: 6px; flex-wrap: wrap; margin: 0; }
  .sess li { display: grid; gap: 2px; padding: 6px 0; border-bottom: 1px solid var(--line); }
  .link { background: none; border: 0; padding: 0; color: var(--accent); font: inherit; cursor: pointer; text-decoration: underline; }
  .skel { display: grid; gap: 8px; }
  .skel span { height: 44px; border-radius: 10px; background: var(--panel2); opacity: .6; }
  /* medium: the details pane slides over the games pane; narrow shows one pane at a time, wide has all three */
  .panes.medium .p3 { position: absolute; top: 0; bottom: 0; left: calc(var(--col1, 0px)); right: 0; grid-column: 2; background: color-mix(in srgb, #12151b 96%, transparent); translate: 105% 0; visibility: hidden; }
  .panes.medium.over .p3 { translate: 0 0; visibility: visible; }
  .panes.medium .p3 { left: auto; width: calc((100% - 14px) * 1.2 / 2.2); }
  @media (prefers-reduced-motion: no-preference) {
    .panes.medium .p3 { transition: translate var(--spring-snappy-dur) var(--spring-snappy), visibility 0s linear var(--spring-snappy-dur); }
    .panes.medium.over .p3 { transition-delay: 0s; }
  }
  .panes.medium:not(.over) .p3 { display: grid; }
  .back { justify-self: start; }
  @media (max-width: 699px) { :global(.mm.mp) { top: 3vh; max-height: 94vh; padding: 16px; } .row { grid-template-columns: 34px 1fr auto; } .row .meta { display: none; } }
</style>
