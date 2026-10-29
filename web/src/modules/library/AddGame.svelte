<script lang="ts">
  // "Add game": send game files to a machine (resumable, two at a time, state/uploads.ts), or scan its games folder.
  // The dialog can be closed while files are going; the list is still here when it opens again.
  import { Dialog } from 'bits-ui';
  import { onMount } from 'svelte';
  import { api } from '../../state/api';
  import { refreshGames } from '../../state/games';
  import { hub, machines, selected } from '../../state/machines';
  import { createUploader, fmtEta, fmtSpeed, type Item } from '../../state/uploads';
  import Dropdown from '../../ui/Dropdown.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { fmtSize } from './library';
  import ScanGames from './ScanGames.svelte';

  const ACCEPT = '.iso,.gcm,.wbfs,.rvz,.wia,.nes,.sfc,.smc,.gb,.gbc,.gba,.md,.gen';
  let open = $state(false);
  let items = $state<Item[]>([]);
  let dragging = $state(false);
  let picked = $state<string | null>(null);
  let input: HTMLInputElement | undefined = $state();

  // On a hub the files go to the machine picked here (default: the selected one); on a single host there is nothing to pick.
  const target = $derived($hub ? (picked ?? $selected ?? $machines[0]?.id ?? null) : null);
  const targetMachine = $derived($machines.find((m) => m.id === target) ?? null);
  const notReady = $derived($hub === true && targetMachine?.state !== 'ready');
  const options = $derived($machines.map((m): [string, string] => [m.id, m.state === 'ready' ? m.name : `${m.name} (${m.state})`]));

  const up = createUploader({
    onChange: (l) => (items = l),
    // The list on screen belongs to the selected machine; another machine's new game shows when it is selected.
    onDone: () => void (target === $selected || !$hub ? refreshGames() : 0),
  });
  const busy = $derived(items.some((i) => ['queued', 'sending', 'finishing'].includes(i.state)));
  const finished = $derived(items.filter((i) => i.state === 'done').length);

  function add(files: FileList | File[] | null) {
    const list = [...(files ?? [])];
    if (!list.length || notReady) return;
    up.add(list, api.uploadsFor(target));
  }
  const pct = (i: Item) => (i.size ? Math.round((i.offset / i.size) * 100) : 0);

  // A page left mid-upload loses the file the browser is reading: ask first.
  onMount(() => {
    const warn = (e: BeforeUnloadEvent) => busy && e.preventDefault();
    addEventListener('beforeunload', warn);
    return () => removeEventListener('beforeunload', warn);
  });
</script>

<Dialog.Root bind:open>
  <Dialog.Trigger class="btn ghost">Add game</Dialog.Trigger>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim ag-dim" />
    <Dialog.Content class="rs glass ag">
      <Dialog.Title class="rs-t">Add game</Dialog.Title>
      <Dialog.Description class="lab">Send game files to a machine. They go in its Games folder, sorted by system.</Dialog.Description>

      {#if $hub}
        <div class="who">
          <span class="lab" id="ag-who">Send to</span>
          <Dropdown value={target ?? ''} {options} label="Machine to send to" onchange={(v) => (picked = v)} />
        </div>
        {#if notReady}<p class="warn" role="note">This machine is not ready. Wake it first, then add games.</p>{/if}
      {/if}

      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="drop"
        class:over={dragging}
        ondragover={(e) => (e.preventDefault(), (dragging = true))}
        ondragleave={() => (dragging = false)}
        ondrop={(e) => (e.preventDefault(), (dragging = false), add(e.dataTransfer?.files ?? null))}
      >
        <Icon name="dl" size={18} />
        <span>Drop game files here, or</span>
        <button class="btn" aria-disabled={notReady} onclick={() => !notReady && input?.click()}>Choose files</button>
        <input bind:this={input} type="file" multiple accept={ACCEPT} class="vh" tabindex="-1" aria-label="Game files" onchange={(e) => (add(e.currentTarget.files), (e.currentTarget.value = ''))} />
      </div>

      <p class="lab" role="status" aria-live="polite">
        {#if !items.length}Wii, GameCube and cartridge files work. Big files can take a while.{:else}{finished} of {items.length} done{/if}
      </p>

      {#if items.length}
        <ul class="files" aria-label="Files">
          {#each items as i (i.key)}
            <li class:bad={i.state === 'failed'}>
              <div class="line">
                <b class="n" title={i.name}>{i.name}</b>
                <span class="lab">{fmtSize(i.size)}</span>
                {#if i.state === 'sending' || i.state === 'queued' || i.state === 'finishing' || i.state === 'failed'}
                  <button class="btn ghost sm" aria-label="Cancel {i.name}" onclick={() => up.cancel(i.key)}>Cancel</button>
                {/if}
                {#if i.state === 'failed'}<button class="btn ghost sm" aria-label="Try {i.name} again" onclick={() => up.retry(i.key)}>Try again</button>{/if}
              </div>
              {#if i.state === 'sending' || i.state === 'finishing'}
                <div class="bar" role="progressbar" aria-label="Sending {i.name}" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct(i)}><i style="width:{pct(i)}%"></i></div>
              {/if}
              <p class="st" class:err={i.state === 'failed'}>
                {#if i.state === 'queued'}Waiting
                {:else if i.state === 'sending'}{pct(i)}%{i.speed ? ` · ${fmtSpeed(i.speed)}` : ''}{i.etaS ? ` · ${fmtEta(i.etaS)}` : ''}
                {:else if i.state === 'finishing'}Checking the file…
                {:else if i.state === 'done'}Added{i.note ? `. ${i.note}` : ''}
                {:else if i.state === 'failed'}{i.error}
                {:else}Cancelled{/if}
              </p>
            </li>
          {/each}
        </ul>
        {#if !busy}<button class="btn ghost sm clr" onclick={() => up.clear()}>Clear list</button>{/if}
      {/if}

      <div class="sep"></div>
      <p class="lab">Put files in the Games folder some other way? Scan to add them.</p>
      {#key target}<ScanGames machine={target} />{/key}

      <div class="rs-a"><Dialog.Close class="btn ghost">Close</Dialog.Close></div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  :global(.rs.ag) { top: 12%; width: min(560px, calc(100vw - 32px)); max-height: 80vh; overflow: auto; gap: 10px; }
  :global(.cm-dim.ag-dim) { z-index: 40; }
  .who { display: flex; align-items: center; gap: 10px; }
  .warn { margin: 0; color: var(--warn); font-size: 13px; }
  .drop { display: flex; align-items: center; justify-content: center; gap: 10px; flex-wrap: wrap; padding: 22px 14px; border: 1px dashed var(--line); border-radius: 14px; color: var(--muted); }
  .drop.over { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .vh { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  .files { list-style: none; margin: 0; padding: 0; display: grid; gap: 10px; }
  .line { display: flex; align-items: center; gap: 8px; }
  .n { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .sm { padding: 3px 10px; min-height: 0; font-size: 12.5px; }
  .clr { justify-self: start; }
  .bar { height: 6px; margin-top: 6px; border-radius: 3px; background: var(--line); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--accent); }
  @media (prefers-reduced-motion: no-preference) { .bar i { transition: width .25s linear; } }
  .st { margin: 4px 0 0; font-size: 12.5px; color: var(--muted); }
  .st.err { color: var(--warn); }
  .sep { border-top: 1px solid var(--line); }
  p { margin: 0; }
</style>
