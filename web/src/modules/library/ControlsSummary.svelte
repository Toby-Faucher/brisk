<script lang="ts">
  // The Controls section of an emulator game's detail panel: a short note and a link into Settings, where the Dolphin
  // mapping is edited (docs/plans/2026-10-28-emulator-controls-v2.md, section 1.9). Hidden when this machine has no Dolphin.
  import { onMount } from 'svelte';
  import { api, ApiError } from '../../state/api';

  let hidden = $state(false);
  onMount(() => {
    api.emulatorControls('classic').catch((e) => {
      if (e instanceof ApiError && e.status === 404) hidden = true;
    });
  });
</script>

{#if !hidden}
  <section class="sec" aria-labelledby="ctl-h">
    <h3 id="ctl-h">Controls</h3>
    <p class="lab">Dolphin uses one mapping for every game on this machine. Wii games use the Classic Controller mapping. GameCube games use the GameCube controller mapping.</p>
    <p class="lab"><a class="link" href="/settings#emulators/dolphin/controllers">Edit mappings</a></p>
  </section>
{/if}

<style>
  .sec { padding: 14px 22px 0; display: grid; gap: 8px; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
  .lab { margin: 0; }
  .link { color: var(--accent); }
  .link:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
</style>
