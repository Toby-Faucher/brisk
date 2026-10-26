<script lang="ts">
  import { onMount } from 'svelte';
  import { Tabs } from 'bits-ui';
  import { ACCENT_HEX, REPLAY_SECONDS, settings, type Accent, type ReplaySeconds, type Settings } from '../../state/settings';
  import type { Preset } from '../../state/api';
  import { host } from '../../state/host';
  import Card from '../../ui/Card.svelte';
  import { glideSwap } from '../../ui/motion';
  import ControllerTest from './ControllerTest.svelte';
  import { TABS, TAB_KEY, hashFor, initialTab, parseHash, type Tab } from './tabs';

  const PRESETS: Exclude<Preset, 'custom'>[] = ['fast', 'balanced', 'crisp'];
  const set = (p: Partial<Settings>) => settings.update((s) => ({ ...s, ...p }));
  const ACCENTS = Object.keys(ACCENT_HEX) as Accent[];
  function arrow(e: KeyboardEvent) {
    const d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!d) return;
    e.preventDefault();
    const a = ACCENTS[(ACCENTS.indexOf($settings.accent) + d + ACCENTS.length) % ACCENTS.length];
    pickAccent(a);
    (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`[aria-label="${a}"]`)?.focus();
  }
  const pickAccent = (a: Accent) => set({ accent: a });

  const LABEL: Record<Tab, string> = { general: 'General', streaming: 'Streaming', controller: 'Controller', about: 'About' };
  const SHORTCUTS = [['Ctrl+K', 'Command menu'], ['/', 'Search'], ['g h', 'Go to Home'], ['g l', 'Go to Library'], ['g s', 'Go to Stream'], ['Hold Esc', 'Release the keyboard on the stream'], ['Ctrl+Alt+S', 'Screenshot (on the stream)'], ['Ctrl+Alt+R', 'Save replay (on the stream)']];
  const DOCS = ['docs/ui-notes.md', 'docs/ui-design.md', 'docs/controller-notes.md'];
  const commit = typeof __BRISK_COMMIT__ === 'string' ? __BRISK_COMMIT__ : 'unknown';

  // The server render shows General; the real tab is picked on mount (hash, then last used).
  let tab = $state<Tab>('general');
  let list = $state<HTMLElement | null>(null);
  let ind = $state({ x: 0, w: 0 });
  let live = $state(false);
  let quality: HTMLElement | undefined = $state();
  function show(t: Tab) {
    tab = t;
    live = true;
    try { localStorage.setItem(TAB_KEY, t); } catch {}
    if (location.hash !== hashFor(t)) history.replaceState(history.state, '', hashFor(t));
  }
  const measure = () => {
    const el = list?.querySelector<HTMLElement>(`[data-value="${tab}"]`);
    if (el) ind = { x: el.offsetLeft, w: el.offsetWidth };
  };
  $effect(() => { tab; list; measure(); });
  onMount(() => {
    let stored: string | null = null;
    try { stored = localStorage.getItem(TAB_KEY); } catch {}
    tab = initialTab(location.hash, stored);
    const onHash = () => { const t = parseHash(location.hash); if (t) (tab = t, live = true); };
    window.addEventListener('hashchange', onHash);
    window.addEventListener('popstate', onHash);
    window.addEventListener('resize', measure);
    document.fonts?.ready.then(measure);
    return () => {
      window.removeEventListener('hashchange', onHash);
      window.removeEventListener('popstate', onHash);
      window.removeEventListener('resize', measure);
    };
  });
</script>

<div class="top reveal"><h2>Settings</h2></div>
<div class="st" class:live>
  <!-- `live` turns on the panel fade only after a user tab change, so first load animates once (page-in) -->

  <Tabs.Root value={tab} onValueChange={(v) => show(v as Tab)}>
    <Tabs.List class="strip" aria-label="Settings sections" bind:ref={list}>
      {#each TABS as t (t)}<Tabs.Trigger value={t} class="tab">{LABEL[t]}</Tabs.Trigger>{/each}
      <span class="ind" style:transform="translateX({ind.x}px)" style:width="{ind.w}px" aria-hidden="true"></span>
    </Tabs.List>

    <Tabs.Content value="general">
      <div class="col">
        <Card cols={2}>
          <h4>Accent</h4>
          <div class="row" role="radiogroup" aria-label="Accent colour">
            {#each Object.entries(ACCENT_HEX) as [a, hex] (a)}
              <button class="sw" style:background={hex} role="radio" aria-checked={$settings.accent === a} aria-label={a} tabindex={$settings.accent === a ? 0 : -1} onkeydown={arrow} onclick={() => pickAccent(a as Accent)}></button>
            {/each}
          </div>
        </Card>
        <Card cols={2}>
          <h4>Effects</h4>
          <label class="opt"><span>Sounds</span><input type="checkbox" checked={$settings.sounds} onchange={(e) => set({ sounds: e.currentTarget.checked })} /></label>
          <label class="opt"><span>Shaders <small class="lab">background smoke</small></span><input type="checkbox" checked={$settings.shaders} onchange={(e) => set({ shaders: e.currentTarget.checked })} /></label>
        </Card>
      </div>
    </Tabs.Content>

    <Tabs.Content value="streaming">
      <div class="col">
        <Card cols={2}>
          <h4>Default quality</h4>
          <div class="row chips" role="group" aria-label="Default quality" bind:this={quality}>
            {#each PRESETS as p (p)}
              <button class="fchip" aria-pressed={$settings.defaultPreset === p} onclick={() => glideSwap(quality, '[aria-pressed="true"]', () => set({ defaultPreset: p }))}>{p}</button>
            {/each}
          </div>
        </Card>
        <Card cols={2}>
          <h4>Replay buffer</h4>
          <label class="opt"><span>Keep the last <small class="lab">for Save replay, Ctrl+Alt+R</small></span>
            <select value={$settings.replaySeconds} onchange={(e) => set({ replaySeconds: Number(e.currentTarget.value) as ReplaySeconds })}>
              {#each REPLAY_SECONDS as n (n)}<option value={n}>{n ? `${n} s` : 'Off'}</option>{/each}
            </select>
          </label>
          <p class="note">Held in this browser's memory only (at most 160 MB), dropped when the stream disconnects or changes quality. While on, the page asks the host for a key frame every 10 s. Applies from the next connect. Ctrl+Alt+S takes a screenshot.</p>
        </Card>
        <Card cols={2}>
          <h4>Clipboard</h4>
          <label class="opt"><span>Clipboard sync <small class="lab">text and images, while streaming</small></span><input type="checkbox" checked={$settings.clipboardSync} onchange={(e) => set({ clipboardSync: e.currentTarget.checked })} /></label>
          <p class="note">On: every copy on this laptop goes to the host's clipboard while the stream has focus, passwords from a password manager included.</p>
        </Card>
      </div>
    </Tabs.Content>

    <Tabs.Content value="controller">
      {#if tab === 'controller'}<ControllerTest />{/if}
    </Tabs.Content>

    <Tabs.Content value="about">
      <div class="col">
        <Card cols={2}>
          <h4>Brisk</h4>
          <dl class="kv"><dt>Commit</dt><dd>{commit}</dd><dt>Host</dt><dd>{$host?.name ?? '–'}</dd><dt>Sunshine</dt><dd>{$host?.sunshine ?? '–'}</dd></dl>
          <h4 class="gap">Docs</h4>
          <ul class="plain">{#each DOCS as d (d)}<li><code>{d}</code></li>{/each}</ul>
        </Card>
        <Card cols={2}>
          <h4>Keyboard shortcuts</h4>
          <dl class="kv">{#each SHORTCUTS as [k, d] (k)}<dt><kbd>{k}</kbd></dt><dd>{d}</dd>{/each}</dl>
        </Card>
      </div>
    </Tabs.Content>
  </Tabs.Root>
</div>

<style>
  .top { padding: 0 2px 10px; }
  h2 { font-size: 18px; letter-spacing: -.02em; }
  .st :global(.strip) { position: sticky; top: 0; z-index: 5; display: flex; gap: 4px; overflow-x: auto; overscroll-behavior-x: contain; scrollbar-width: none; background: var(--bg); border-bottom: 1px solid var(--line); }
  .st :global(.strip::-webkit-scrollbar) { display: none; }
  .st :global(.tab) { flex: none; padding: 10px 14px; border: 0; background: none; color: var(--muted); font: inherit; font-size: 14px; cursor: pointer; border-radius: 10px 10px 0 0; transition: color .2s; }
  .st :global(.tab:hover), .st :global(.tab[aria-selected='true']) { color: var(--fg); }
  .st :global(.tab:focus-visible) { outline: 2px solid var(--accent); outline-offset: -2px; }
  .live :global([role='tabpanel']) { animation: rise-in var(--dur-tab) var(--ease) backwards; }
  @media (prefers-reduced-motion: reduce) { .live :global([role='tabpanel']) { animation: none; } }
  .ind { position: absolute; left: 0; bottom: 0; height: 2px; border-radius: 2px; background: var(--accent); transition: transform .25s var(--ease), width .25s var(--ease); }
  @media (prefers-reduced-motion: reduce) { .ind, .st :global(.tab) { transition: none; } }
  .col { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 14px; padding-top: 16px; }
  .row { display: flex; gap: 10px; margin-top: 14px; flex-wrap: wrap; }
  .sw { width: 30px; height: 30px; border-radius: 50%; border: 2px solid transparent; cursor: pointer; }
  .sw[aria-checked='true'] { outline: 2px solid var(--fg); outline-offset: 2px; }
  .opt { display: flex; justify-content: space-between; align-items: center; margin-top: 14px; }
  .note { margin: 10px 0 0; font-size: 12px; color: var(--muted); }
  .opt select { background: var(--panel2); color: var(--fg); border: 1px solid var(--line); border-radius: 9px; padding: 5px 8px; font: inherit; font-size: 13px; }
  .opt input { accent-color: var(--accent); width: 18px; height: 18px; }
  .chips { isolation: isolate; } /* the gliding fill passes under the other chips */
  .fchip { position: relative; padding: 5px 14px; border-radius: 999px; border: 1px solid var(--line); background: transparent; color: var(--muted); cursor: pointer; font-size: 13px; text-transform: capitalize; transition: color .2s var(--ease); }
  .fchip[aria-pressed='true'] { border-color: transparent; color: var(--fg); }
  .fchip[aria-pressed='true']::before { content: ''; position: absolute; inset: -1px; z-index: -1; border-radius: inherit; background: color-mix(in srgb, var(--accent) 20%, transparent); border: 1px solid var(--accent); transform-origin: 0 0; }
  .kv { display: grid; grid-template-columns: auto 1fr; gap: 8px 16px; margin: 12px 0 0; font-size: 13px; }
  .kv dt { color: var(--muted); } .kv dd { margin: 0; }
  kbd { font-family: var(--mono); font-size: 12px; padding: 2px 7px; border: 1px solid var(--line); border-radius: 7px; background: var(--panel2); }
  .plain { list-style: none; margin: 10px 0 0; padding: 0; font-size: 13px; display: grid; gap: 4px; }
  code { font-family: var(--mono); font-size: 12px; color: var(--muted); }
  .gap { margin-top: 18px; }
</style>
