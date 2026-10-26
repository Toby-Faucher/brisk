<script lang="ts">
  import { ACCENT_HEX, settings, type Accent, type Settings } from '../../state/settings';
  import type { Preset } from '../../state/api';
  import Card from '../../ui/Card.svelte';

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
</script>

<div class="top"><h2>Settings</h2></div>
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
  <Card cols={2}>
    <h4>Clipboard</h4>
    <label class="opt"><span>Clipboard sync <small class="lab">text and images, while streaming</small></span><input type="checkbox" checked={$settings.clipboardSync} onchange={(e) => set({ clipboardSync: e.currentTarget.checked })} /></label>
    <p class="note">On: every copy on this laptop goes to the host's clipboard while the stream has focus, passwords from a password manager included.</p>
  </Card>
  <Card cols={2}>
    <h4>Default quality</h4>
    <div class="row" role="group" aria-label="Default quality">
      {#each PRESETS as p (p)}
        <button class="fchip" aria-pressed={$settings.defaultPreset === p} onclick={() => set({ defaultPreset: p })}>{p}</button>
      {/each}
    </div>
  </Card>
</div>

<style>
  .top { padding: 0 2px 14px; border-bottom: 1px solid var(--line); }
  h2 { font-size: 18px; letter-spacing: -.02em; }
  .col { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 14px; padding-top: 16px; }
  .row { display: flex; gap: 10px; margin-top: 14px; flex-wrap: wrap; }
  .sw { width: 30px; height: 30px; border-radius: 50%; border: 2px solid transparent; cursor: pointer; }
  .sw[aria-checked='true'] { outline: 2px solid var(--fg); outline-offset: 2px; }
  .opt { display: flex; justify-content: space-between; align-items: center; margin-top: 14px; }
  .note { margin: 10px 0 0; font-size: 12px; color: var(--muted); }
  .opt input { accent-color: var(--accent); width: 18px; height: 18px; }
  .fchip { padding: 5px 14px; border-radius: 999px; border: 1px solid var(--line); background: transparent; color: var(--muted); cursor: pointer; font-size: 13px; text-transform: capitalize; }
  .fchip[aria-pressed='true'] { background: color-mix(in srgb, var(--accent) 20%, transparent); border-color: var(--accent); color: var(--fg); }
</style>
