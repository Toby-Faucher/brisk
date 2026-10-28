<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { enter } from '../../shell/reveal';
  import { Dialog } from 'bits-ui';
  import { api, type HostInfo, type StreamInfo } from '../../state/api';
  import type { Machine } from '../../state/machines';
  import { games, watchGames } from '../../state/games';
  import { askQuit, quitting } from '../../shell/QuitDialog.svelte';
  import { host as hostStore, refreshHost } from '../../state/host';
  import { toast } from '../../shell/Toasts.svelte';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import StatusDot from '../../ui/StatusDot.svelte';
  import ConnPath from './ConnPath.svelte';
  import { count, fadeOnChange } from '../../ui/motion';
  import { DASH, fmtTemp, fmtUptime } from '../home/home';
  import { hub, machines as hubMachines, selected, selectMachine, powerMachine } from '../../state/machines';
  import { machinesFrom, restartError, dotState, powerWhy, STATE_LABEL } from './machines';

  const SUN = { ready: 'Ready', busy: 'Streaming', down: 'Down' } as const;
  let stream = $state<StreamInfo | null>(null);
  let confirming = $state(false);
  let busy = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  type Entry = { id: string; name: string; state: Machine['state']; canPower: boolean; info: HostInfo | null };
  const hubOn = $derived($hub === true);
  // single-host mode keeps today's one card from the host store
  const machines = $derived<Entry[]>(hubOn ? $hubMachines.map((m) => ({ id: m.id, name: m.name, state: m.state, canPower: m.canPower, info: m.host })) : ($hostStore ? machinesFrom($hostStore) : []).map((m) => ({ id: m.id, name: m.info.name, state: 'ready', canPower: false, info: m.info })));
  let shutting = $state<Entry | null>(null);
  const failed = (what: string, e: unknown) => toast(`Could not ${what}: ${(e as Error).message}`, { error: true });
  const start = (m: Entry) => powerMachine(m.id, 'start').catch((e) => failed(`start ${m.name}`, e));
  async function shutDown(m: Entry) {
    shutting = null;
    await powerMachine(m.id, 'shutdown').catch((e) => failed(`shut down ${m.name}`, e));
  }
  const app = $derived(stream?.appid == null ? null : ($games?.games.find((g) => g.appid === stream!.appid)?.name ?? `App ${stream.appid}`));

  let arrived = false; // the host card lands after hydration: entrance once
  $effect(() => {
    if (machines.length && !arrived) (arrived = true), void tick().then(enter);
  });

  const running = $derived(($games?.games ?? []).filter((g) => g.running));

  onMount(() => {
    const unwatch = watchGames();
    const load = () => api.stream().then((s) => (stream = s), () => (stream = null));
    void load();
    const id = setInterval(() => !document.hidden && void load(), 10_000);
    return () => (clearInterval(id), clearTimeout(timer), unwatch()); // HostPill keeps $host fresh
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
<div class="grid">
  {#each machines as m (m.id)}
    {@const i = m.info}
    {@const here = !hubOn || m.id === $selected}
    {@const why = powerWhy(m.state)}
    <Card cols={2} rows={2}>
      <h4><Icon name="host" color="var(--c-host)" size={16} /> {m.name} <StatusDot state={i ? dotState({ state: m.state, sunshine: i.sunshine }) : dotState({ state: m.state, sunshine: null })} />{#if hubOn && here}<span class="sel">Selected</span>{/if}</h4>
      {#if hubOn && !i}
        <div class="big">{STATE_LABEL[m.state]}</div>
        <p class="lab">{m.state === 'off' ? `${m.name} is switched off. Start it when you want to play.` : m.state === 'starting' ? `${m.name} is waking up. This takes a minute or two.` : m.state === 'stopping' ? `${m.name} is shutting down.` : `Brisk cannot reach ${m.name} right now.`}</p>
      {:else if i}
      {@const big = i.sunshine === 'down' ? 'Sunshine is down' : 'Online'}
      {@const up = fmtUptime(i.uptimeS)}
      <div class="big" {@attach fadeOnChange(big)}>{big}</div>
      <ConnPath path={i.path} />
      <dl>
        <dt>Sunshine</dt><dd {@attach fadeOnChange(SUN[i.sunshine])}>{SUN[i.sunshine]}</dd>
        <dt>Uptime</dt><dd {@attach fadeOnChange(up)}>{up}</dd>
        <dt>GPU</dt><dd {@attach count(i.gpuTempC, fmtTemp)}>{fmtTemp(i.gpuTempC)}</dd>
        <dt>CPU</dt><dd {@attach count(i.cpuTempC, fmtTemp)}>{fmtTemp(i.cpuTempC)}</dd>
        <dt>Network</dt><dd>{i.path === 'relay' ? `Relay${i.relay ? ` · ${i.relay}` : ''}` : i.path === 'direct' ? 'Direct' : DASH}</dd>
        <dt>Running</dt><dd>{here ? (app ?? (stream?.state === 'live' ? 'Streaming' : stream ? 'Nothing' : DASH)) : DASH}</dd>
      </dl>
      {#if here && running.length}
        <h5 class="lab">Running apps</h5>
        <ul class="apps">
          {#each running as g (g.appid)}
            {@const q = $quitting.has(g.appid)}
            <li><span>{g.name}</span><button class="btn ghost sm" aria-disabled={q} onclick={() => askQuit(g)} aria-label={q ? `Quitting ${g.name}` : `Quit ${g.name}`}>{q ? 'Quitting…' : 'Quit'}</button></li>
          {/each}
        </ul>
      {/if}
      {/if}
      <div class="actions">
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
    </Card>
  {:else}
    <Card cols={2}>
      <h4><Icon name="host" color="var(--c-host)" size={16} /> Machines</h4>
      <div class="big">{DASH}</div>
      <div class="lab">Machine status is not available here. Brisk could not reach the host agent.</div>
    </Card>
  {/each}
</div>
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
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 14px; padding-top: 16px; }
  .big { font-size: 30px; font-weight: 700; /* rendered as 700 with the old static fonts; 650 would be a new, lighter weight */ letter-spacing: -.03em; margin: 6px 0 14px; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 8px 18px; margin: 0; }
  dt { color: var(--muted); font-size: 13px; }
  dd { margin: 0; font-family: var(--mono); font-variant-numeric: tabular-nums; font-size: 13px; text-align: right; }
  h5 { margin: 16px 0 6px; font-size: 13px; font-weight: 500; }
  .apps { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .apps li { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  .apps span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sel { margin-left: auto; font-size: 12px; font-weight: 600; color: var(--accent); }
  .sm { padding: 4px 10px; font-size: 13px; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 18px; }
</style>
