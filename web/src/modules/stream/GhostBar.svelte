<script lang="ts">
  import { DASH } from '../../ui/format';
  import { onMount } from 'svelte';
  import type { Codec, Preset, StreamReq } from '../../state/api';
  import Icon from '../../ui/Icon.svelte';
  import { hevcSupported } from '../../lib/video';
  import { EDGE_PX, HIDE_MS, advancedReq, edgeReveal, type Advanced } from './ghostbar';
  import { settings } from '../../state/settings';
  import type { Upload } from './clipboard';
  import type { PadInfo } from '../../lib/gamepad';

  let {
    host,
    rttMs,
    preset,
    codec,
    captured,
    clipDenied,
    upload,
    pads = [],
    onfiles,
    statsOn = $bindable(false),
    onvolume,
    oncapture,
    onquality,
    savedFor = null,
    onforget,
    onfullscreen,
    ondisconnect,
    replaySeconds = 0,
    onshot,
    onreplay,
    storesShots = false,
  }: {
    host: string;
    rttMs: number | null;
    preset: Preset | null;
    codec: Codec | null;
    captured: boolean;
    clipDenied: boolean;
    upload: { u: Upload; queued: number } | null;
    pads?: PadInfo[];
    onfiles: (f: File[]) => void;
    statsOn?: boolean;
    onvolume: (v: number) => void;
    oncapture: () => void;
    onquality: (r: StreamReq) => void;
    /** The game's name while its quality is saved, else null. */
    savedFor?: string | null;
    onforget: () => void | Promise<void>;
    onfullscreen: () => void;
    ondisconnect: () => void;
    /** Settings' replay buffer length; 0 = off (the button is disabled). */
    replaySeconds?: number;
    onshot: () => void;
    onreplay: () => void;
    /** The game's machine also keeps the screenshot (emulator games). */
    storesShots?: boolean;
  } = $props();

  const PRESETS: { id: Preset; label: string; mbps: number }[] = [
    { id: 'fast', label: 'Fast', mbps: 10 },
    { id: 'balanced', label: 'Balanced', mbps: 20 },
    { id: 'crisp', label: 'Crisp', mbps: 40 },
  ];

  let bar: HTMLElement;
  let visible = $state(false);
  let menu = $state<null | 'quality' | 'clip'>(null);
  let advanced = $state(false);
  let vol = $state(70);
  let muted = $state(false);
  let adv = $state<Advanced>({ res: '1920x1080', fps: 60, bitrateKbps: 20000, codec: 'h264' });
  let hevcOk = $state(false);
  let fileInput = $state<HTMLInputElement>();

  // Pointer state for edgeReveal: last y, whether it is on the bar, when it left the bar or edge.
  let y = 999;
  let hovering = false;
  let inside = false;
  let leftAt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const update = () => {
    const now = performance.now();
    const isIn = hovering || y <= EDGE_PX;
    if (inside && !isIn) leftAt = now;
    inside = isIn;
    visible = edgeReveal(y, visible, hovering, menu !== null, now - leftAt);
    clearTimeout(timer);
    if (visible && !isIn && menu === null) timer = setTimeout(update, HIDE_MS - (now - leftAt) + 5);
  };

  onMount(() => {
    void hevcSupported().then((ok) => (hevcOk = ok));
    const move = (e: MouseEvent) => {
      if (document.pointerLockElement) return; // playing: the bar stays away
      y = e.clientY;
      hovering = bar.contains(e.target as Node);
      update();
    };
    const lock = () => {
      if (!document.pointerLockElement) return;
      menu = null;
      hovering = false;
      y = 999;
      visible = false;
    };
    document.addEventListener('mousemove', move, { passive: true });
    document.addEventListener('pointerlockchange', lock);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousemove', move);
      document.removeEventListener('pointerlockchange', lock);
    };
  });

  $effect(() => onvolume(muted ? 0 : vol / 100));

  const toggle = (m: 'quality' | 'clip') => {
    menu = menu === m ? null : m;
    if (menu === 'quality') adv.codec = codec ?? 'h264'; // start from the live codec
    update();
  };
  const pick = (p: Preset) => {
    menu = null;
    update();
    if (p !== preset) onquality({ preset: p });
  };
  const applyAdvanced = () => {
    menu = null;
    update();
    onquality(advancedReq(adv));
  };
  let qualityBtn: HTMLButtonElement;
  const forget = async () => {
    await onforget();
    qualityBtn?.focus(); // the Forget button is gone now
  };
  const current = $derived(PRESETS.find((p) => p.id === preset));
  const codecName = $derived(codec === 'hevc' ? 'HEVC' : 'H.264');
</script>

<div
  class="bar glass"
  class:on={visible}
  bind:this={bar}
  role="toolbar"
  aria-label="Stream controls"
  tabindex="-1"
  onfocusin={() => ((hovering = true), update())}
  onfocusout={(e) => {
    if (!bar.contains(e.relatedTarget as Node)) (hovering = false), update();
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape' && menu) (menu = null), update();
  }}
>
  <span class="hostchip"><span class="dot"></span><b>{host}</b><span class="mono muted">{rttMs === null ? DASH : `${rttMs.toFixed(0)} ms`}</span></span>
  {#if pads.length}
    <!-- Not a live region (it would announce every replug); the tooltip text is in the DOM. -->
    <span class="padchip">
      <Icon name="pad" /><span class="tip">{pads.map((p) => `Controller: ${p.name} · slot ${p.slot + 1}`).join(' / ')}</span>
    </span>
  {/if}
  <span class="sep"></span>
  <button class="ic" aria-pressed={muted} aria-label={muted ? 'Unmute' : 'Mute'} onclick={() => (muted = !muted)} style="color:var(--c-power)">
    <Icon name={muted ? 'mute' : 'vol'} /><span class="tip">{muted ? 'Unmute' : 'Mute'}</span>
  </button>
  <input class="vol" type="range" min="0" max="100" bind:value={vol} aria-label="Volume" />
  <button class="ic" aria-pressed={captured} aria-label="Keyboard capture" onclick={oncapture} style="color:var(--c-stream)">
    <Icon name="kbd" /><span class="tip">{captured ? 'Release keyboard and mouse' : 'Capture keyboard and mouse'}</span>
  </button>
  <button class="ic" aria-expanded={menu === 'clip'} aria-label="Clipboard and files" onclick={() => toggle('clip')} style="color:var(--c-lib)">
    <Icon name="clip" /><span class="tip">Clipboard · drop files</span>
  </button>
  <button class="ic" aria-expanded={menu === 'quality'} aria-label="Quality" bind:this={qualityBtn} onclick={() => toggle('quality')} style="color:var(--c-agent)">
    <Icon name="quality" /><span class="tip">Quality: {current?.label ?? 'Custom'}</span>
  </button>
  <button class="ic" aria-label="Screenshot" onclick={onshot} style="color:var(--c-stream)">
    <Icon name="shot" /><span class="tip">Screenshot{storesShots ? ' · kept with the game' : ''} · Ctrl+Alt+S</span>
  </button>
  <button class="ic" aria-label={replaySeconds ? `Save last ${replaySeconds} s` : 'Save replay (off)'} disabled={!replaySeconds} onclick={onreplay} style="color:var(--c-lib)">
    <Icon name="replay" /><span class="tip">{replaySeconds ? `Save last ${replaySeconds} s · Ctrl+Alt+R` : 'Replay buffer is off (Settings)'}</span>
  </button>
  <button class="ic" aria-pressed={statsOn} aria-label="Stats" onclick={() => (statsOn = !statsOn)} style="color:var(--accent)">
    <Icon name="chart" /><span class="tip">Stats</span>
  </button>
  <button class="ic" aria-label="Fullscreen" onclick={onfullscreen} style="color:var(--c-set)">
    <Icon name="full" /><span class="tip">Fullscreen</span>
  </button>
  <span class="sep"></span>
  <button class="ic" aria-label="Disconnect" onclick={ondisconnect} style="color:var(--bad)">
    <Icon name="power" /><span class="tip">Disconnect</span>
  </button>

  {#if menu === 'quality'}
    <div class="menu glass" role="dialog" aria-label="Quality">
      <div class="chips">
        {#each PRESETS as p (p.id)}
          <button class="chip" aria-pressed={preset === p.id} onclick={() => pick(p.id)}>{p.label}</button>
        {/each}
        <button class="chip" aria-pressed={advanced || preset === 'custom'} aria-expanded={advanced} onclick={() => (advanced = !advanced)}>Advanced…</button>
      </div>
      <p class="mono muted line">{current ? `1080p · 60 fps · ${codecName} · ${current.mbps} Mbps` : `custom · ${codecName}`}</p>
      <!-- The live region is always rendered (empty first) so the screen reader announces the text when it appears. -->
      <p class="line muted saved">
        <span role="status" aria-live="polite">{savedFor ? `Saved for ${savedFor}` : ''}</span>
        {#if savedFor}<button class="forget" onclick={forget}>Forget for this game</button>{/if}
      </p>
      {#if advanced}
        <div class="adv">
          <label>Resolution
            <select bind:value={adv.res}>
              <option value="1280x720">1280×720</option>
              <option value="1920x1080">1920×1080</option>
              <option value="2560x1440">2560×1440</option>
            </select>
          </label>
          <label>Frame rate
            <select bind:value={adv.fps}>
              <option value={30}>30 fps</option>
              <option value={60}>60 fps</option>
              <option value={120}>120 fps</option>
            </select>
          </label>
          <label>Bitrate (kbps)
            <input type="number" min="1000" max="150000" step="1000" bind:value={adv.bitrateKbps} />
          </label>
          <label>Codec
            <select bind:value={adv.codec}>
              <option value="h264">H.264</option>
              <option value="hevc" disabled={!hevcOk}>{hevcOk ? 'HEVC' : 'HEVC (not supported on this browser)'}</option>
            </select>
          </label>
          <button class="btn sm" onclick={applyAdvanced}>Apply</button>
        </div>
      {/if}
    </div>
  {:else if menu === 'clip'}
    <div class="menu glass" role="dialog" aria-label="Clipboard and files">
      <label class="opt"><span>Clipboard sync <small class="muted">text and images, both ways</small></span>
        <input type="checkbox" checked={$settings.clipboardSync} onchange={(e) => settings.update((s) => ({ ...s, clipboardSync: e.currentTarget.checked }))} />
      </label>
      {#if $settings.clipboardSync && clipDenied}
        <p class="line warn">Allow clipboard access: click the icon left of the address, then Clipboard: Allow.</p>
      {/if}
      <p class="line muted">Files go to ~/Downloads/brisk on {host}. Drop them on the stream, or:</p>
      <button class="btn ghost sm" onclick={() => fileInput?.click()}>Send files…</button>
      <input type="file" multiple hidden bind:this={fileInput} onchange={(e) => (onfiles(Array.from(e.currentTarget.files ?? [])), (e.currentTarget.value = ''))} />
      {#if upload}
        <p class="line mono">{upload.u.name} · {upload.u.total ? Math.floor((upload.u.loaded / upload.u.total) * 100) : 100}%{upload.queued ? ` · ${upload.queued} more` : ''}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .bar { position: fixed; top: 14px; left: 50%; transform: translateX(-50%) translateY(-90px); padding: 6px 8px; display: flex; align-items: center; gap: 2px; transition: transform .3s var(--ease); z-index: 35; border-radius: 999px; }
  /* enter on the settle spring, leave faster (the transition of the state being entered applies) */
  .bar.on { transform: translateX(-50%) translateY(0); transition: transform var(--spring-settle-dur) var(--spring-settle); }
  .hostchip { display: inline-flex; align-items: center; gap: 8px; padding: 0 10px; white-space: nowrap; }
  .hostchip b { font-weight: 600; }
  .padchip { position: relative; display: grid; place-items: center; width: 30px; height: 34px; color: var(--ok); }
  .padchip:hover .tip { opacity: 1; transform: translateX(-50%); }
  .ic { width: 34px; height: 34px; border-radius: 11px; display: grid; place-items: center; border: 0; background: transparent; cursor: pointer; position: relative; transition: background .18s, transform .2s var(--ease); }
  .ic:hover { background: color-mix(in srgb, #fff 10%, transparent); transform: translateY(-1px); }
  .ic[aria-pressed='true'], .ic[aria-expanded='true'] { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  .tip { position: absolute; top: calc(100% + 8px); left: 50%; transform: translateX(-50%) translateY(-4px); background: #000c; color: #fff; font-size: 11.5px; padding: 3px 8px; border-radius: 7px; white-space: nowrap; opacity: 0; pointer-events: none; transition: all .18s var(--ease); }
  .ic:disabled { opacity: .4; cursor: default; }
  .ic:disabled:hover { background: transparent; transform: none; }
  .ic:hover .tip, .ic:focus-visible .tip { opacity: 1; transform: translateX(-50%); }
  .ic[aria-expanded='true'] .tip { opacity: 0; }
  .sep { width: 1px; height: 20px; background: color-mix(in srgb, #fff 14%, transparent); margin: 0 4px; }
  .vol { width: 90px; accent-color: var(--accent); height: 4px; }
  .menu { position: absolute; top: calc(100% + 10px); left: 50%; transform: translateX(-50%); padding: 12px; min-width: 320px; display: grid; gap: 10px; animation: drop .3s var(--ease); }
  @keyframes drop { from { opacity: 0; transform: translateX(-50%) translateY(-6px); } }
  .chips { display: flex; gap: 6px; flex-wrap: wrap; }
  .chip { display: inline-flex; padding: 5px 11px; border-radius: 999px; background: color-mix(in srgb, #fff 8%, transparent); font-size: 12.5px; border: 0; cursor: pointer; transition: background .18s; }
  .chip:hover { background: color-mix(in srgb, #fff 14%, transparent); }
  .chip[aria-pressed='true'] { background: color-mix(in srgb, var(--accent) 24%, transparent); box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 60%, transparent) inset; }
  .line { margin: 0; font-size: 12px; }
  .saved { display: flex; gap: 10px; align-items: baseline; justify-content: space-between; }
  .forget { background: none; border: 0; padding: 0; color: var(--muted); font: inherit; text-decoration: underline; cursor: pointer; }
  .forget:hover, .forget:focus-visible { color: var(--fg); }
  .warn { color: var(--warn); }
  .opt { display: flex; justify-content: space-between; align-items: center; gap: 12px; font-size: 13px; }
  .opt input { accent-color: var(--accent); width: 18px; height: 18px; }
  .adv { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 10px; align-items: end; }
  .adv label { display: grid; gap: 4px; font-size: 12px; color: var(--muted); }
  .adv select, .adv input { background: var(--panel2); color: var(--fg); border: 1px solid var(--line); border-radius: 9px; padding: 5px 8px; font: inherit; font-size: 12.5px; }
  .btn.sm { padding: 6px 12px; font-size: 12.5px; border-radius: 9px; justify-content: center; }
</style>
