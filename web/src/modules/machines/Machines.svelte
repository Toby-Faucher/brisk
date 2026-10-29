<script lang="ts">
  import { flushSync, onMount, tick } from 'svelte';
  import { enter } from '../../shell/reveal';
  import { Dialog } from 'bits-ui';
  import { api, type HostInfo, type StreamInfo } from '../../state/api';
  import type { Machine } from '../../state/machines';
  import { games, watchGames } from '../../state/games';
  import { findGame, gameKey, rowKey } from '../../state/gameId';
  import QuitLabel from '../../ui/QuitLabel.svelte';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';
  import { host as hostStore, refreshHost } from '../../state/host';
  import { toast, toastError } from '../../shell/Toasts.svelte';
  import Card from '../../ui/Card.svelte';
  import HeaderSpark from '../../ui/HeaderSpark.svelte';
  import Ring from '../../ui/Ring.svelte';
  import { defaultColor, headerBg, onColor } from '../../ui/color';
  import Icon from '../../ui/Icon.svelte';
  import StatusDot from '../../ui/StatusDot.svelte';
  import ConnPath from './ConnPath.svelte';
  import MachineModal from './MachineModal.svelte';
  import Slot from '../home/Slot.svelte';
  import { count, fadeOnChange } from '../../ui/motion';
  import { reconcileOrder } from '../home/home';
  import { DASH, fmtUptime } from '../../ui/format';
  import { reorder } from '../home/drag';
  import { hub, machineHue, orderedMachines, pings, startPings, selected, selectMachine, setMachinesOrder, powerMachine } from '../../state/machines';
  import { isTap, machinesFrom, restartError, dimmed, dotState, powerWhy, statRows, chipData, gauges, headerVars, networkText, pingText, STATE_LABEL, SUN_LABEL, type Vm } from './machines';

  let stream = $state<StreamInfo | null>(null);
  let confirming = $state(false);
  let busy = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  type Entry = { id: string; name: string; state: Machine['state']; canPower: boolean; info: HostInfo | null; vm: Vm | null };
  const hubOn = $derived($hub === true);
  // single-host mode keeps today's one card from the host store
  // while a card is being dragged (or just dropped) `local` is the order on screen; the store catches up on the drop
  let local: string[] | null = $state(null);
  const ordered = $derived.by(() => {
    if (!local) return $orderedMachines;
    const ids = reconcileOrder(local, $orderedMachines.map((m) => m.id));
    return ids.map((id) => $orderedMachines.find((m) => m.id === id)!);
  });
  const machines = $derived<Entry[]>(hubOn ? ordered.map((m) => ({ id: m.id, name: m.name, state: m.state, canPower: m.canPower, info: m.host, vm: m.vm })) : ($hostStore ? machinesFrom($hostStore) : []).map((m) => ({ id: m.id, name: m.info.name, state: 'ready', canPower: false, info: m.info, vm: null })));
  let shutting = $state<Entry | null>(null);
  const failed = toastError;
  const start = (m: Entry) => powerMachine(m.id, 'start').catch((e) => failed(`start ${m.name}`, e));
  async function shutDown(m: Entry) {
    shutting = null;
    await powerMachine(m.id, 'shutdown').catch((e) => failed(`shut down ${m.name}`, e));
  }
  const liveKey = $derived(stream ? rowKey(stream) : null);
  const app = $derived(liveKey === null ? null : (findGame($games?.games ?? [], liveKey)?.name ?? `App ${liveKey}`));

  let arrived = false; // the host card lands after hydration: entrance once
  $effect(() => {
    if (machines.length && !arrived) (arrived = true), void tick().then(enter);
  });

  const running = $derived(($games?.games ?? []).filter((g) => g.running));

  let modal = $state<{ id: string; origin: DOMRect } | null>(null);
  let opener: HTMLElement | null = null;
  let said = $state('');
  // The whole card opens the details: the one focusable `.fill` button per card (keyboard, screen readers) plus a click
  // anywhere else on the card. Real controls, the drag handle, a drag and a text selection never open it.
  function openModal(fill: HTMLElement) {
    opener = fill;
    modal = { id: fill.closest<HTMLElement>('[data-card]')!.dataset.card!, origin: fill.closest('.card')!.getBoundingClientRect() };
  }
  let down: { x: number; y: number } | null = null;
  function cardClick(e: MouseEvent) {
    const t = e.target as Element;
    const card = t.closest<HTMLElement>('[data-card]');
    const fill = card?.querySelector<HTMLElement>('.fill');
    if (!fill || t.closest('button, a, input, select, textarea, [role="dialog"], [data-grip]') || !isTap(down, { x: e.clientX, y: e.clientY })) return;
    const sel = getSelection();
    if (sel && !sel.isCollapsed && card!.contains(sel.anchorNode)) return; // selecting text, not opening
    openModal(fill);
  }
  const closeModal = () => (modal = null, void tick().then(() => opener?.focus()));

  const drag = reorder({
    order: () => machines.map((m) => m.id),
    set: (next) => {
      local = next;
      flushSync();
    },
    drop: (id, next, changed) => {
      said = `${machines.find((m) => m.id === id)?.name} card dropped, position ${next.indexOf(id) + 1} of ${next.length}`;
      if (!changed) return void (local = null);
      setMachinesOrder(next).catch(() => toast('Could not save the machine order', { error: true })).finally(() => (local = null));
    },
  });

  onMount(() => {
    const unwatch = watchGames();
    const stopPings = startPings();
    const load = () => api.stream().then((s) => (stream = s), () => (stream = null));
    void load();
    const id = setInterval(() => !document.hidden && void load(), 10_000);
    return () => (clearInterval(id), clearTimeout(timer), unwatch(), stopPings()); // HostPill keeps $host fresh
  });

  async function restart() {
    busy = true;
    try {
      await api.restartSunshine();
      toast('Sunshine is restarting');
      confirming = false;
      timer = setTimeout(refreshHost, 3000);
    } catch (e) {
      toast(restartError(e), { error: true });
      confirming = false;
    } finally {
      busy = false;
    }
  }
</script>

<div class="top reveal"><h2>Machines</h2></div>
{#if hubOn}
  <span id="grip-hint" hidden>Space picks the card up, arrow keys move it, Space or Enter drops it, Escape puts it back.</span>
  <!-- one polite line per drop, as on Home -->
  <span class="sr" role="status">{said}</span>
{/if}
<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (the keyboard path is each card's .fill button) -->
<div class="grid" class:hub={hubOn} onpointerdown={(e) => (down = { x: e.clientX, y: e.clientY })} onclick={cardClick} {@attach hubOn ? drag : undefined}>
  {#each machines as m (m.id)}
    {#if hubOn}
      <Slot id={m.id} label={m.name}>{@render card(m)}</Slot>
    {:else}
      {@render card(m)}
    {/if}
  {:else}
    <Card cols={2}>
      <h4><Icon name="host" color="var(--c-host)" size={16} /> Machines</h4>
      <div class="big">{DASH}</div>
      <div class="lab">Machine status is not available here. Brisk could not reach the host agent.</div>
    </Card>
  {/each}
</div>
{#if modal}<MachineModal machineId={modal.id} origin={modal.origin} onclose={closeModal} />{/if}

{#snippet actions(m: Entry)}
  {@const i = m.info}
  {@const here = !hubOn || m.id === $selected}
  {@const why = powerWhy(m.state)}
  <div class="actions" class:pin={hubOn}>
    {#if hubOn && !here}<button class="btn" onclick={() => void selectMachine(m.id)}>Use</button>{/if}
    {#if hubOn && m.canPower && m.state === 'off'}<button class="btn" onclick={() => void start(m)}>Start</button>{/if}
    {#if hubOn && m.canPower && m.state !== 'off'}
      <button class="btn ghost" aria-disabled={!!why || m.state !== 'ready'} aria-describedby={why ? `why-${m.id}` : undefined} onclick={() => m.state === 'ready' && (shutting = m)}>Shut down</button>
    {/if}
    {#if why}<span class="lab" id="why-{m.id}">{why}</span>{/if}
    {#if i && here}
      <Dialog.Root bind:open={confirming}>
        <Dialog.Trigger class="btn ghost">Restart Sunshine</Dialog.Trigger>
        <Dialog.Portal>
          <Dialog.Overlay class="cm-dim" />
          <Dialog.Content class="rs glass">
            <Dialog.Title class="rs-t">Restart Sunshine?</Dialog.Title>
            <Dialog.Description class="lab">A live stream will drop.</Dialog.Description>
            <div class="rs-a">
              <button class="btn ghost" disabled={busy} onclick={() => (confirming = false)}>Cancel</button>
              <button class="btn" disabled={busy} onclick={restart}>Restart</button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    {/if}
    {#if !hubOn}<button class="btn ghost" disabled title="Not in v1">Wake-on-LAN · later</button>{/if}
  </div>
{/snippet}

{#snippet runningApps(here: boolean)}
  {#if here && running.length}
    <h5 class="lab">Running apps</h5>
    <ul class="apps">
      {#each running as g (gameKey(g))}
        {@const q = $quitting.has(gameKey(g))}
        <li><span>{g.name}</span><button class="btn ghost sm" aria-disabled={q} aria-busy={q} onclick={() => askQuit(g)} aria-label={q ? `Quitting ${g.name}` : `Quit ${g.name}`}><QuitLabel busy={q} label="Quit" /></button></li>
      {/each}
    </ul>
  {/if}
{/snippet}

{#snippet card(m: Entry)}
    {@const i = m.info}
    {@const here = !hubOn || m.id === $selected}
    {@const state = i ? dotState({ state: m.state, sunshine: i.sunshine }) : dotState({ state: m.state, sunshine: null })}
    {@const hue = $machineHue[m.id] ?? defaultColor(0)}
    {@const big = i ? (i.sunshine === 'down' ? 'Sunshine is down' : 'Online') : STATE_LABEL[m.state]}
    <Card cols={2} rows={2} style={hubOn ? headerVars(hue, onColor(hue), !!i, headerBg(hue)) : ''} class={(hubOn ? 'hc ' : '') + (hubOn && here ? 'on' : dimmed(hubOn, machines.length, $selected, m.id) ? 'dim' : '')}>
      <!-- the card's one tab stop: visually hidden, the card shows its focus ring; mouse clicks are handled on the grid -->
      {#if hubOn}<button type="button" class="fill" aria-label="Open {m.name} details" onclick={(e) => openModal(e.currentTarget)}></button>{/if}
      {#if hubOn}
        {@const g = i ? gauges(i, m.vm) : []}
        {@const p = pingText($pings[m.id] ?? [])}
        <!-- the header is the machine's colour; a machine that is not up keeps it muted, with no rings or ping -->
        <div class="hd" class:idle={!i}>
          <h4><Icon name="host" color="var(--on)" size={16} /> <span class="nm">{m.name}</span> <StatusDot state={state} />{#if here}<span class="sel">Selected</span>{/if}</h4>
          <div class="word" {@attach fadeOnChange(big)}>{big}</div>
          {#if i}
            <div class="hflex">
              <div class="hrings">
                {#each g as r (r.key)}
                  <div class="hr"><Ring pct={r.pct} text={r.text} level={r.level} /><span class="sr">{r.label} {r.text}</span><span class="k" aria-hidden="true">{r.label}</span></div>
                {/each}
              </div>
              <div class="lat" role="img" aria-label="Ping {p.direct}, hub {p.hub}"><em>{p.direct}</em>hub {p.hub}</div>
            </div>
            <div class="hsp"><HeaderSpark samples={$pings[m.id] ?? []} /></div>
          {/if}
        </div>
        <div class="bd">
          {#if i}
            <div class="chips">
              {#each chipData(i, here ? (app ?? (stream?.state === 'live' ? 'Streaming' : stream ? 'Nothing' : DASH)) : DASH) as c (c.label)}
                <span>{c.label}<b {@attach fadeOnChange(c.value)}>{c.value}</b></span>
              {/each}
            </div>
            {@render runningApps(here)}
          {:else}
            <p class="lab calm">{m.state === 'off' ? `${m.name} is switched off. Start it when you want to play.` : m.state === 'starting' ? `${m.name} is waking up. This takes a minute or two.` : m.state === 'stopping' ? `${m.name} is shutting down.` : `Brisk cannot reach ${m.name} right now.`}</p>
          {/if}
        </div>
      {:else}
      <h4><Icon name="host" color="var(--c-host)" size={16} /> {m.name} <StatusDot {state} /></h4>
      {#if i}
      {@const up = fmtUptime(i.uptimeS)}
      <div class="big" {@attach fadeOnChange(big)}>{big}</div>
      <ConnPath path={i.path} />
      <dl>
        <dt>Sunshine</dt><dd {@attach fadeOnChange(SUN_LABEL[i.sunshine])}>{SUN_LABEL[i.sunshine]}</dd>
        <dt>Uptime</dt><dd {@attach fadeOnChange(up)}>{up}</dd>
        {#each statRows(i, m.vm, hubOn) as r (r.label)}
          <dt>{r.label}</dt><dd {@attach r.n == null ? undefined : count(r.n, r.fmt)}>{r.text}</dd>
        {/each}
        <dt>Network</dt><dd>{networkText(i)}</dd>
        <dt>Running</dt><dd>{app ?? (stream?.state === 'live' ? 'Streaming' : stream ? 'Nothing' : DASH)}</dd>
      </dl>
      {@render runningApps(here)}
      {/if}
      {/if}
      {@render actions(m)}
    </Card>
{/snippet}

<Dialog.Root bind:open={() => !!shutting, (v) => !v && (shutting = null)}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content class="rs glass">
      <Dialog.Title class="rs-t">Shut down {shutting?.name}?</Dialog.Title>
      <Dialog.Description class="lab">Running games will close and a live stream will end. Unsaved progress may be lost.</Dialog.Description>
      <div class="rs-a">
        <Dialog.Close class="btn ghost">Cancel</Dialog.Close>
        <button class="btn" onclick={() => shutting && void shutDown(shutting)}>Shut down</button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .top { padding: 0 2px 14px; border-bottom: 1px solid var(--line); }
  h2 { font-size: 18px; letter-spacing: -.02em; }
  /* position: the cards' offsets (drag.ts hit tests) are relative to the grid */
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 14px; padding-top: 16px; position: relative; }
  /* the whole hub card is the target: pointer, a little brighter on hover; the real controls keep their own cursor */
  .hub :global(.card) { cursor: pointer; }
  .hub :global(.card:hover) { background: color-mix(in srgb, var(--panel) 90%, #fff 4%); }
  .hub :global(.card:has(.fill:focus-visible)) { outline: 2px solid var(--fg); outline-offset: -4px; }
  .hub :global(.card button) { cursor: pointer; }
  .hub :global(.card .grip) { cursor: grab; }
  /* visually hidden but focusable and real */
  .hub :global(.fill) { position: absolute; width: 1px; height: 1px; margin: 0; padding: 0; border: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  /* drag.ts marks, as on Home (Home.svelte) */
  .grid :global([data-lifted]) { z-index: 3; scale: 1.03; background: var(--panel); box-shadow: 0 18px 40px rgba(0, 0, 0, .45); border-color: color-mix(in srgb, var(--accent) 55%, var(--line)); }
  .grid :global([data-landing]) { z-index: 2; background: var(--panel); }
  .grid :global([data-lifted]:hover) { transform: none; }
  .grid:global([data-sorted]) :global([data-enter]) { animation: none; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .big { font-size: 30px; font-weight: 700; /* rendered as 700 with the old static fonts; 650 would be a new, lighter weight */ letter-spacing: -.03em; margin: 6px 0 14px; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 8px 18px; margin: 0; }
  dt { color: var(--muted); font-size: 13px; }
  dd { margin: 0; font-family: var(--mono); font-variant-numeric: tabular-nums; font-size: 13px; text-align: right; }
  h5 { margin: 16px 0 6px; font-size: 13px; font-weight: 500; }
  .apps { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .apps li { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .apps span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sel { margin-left: auto; font-size: 12px; font-weight: 600; color: var(--accent); }
  .hd .sel { color: var(--on); background: color-mix(in srgb, var(--on) 18%, transparent); padding: 2px 9px; border-radius: 999px; font-weight: 700; }
  /* the drag handle sits in the card's top-right corner (28 px at 8 px): keep the label clear of it */
  .hub .sel { margin-right: 28px; }
  /* Spotlight: the selected card is lit, the others rest dimmed (their content, not .actions, so Use/Start stay full contrast) */
  @media (prefers-reduced-motion: no-preference) {
    .hub :global(.card) { transition: transform var(--spring-soft-dur) var(--spring-soft), scale .3s cubic-bezier(.2, .8, .2, 1), box-shadow .25s, border-color .3s cubic-bezier(.2, .8, .2, 1); }
    .hub :global(.card.dim > :not(.actions, .grip, .fill)) { transition: opacity .3s cubic-bezier(.2, .8, .2, 1), filter .3s cubic-bezier(.2, .8, .2, 1); }
    .sel { animation: sel-in .16s .12s ease-out backwards; }
  }
  .hub :global(.card.on:not([data-lifted])) { border-color: var(--accent); scale: 1.012; }
  .hub :global(.card.dim:not([data-lifted]) > :not(.actions, .grip, .fill)) { opacity: .5; filter: saturate(.5); }
  .hub :global(.card.dim:not([data-lifted]):hover > :not(.actions, .grip, .fill)) { opacity: .85; }
  @keyframes sel-in { from { opacity: 0; } }
  .sm { padding: 4px 10px; font-size: 13px; }
  /* V5 card: the header bleeds to the card edge (Card pads 16), the body pads itself, the buttons sit at the bottom of any height */
  .hub :global(.card.hc) { padding: 0; display: flex; flex-direction: column; }
  :global(.hub .card.hc .grip) { color: var(--on); }
  .hd { position: relative; padding: 14px 16px 12px; overflow: hidden; color: var(--on); background: var(--hbg); }
  /* not up: the colour only tints a grey block, and the text is the page's own */
  .hd.idle { background: linear-gradient(135deg, color-mix(in srgb, var(--h) 18%, #2b303b), color-mix(in srgb, var(--h) 6%, #171a22)); }
  /* the dot keeps a ring in the text colour so it reads on a header of its own colour */
  .hd :global(.dot) { box-shadow: 0 0 0 2px color-mix(in srgb, var(--on) 60%, transparent); }
  .hd h4 { font-size: 14px; color: inherit; min-width: 0; }
  .hd .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .word { font-size: 30px; font-weight: 700; letter-spacing: -.02em; line-height: 1.1; margin-top: 6px; }
  .hflex { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; }
  .hrings { display: flex; gap: 14px; align-items: center; margin-top: 10px; min-width: 0; flex-wrap: wrap; }
  .hr { display: grid; justify-items: center; gap: 3px; }
  .hr .k { font: 500 9.5px/1 var(--mono); letter-spacing: .07em; text-transform: uppercase; opacity: .85; }
  .lat { margin-left: auto; font: 600 12px var(--mono); text-align: right; white-space: nowrap; }
  .lat em { display: block; font-style: normal; font-size: 22px; line-height: 1; }
  .hsp { margin: 8px -16px -12px; opacity: .85; }
  .bd { padding: 12px 16px 0; display: grid; gap: 10px; }
  .bd h5 { margin: 4px 0 0; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .chips span { padding: 4px 10px; border-radius: 999px; border: 1px solid var(--line); background: color-mix(in srgb, #fff 4%, transparent); font-size: 12px; color: var(--muted); }
  .chips b { color: var(--fg); font-weight: 600; margin-left: 4px; }
  .calm { margin: 0; }
  .actions.pin { margin-top: auto; padding: 12px 16px 16px; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 18px; }
</style>
