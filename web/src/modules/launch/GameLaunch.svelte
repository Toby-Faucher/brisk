<script lang="ts">
  // The Launch options section of a game's detail panel (Steam and emulator games). Each field follows the default
  // (Inherit) or is the game's own (Override). Steam games also show the line to paste into Steam, once.
  import { onDestroy, onMount } from 'svelte';
  import type { Game } from '../../state/api';
  import { copyText } from './copy';
  import { FIELDS, inheritOf, launchIdOf, summary } from './form';
  import OptionsEditor from './OptionsEditor.svelte';
  import { createGameLaunch, SAVED } from './store';

  let { game }: { game: Pick<Game, 'id' | 'appid'> } = $props();
  // The parent keys this component by the game id, so reading it once is right.
  // svelte-ignore state_referenced_locally
  const ctl = createGameLaunch(launchIdOf(game));
  const s = ctl.state;
  onMount(() => void ctl.load());
  onDestroy(() => void ctl.flush());

  let field: HTMLInputElement | undefined = $state();
  let copied = $state('');
  async function copy(line: string) {
    copied = (await copyText(line, field)) === 'copied' ? 'Copied.' : 'Selected. Press Ctrl+C to copy.';
  }
  const own = $derived($s.server ? FIELDS.filter((f) => !inheritOf($s.server!.override)[f]) : []);
  const NAME = { wrappers: 'wrappers', env: 'environment variables', custom: 'your own command', extra: 'extra arguments' } as const;
  const line = $derived($s.saving ? 'Saving...' : $s.note?.tone === 'info' ? $s.note.text : $s.saved ? SAVED : '');
</script>

<section class="sec" aria-labelledby="lo-h" aria-busy={$s.load === 'loading'}>
  <h3 id="lo-h">Launch options</h3>
  {#if $s.load === 'loading'}
    <p class="lab">Loading the launch options...</p>
  {:else if $s.load === 'error' || !$s.draft || !$s.server}
    <p class="warn"><span role="alert">{$s.note?.text}</span> <button type="button" class="btn ghost sm" onclick={() => ctl.retry()}>Try again</button></p>
  {:else}
    {@const d = $s.draft}
    {@const sv = $s.server}
    <p class="lab">{summary(sv.effective)}</p>
    <p class="lab">{own.length ? `This game has its own ${own.map((f) => NAME[f]).join(', ')}. The rest follows the defaults.` : 'This game follows the defaults.'} <a class="link" href="/settings#launch">Edit the defaults</a></p>
    {#if sv.problems.length}
      <div class="warn" role="alert">
        <p>These options will not be used, and the game starts without them:</p>
        <ul>{#each sv.problems as p (p)}<li>{p}</li>{/each}</ul>
      </div>
    {/if}

    {#if sv.steamLine}
      <div class="steam">
        <label for="lo-line">Steam launch option</label>
        <div class="copyrow">
          <input id="lo-line" class="line" type="text" readonly value={sv.steamLine} bind:this={field} aria-describedby="lo-hint" />
          <button type="button" class="btn ghost sm" onclick={() => copy(sv.steamLine!)}>Copy</button>
        </div>
        <p id="lo-hint" class="lab">Paste this once into the game's Launch Options in Steam (Properties, General). After that, change everything here.</p>
        <p id="lo-copy" class="lab" role="status">{copied}</p>
      </div>
    {/if}

    <details>
      <summary>Change launch options</summary>
      <OptionsEditor
        uid="lo"
        form={d.form}
        inherit={d.inherit}
        allow={sv.allowCustomCommands}
        envAllowed={sv.envAllowed}
        onchange={(form, now) => ctl.edit({ ...d, form }, now)}
        oninherit={(inherit) => ctl.edit({ ...d, inherit }, true)} />
    </details>
    {#if $s.problems.length}
      <div class="warn" role="alert">
        <p>These options were not saved:</p>
        <ul>{#each $s.problems as p (p)}<li>{p}</li>{/each}</ul>
      </div>
    {/if}
    {#if $s.note?.tone === 'error'}
      <p class="warn"><span role="alert">{$s.note.text}</span> <button type="button" class="btn ghost sm" onclick={() => ctl.retry()}>Try again</button></p>
    {/if}
    <div class="foot">
      <button type="button" class="btn ghost" disabled={!sv.override || $s.saving} onclick={() => ctl.reset()}>Reset to the defaults</button>
      <p class="lab" role="status">{line}</p>
    </div>
  {/if}
</section>

<style>
  .sec { padding: 14px 22px 0; display: grid; gap: 8px; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
  .lab { margin: 0; }
  .warn { margin: 0; font-size: 13px; color: var(--warn); }
  .warn p { margin: 0; }
  .warn ul { margin: 4px 0 0; padding-left: 18px; }
  .link { color: var(--accent); }
  .steam { display: grid; gap: 6px; }
  .steam label { font-size: 13px; font-weight: 600; }
  .copyrow { display: flex; gap: 8px; }
  .line { flex: 1; min-width: 0; font-family: var(--mono); font-size: 12.5px; padding: 6px 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--panel); color: var(--fg); }
  summary { cursor: pointer; font-size: 13.5px; }
  summary:focus-visible, .line:focus-visible, .link:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .foot { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
</style>
