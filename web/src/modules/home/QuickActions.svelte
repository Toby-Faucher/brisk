<script lang="ts">
  import { api, type Preset } from '../../state/api';
  import { settings } from '../../state/settings';
  import { toast, toastError } from '../../shell/Toasts.svelte';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';

  const presets: Preset[] = ['fast', 'balanced', 'crisp'];
  let restarting = $state(false);

  async function restart() {
    restarting = true;
    try {
      await api.restartSunshine();
      toast('Sunshine is restarting');
    } catch (e) {
      toastError('restart Sunshine', e);
    } finally {
      restarting = false;
    }
  }
</script>

<Card cols={3}>
  <div class="head">
    <h4><Icon name="bolt" color="var(--c-power)" size={16} /> Quick actions</h4>
    <div class="chips" role="radiogroup" aria-label="Default quality">
      {#each presets as p}
        <button class="chip" role="radio" aria-checked={$settings.defaultPreset === p} onclick={() => ($settings.defaultPreset = p)}>{p[0].toUpperCase() + p.slice(1)}</button>
      {/each}
    </div>
  </div>
  <div class="row">
    <button class="btn ghost" disabled title="Wake-on-LAN comes later">Wake <span class="tag">later</span></button>
    <button class="btn ghost" disabled={restarting} onclick={restart}>Restart Sunshine</button>
  </div>
</Card>

<style>
  .head { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin: -4px 28px -4px 0; } /* right: clear of the drag handle */
  .row { display: flex; gap: 8px; margin-top: 18px; flex-wrap: wrap; align-items: center; }
  .btn.ghost { background: color-mix(in srgb, #fff 9%, transparent); box-shadow: none; backdrop-filter: blur(6px); }
  .chips { display: flex; gap: 3px; padding: 3px; border-radius: var(--r-ctl); background: color-mix(in srgb, #fff 6%, transparent); }
  .chip { border: 0; background: transparent; color: var(--muted); font-weight: 500; padding: 4px 10px; font-size: 12.5px; border-radius: 9px; cursor: pointer; transition: background .2s var(--ease), color .2s; }
  .chip[aria-checked='true'] { background: var(--panel2); color: var(--fg); box-shadow: 0 0 0 1px var(--line); }
</style>
