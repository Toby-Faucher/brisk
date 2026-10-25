<script lang="ts">
  import { onMount } from 'svelte';
  import { host, pollHost } from '../state/host';
  import Icon from '../ui/Icon.svelte';

  onMount(() => pollHost()); // every 10 s while visible; cleaned up on each remount
  const label = { ready: 'Sunshine ready', busy: 'Streaming', down: 'Sunshine down' } as const;
</script>

<a class="pill" href="/machines" title={$host ? label[$host.sunshine] : 'Host not reachable'}>
  <Icon name="host" color={$host ? 'var(--ok)' : 'var(--muted)'} />
  <span class="name">{$host?.name ?? '–'}</span>
  <span class="dot {$host?.sunshine ?? 'off'}"></span>
</a>

<style>
  .pill { margin-top: auto; display: flex; align-items: center; gap: 10px; padding: 7px 10px; border-radius: 10px; color: var(--muted); transition: background .18s, color .18s; }
  .pill:hover { background: var(--panel2); color: var(--fg); }
  .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
