<script lang="ts">
  // The launch option defaults for every game on this machine, with autosave. In Settings > Launch and in the Emulators tab.
  import { onDestroy, onMount } from 'svelte';
  import OptionsEditor from './OptionsEditor.svelte';
  import { withAllow } from './form';
  import { createDefaults, SAVED } from './store';

  let { uid }: { uid: string } = $props();
  const ctl = createDefaults();
  const s = ctl.state;
  onMount(() => void ctl.load());
  onDestroy(() => void ctl.flush()); // leaving the page does not drop an edit waiting for its pause
  const line = $derived($s.saving ? 'Saving...' : $s.note?.tone === 'info' ? $s.note.text : $s.saved ? SAVED : '');
</script>

<div class="ld" aria-busy={$s.load === 'loading'}>
  {#if $s.load === 'loading'}
    <p class="note">Loading the launch options...</p>
  {:else if $s.load === 'error' || !$s.draft || !$s.server}
    <div class="err"><span role="alert">{$s.note?.text}</span><button type="button" class="btn ghost sm" onclick={() => ctl.retry()}>Try again</button></div>
  {:else}
    {@const d = $s.draft}
    <OptionsEditor
      {uid}
      form={d.form}
      allow={d.allow}
      envAllowed={$s.server.envAllowed}
      onchange={(form, now) => ctl.edit({ ...d, form }, now)}
      onallow={(on) => ctl.edit({ form: withAllow(d.form, on), allow: on }, true)} />
    {#if $s.problems.length}
      <div class="bad" role="alert">
        <p>These options were not saved:</p>
        <ul>{#each $s.problems as p (p)}<li>{p}</li>{/each}</ul>
      </div>
    {/if}
    {#if $s.note?.tone === 'error'}
      <div class="err"><span role="alert">{$s.note.text}</span><button type="button" class="btn ghost sm" onclick={() => ctl.retry()}>Try again</button></div>
    {/if}
    <p class="status" role="status">{line}</p>
  {/if}
</div>

<style>
  .note { margin: 10px 0 0; font-size: 12.5px; color: var(--muted); }
  .status { margin: 12px 0 0; min-height: 1.3em; font-size: 12.5px; color: var(--muted); }
  .err, .bad { margin-top: 12px; font-size: 13px; color: var(--warn); display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .bad { display: block; }
  .bad p { margin: 0; }
  .bad ul { margin: 4px 0 0; padding-left: 18px; }
</style>
