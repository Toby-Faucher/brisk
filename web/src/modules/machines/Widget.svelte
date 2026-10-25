<script lang="ts">
  import { host } from '../../state/host';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';

  const SUN = { ready: 'Sunshine ready', busy: 'Streaming', down: 'Sunshine down' } as const;
</script>

<Card cols={2}>
  <a href="/machines" class="fill" aria-label="Open machines"></a>
  <h4><Icon name="host" color="var(--c-host)" size={16} /> Machines</h4>
  <div class="big">1 <span class="lab">machine</span></div>
  <div class="lab">{$host ? `${$host.name} · ${SUN[$host.sunshine]}` : 'Status is not available here'}</div>
</Card>

<style>
  .fill { position: absolute; inset: 0; }
  .big { font-size: 24px; font-weight: 650; letter-spacing: -.03em; margin-top: 6px; }
</style>
