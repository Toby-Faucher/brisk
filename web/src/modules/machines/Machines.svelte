<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { enter } from '../../shell/reveal';
  import { Dialog } from 'bits-ui';
  import { api, type StreamInfo } from '../../state/api';
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
  import { machinesFrom, restartError } from './machines';

  const SUN = { ready: 'Ready', busy: 'Streaming', down: 'Down' } as const;
  let stream = $state<StreamInfo | null>(null);
  let confirming = $state(false);
  let busy = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const machines = $derived($hostStore ? machinesFrom($hostStore) : []);
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
    <Card cols={2} rows={2}>
      <h4><Icon name="host" color="var(--c-host)" size={16} /> {i.name} <StatusDot state={i.sunshine} /></h4>
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
        <dt>Running</dt><dd>{app ?? (stream?.state === 'live' ? 'Streaming' : stream ? 'Nothing' : DASH)}</dd>
      </dl>
      {#if running.length}
        <h5 class="lab">Running apps</h5>
        <ul class="apps">
          {#each running as g (g.appid)}
            {@const q = $quitting.has(g.appid)}
            <li><span>{g.name}</span><button class="btn ghost sm" aria-disabled={q} onclick={() => askQuit(g)} aria-label={q ? `Quitting ${g.name}` : `Quit ${g.name}`}>{q ? 'Quitting…' : 'Quit'}</button></li>
          {/each}
        </ul>
      {/if}
      <div class="actions">
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
        <button class="btn ghost" disabled title="Not in v1">Wake-on-LAN · later</button>
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
  .sm { padding: 4px 10px; font-size: 13px; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 18px; }
</style>
