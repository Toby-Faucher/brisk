<script lang="ts">
  import { host } from '../../state/host';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { DASH, fmtTemp, fmtUptime } from './home';

  const SUN = { ready: 'Sunshine ready', busy: 'Sunshine streaming', down: 'Sunshine down' } as const;
  const temps = $derived([
    ['GPU', $host?.gpuTempC ?? null],
    ['CPU', $host?.cpuTempC ?? null],
  ] as const);
  const path = $derived($host?.path === 'relay' ? `Relay${$host.relay ? ` · ${$host.relay}` : ''}` : $host?.path === 'direct' ? 'Direct' : DASH);
</script>

<Card cols={3} rows={2}>
  <h4><Icon name="host" color="var(--c-host)" size={16} /> {$host?.name ?? 'Host'} <span class="dot {$host?.sunshine ?? 'off'}"></span></h4>
  <div class="big">{$host ? 'Online' : DASH}</div>
  <div class="lab">{$host ? `${SUN[$host.sunshine]} · up ${fmtUptime($host.uptimeS)}` : 'Host status is not available here'}</div>
  <div class="stats">
    {#each temps as [k, c]}
      <div class="ring" style:--p={c ?? 0}><b>{c == null ? DASH : `${Math.round(c)}°`}</b></div>
      <div><div class="lab">{k}</div><b class="mono">{fmtTemp(c)}</b></div>
    {/each}
    <div><div class="lab">Network</div><b class="mono">{path}</b></div>
  </div>
</Card>

<style>
  .big { font-size: 30px; font-weight: 650; letter-spacing: -.03em; margin-top: 6px; }
  .stats { display: flex; gap: 16px; margin-top: 26px; align-items: center; flex-wrap: wrap; }
  .ring { width: 52px; height: 52px; border-radius: 50%; background: conic-gradient(var(--accent) calc(var(--p) * 1%), color-mix(in srgb, #fff 10%, transparent) 0); display: grid; place-items: center; }
  .ring b { width: 40px; height: 40px; border-radius: 50%; background: #0f1116; display: grid; place-items: center; font-size: 12px; font-family: var(--mono); }
</style>
