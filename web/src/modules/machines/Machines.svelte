<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type StreamInfo } from '../../state/api';
  import { games } from '../../state/games';
  import { host, pollHost, refreshHost } from '../../state/host';
  import { toast } from '../../shell/Toasts.svelte';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { DASH, fmtTemp, fmtUptime } from '../home/home';
  import { machinesFrom, restartError } from './machines';

  const SUN = { ready: 'Ready', busy: 'Streaming', down: 'Down' } as const;
  let stream = $state<StreamInfo | null>(null);
  let confirming = $state(false);
  let busy = $state(false);

  const machines = $derived($host ? machinesFrom($host) : []);
  const app = $derived(stream?.appid == null ? null : ($games?.games.find((g) => g.appid === stream!.appid)?.name ?? `App ${stream.appid}`));

  onMount(() => {
    const stop = pollHost();
    const load = () => api.stream().then((s) => (stream = s), () => (stream = null));
    void load();
    const id = setInterval(() => !document.hidden && void load(), 10_000);
    return () => (stop(), clearInterval(id));
  });

  async function restart() {
    busy = true;
    try {
      await api.restartSunshine();
      toast('Sunshine is restarting');
      confirming = false;
      setTimeout(refreshHost, 3000);
    } catch (e) {
      toast(restartError(e), { error: true });
      confirming = false;
    } finally {
      busy = false;
    }
  }
</script>

<div class="top"><h2>Machines</h2></div>
<div class="grid">
  {#each machines as m (m.id)}
    {@const i = m.info}
    <Card cols={2} rows={2}>
      <h4><Icon name="host" color="var(--c-host)" size={16} /> {i.name} <span class="dot {i.sunshine}"></span></h4>
      <div class="big">Online</div>
      <dl>
        <dt>Sunshine</dt><dd>{SUN[i.sunshine]}</dd>
        <dt>Uptime</dt><dd>{fmtUptime(i.uptimeS)}</dd>
        <dt>GPU</dt><dd>{fmtTemp(i.gpuTempC)}</dd>
        <dt>CPU</dt><dd>{fmtTemp(i.cpuTempC)}</dd>
        <dt>Network</dt><dd>{i.path === 'relay' ? `Relay${i.relay ? ` · ${i.relay}` : ''}` : i.path === 'direct' ? 'Direct' : DASH}</dd>
        <dt>Running</dt><dd>{app ?? (stream?.state === 'live' ? 'Streaming' : stream ? 'Nothing' : DASH)}</dd>
      </dl>
      <div class="actions">
        {#if confirming}
          <span class="lab">Restart Sunshine? A live stream will drop.</span>
          <button class="btn" disabled={busy} onclick={restart}>Restart</button>
          <button class="btn ghost" disabled={busy} onclick={() => (confirming = false)}>Cancel</button>
        {:else}
          <button class="btn ghost" onclick={() => (confirming = true)}>Restart Sunshine</button>
          <button class="btn ghost" disabled title="Not in v1">Wake-on-LAN · later</button>
        {/if}
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
  .big { font-size: 30px; font-weight: 650; letter-spacing: -.03em; margin: 6px 0 14px; }
  dl { display: grid; grid-template-columns: auto 1fr; gap: 8px 18px; margin: 0; }
  dt { color: var(--muted); font-size: 13px; }
  dd { margin: 0; font-family: var(--mono); font-size: 13px; text-align: right; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 18px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--dim); }
  .dot.ready { background: var(--ok); }
  .dot.busy { background: var(--warn); }
  .dot.down { background: var(--bad); }
</style>
