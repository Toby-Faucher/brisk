<script lang="ts">
  import { hub, machines, selectedMachine } from '../../state/machines';
  import { STATE_LABEL } from './machines';
  import { host } from '../../state/host';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';

  const SUN = { ready: 'Sunshine ready', busy: 'Streaming', down: 'Sunshine down' } as const;
</script>

<Card cols={2}>
  <a href="/machines" class="fill" aria-label="Open machines"></a>
  <h4><Icon name="host" color="var(--c-host)" size={16} /> Machines</h4>
  {#if $hub === true}
    <div class="big">{$machines.length} <span class="lab">{$machines.length === 1 ? 'machine' : 'machines'}</span></div>
    <div class="lab">{$selectedMachine ? `${$selectedMachine.name} · ${$selectedMachine.state === 'ready' && $selectedMachine.sunshine ? SUN[$selectedMachine.sunshine] : STATE_LABEL[$selectedMachine.state]}` : 'No machine selected'}</div>
  {:else}
    <div class="big">1 <span class="lab">machine</span></div>
    <div class="lab">{$host ? `${$host.name} · ${SUN[$host.sunshine]}` : 'Status is not available here'}</div>
  {/if}
</Card>

<style>
  .fill { position: absolute; inset: 0; }
  .big { font-size: 24px; font-weight: 700; /* rendered as 700 with the old static fonts; 650 would be a new, lighter weight */ letter-spacing: -.03em; margin-top: 6px; }
</style>
