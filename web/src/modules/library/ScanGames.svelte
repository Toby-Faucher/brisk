<script lang="ts">
  // "Scan games folder": asks a machine to look through its games folder and add what it finds. The summary stays
  // on screen (aria-live) so it can be read again. `machine` null: the selected machine, or the single host.
  import { api, ApiError, type ScanResult } from '../../state/api';
  import { refreshGames } from '../../state/games';
  import { selected } from '../../state/machines';
  import { scanRows } from '../../state/uploads';

  let { machine = null }: { machine?: string | null } = $props();
  let busy = $state(false);
  let back = $state(false);
  let result = $state<ScanResult | null>(null);
  let failed = $state('');

  const SAY: Record<number, string> = { 409: 'A scan is already running. Try again in a moment.', 429: 'It was scanned a moment ago. Try again in a few seconds.' };

  async function scan() {
    if (busy) return;
    busy = true;
    failed = '';
    result = null;
    try {
      result = await api.scanLibrary(machine, back);
      if (result.added.length && (machine === null || machine === $selected)) void refreshGames();
    } catch (e) {
      failed = (e instanceof ApiError && SAY[e.status]) || 'Could not scan the games folder.';
    }
    busy = false;
  }
</script>

<div class="scan">
  <div class="row">
    <button class="btn ghost" aria-disabled={busy} aria-busy={busy} onclick={scan}>{busy ? 'Scanning…' : 'Scan games folder'}</button>
    <label class="back"><input type="checkbox" bind:checked={back} /> Also bring back games I removed</label>
  </div>
  <div class="out" role="status" aria-live="polite">
    {#if failed}
      <p class="err">{failed}</p>
    {:else if result}
      <dl class="sum" aria-label="Scan result">
        {#each scanRows(result) as [label, n] (label)}<div><dt>{label}</dt><dd>{n}</dd></div>{/each}
      </dl>
      {#if result.truncated}<p class="lab">The folder is big. Only the first part was scanned.</p>{/if}
      {#if !result.scanned}<p class="lab">No game files in the games folder yet.</p>{/if}
    {/if}
  </div>
</div>

<style>
  .scan { display: grid; gap: 8px; }
  .row { display: flex; gap: 14px; align-items: center; flex-wrap: wrap; }
  .back { display: inline-flex; gap: 6px; align-items: center; font-size: 13px; color: var(--muted); }
  .out:empty { display: none; }
  .sum { display: flex; flex-wrap: wrap; gap: 6px 18px; margin: 0; }
  .sum div { display: flex; gap: 6px; align-items: baseline; }
  dt { color: var(--muted); font-size: 13px; }
  dd { margin: 0; font-weight: 600; }
  .err { color: var(--warn); margin: 0; }
  p { margin: 0; }
</style>
