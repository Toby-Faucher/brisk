<script lang="ts">
  import { onMount } from 'svelte';
  import { host } from '../../state/host';
  import { hub, selectedMachine } from '../../state/machines';
  import { loadModules } from '../../shell/registry';

  let { path }: { path: string } = $props();
  // Off on /stream: the picture there is the product, no overlay may touch it.
  const off = $derived(path === '/stream' || path.startsWith('/stream/'));

  const live = loadModules().filter((m) => !m.soon);
  const mode = $derived.by(() => {
    const i = live.findIndex((m) => (m.routes[0].path === '/' ? path === '/' : path === m.routes[0].path || path.startsWith(`${m.routes[0].path}/`)));
    return i < 0 ? null : { n: String(i + 1).padStart(2, '0'), name: live[i].title.toUpperCase() };
  });

  // One lamp per real state: green = ready, orange = streaming or waking, amber blink = off or unknown, red = fault.
  type Lamp = 'ok' | 'busy' | 'off' | 'down';
  const link = $derived.by((): { lamp: Lamp; word: string; who: string } => {
    const m = $hub === true ? $selectedMachine : null;
    if (m) {
      const lamp: Lamp = m.state === 'ready' ? 'ok' : m.state === 'unreachable' ? 'down' : m.state === 'off' ? 'off' : 'busy';
      return { lamp, word: { ready: 'READY', off: 'POWER OFF', starting: 'STARTING', stopping: 'STOPPING', unreachable: 'NO ANSWER' }[m.state], who: m.name };
    }
    if (!$host) return { lamp: 'off', word: 'NO LINK', who: '--' };
    return { lamp: $host.sunshine === 'ready' ? 'ok' : $host.sunshine === 'busy' ? 'busy' : 'down', word: { ready: 'READY', busy: 'ON AIR', down: 'FAULT' }[$host.sunshine], who: $host.name };
  });

  // Mission clock: seconds since this page opened. One text node per second, nothing else.
  let secs = $state(0);
  const clock = $derived(`${String(Math.floor(secs / 3600)).padStart(2, '0')}:${String(Math.floor(secs / 60) % 60).padStart(2, '0')}:${String(secs % 60).padStart(2, '0')}`);
  let boot = $state(false);
  onMount(() => {
    const t = setInterval(() => secs++, 1000);
    // The boot screen shows once per tab session, never with reduced motion.
    try {
      if (!matchMedia('(prefers-reduced-motion: reduce)').matches && !sessionStorage.getItem('brisk.mecha.boot')) {
        sessionStorage.setItem('brisk.mecha.boot', '1');
        boot = true;
        setTimeout(() => (boot = false), 1900);
      }
    } catch {
      /* storage blocked: no boot screen */
    }
    return () => clearInterval(t);
  });
</script>

{#if !off}
  <div class="hud" aria-hidden="true">
    <span class="seg mode"><b>{mode ? `MODE ${mode.n}` : 'MODE --'}</b> {mode?.name ?? ''}</span>
    <span class="rail"></span>
    <span class="seg"><i class="lamp {link.lamp}"></i>{link.who} <b>{link.word}</b></span>
    <span class="seg mono">T+{clock}</span>
  </div>
  <div class="corner tl"></div>
  <div class="corner tr"></div>
  <div class="corner bl"></div>
  <div class="corner br"></div>
  <div class="radar">
    <div class="sweep"></div>
    <i class="b1"></i><i class="b2"></i><i class="b3"></i>
    <span class="cap">SCAN</span>
  </div>
  {#key path}
    <div class="scan"></div>
  {/key}
  {#if boot}
    <div class="boot">
      <div class="shutter top"></div>
      <div class="shutter bot"></div>
      <pre class="log">BRISK COCKPIT OS  REV 7
POWER ............ OK
HOST LINK ........ {link.lamp === 'ok' ? 'OK' : 'STANDBY'}
COVER ARRAYS ..... OK
PILOT ............ SEATED
&gt; ALL SYSTEMS NOMINAL</pre>
    </div>
  {/if}
{/if}

<style>
  .hud,
  .corner,
  .radar,
  .scan,
  .boot {
    position: fixed;
    pointer-events: none;
  }
  .hud {
    z-index: 25;
    top: 0;
    right: 0;
    left: var(--mc-side, 236px);
    height: var(--mc-hud, 28px);
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 0 18px 0 16px;
    background: #08090a;
    border-bottom: 1px solid var(--line);
    font-family: var(--mono);
    font-size: 12px;
    letter-spacing: 0.14em;
    color: var(--mc-ice);
    white-space: nowrap;
    overflow: hidden;
  }
  .hud::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 3px;
    background: repeating-linear-gradient(-45deg, var(--mc-orange) 0 6px, transparent 6px 12px);
    opacity: 0.55;
  }
  .hud b {
    font-weight: 400;
    color: var(--accent);
  }
  .seg {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .mode {
    min-width: 168px;
  }
  .rail {
    flex: 1;
    height: 10px;
    background:
      repeating-linear-gradient(90deg, var(--line-hover) 0 1px, transparent 1px 8px) 0 100% / 100% 5px no-repeat,
      repeating-linear-gradient(90deg, var(--mc-ice) 0 1px, transparent 1px 40px) 0 100% / 100% 10px no-repeat;
    opacity: 0.6;
  }
  .lamp {
    width: 9px;
    height: 9px;
    background: var(--ok);
    box-shadow: 0 0 7px var(--ok);
    clip-path: polygon(2px 0, 100% 0, 100% calc(100% - 2px), calc(100% - 2px) 100%, 0 100%, 0 2px);
  }
  .lamp.busy {
    background: var(--mc-orange);
    box-shadow: 0 0 7px var(--mc-orange);
    animation: blink 0.7s steps(1) infinite;
  }
  .lamp.off {
    background: var(--warn);
    box-shadow: 0 0 7px var(--warn);
    animation: blink 1.2s steps(1) infinite;
  }
  .lamp.down {
    background: var(--bad);
    box-shadow: 0 0 7px var(--bad);
    animation: blink 0.45s steps(1) infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.15;
    }
  }

  /* four corner marks on the viewport */
  .corner {
    z-index: 24;
    width: 18px;
    height: 18px;
    border: 0 solid var(--mc-orange);
    opacity: 0.85;
  }
  .tl {
    left: calc(var(--mc-side, 236px) + 6px);
    top: calc(var(--mc-hud, 28px) + 6px);
    border-width: 2px 0 0 2px;
  }
  .tr {
    right: 6px;
    top: calc(var(--mc-hud, 28px) + 6px);
    border-width: 2px 2px 0 0;
  }
  .bl {
    left: calc(var(--mc-side, 236px) + 6px);
    bottom: 6px;
    border-width: 0 0 2px 2px;
  }
  .br {
    right: 6px;
    bottom: 6px;
    border-width: 0 2px 2px 0;
  }

  /* radar above the host plate; only when the window is tall enough to have room */
  .radar {
    z-index: 3;
    left: 30px;
    bottom: 92px;
    width: 110px;
    height: 110px;
    border-radius: 50%;
    border: 1px solid var(--line-hover);
    background:
      radial-gradient(circle, transparent 0 30%, color-mix(in srgb, var(--mc-ice) 35%, transparent) 30% calc(30% + 1px), transparent calc(30% + 1px) 62%, color-mix(in srgb, var(--mc-ice) 35%, transparent) 62% calc(62% + 1px), transparent calc(62% + 1px)),
      linear-gradient(90deg, transparent calc(50% - 0.5px), color-mix(in srgb, var(--mc-ice) 30%, transparent) 0 calc(50% + 0.5px), transparent 0),
      linear-gradient(0deg, transparent calc(50% - 0.5px), color-mix(in srgb, var(--mc-ice) 30%, transparent) 0 calc(50% + 0.5px), transparent 0);
    opacity: 0.85;
    display: none;
  }
  @media (min-height: 880px) {
    .radar {
      display: block;
    }
  }
  .sweep {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: conic-gradient(from 0deg, color-mix(in srgb, var(--accent) 55%, transparent), transparent 22%);
    animation: spin 4s linear infinite;
  }
  .radar i {
    position: absolute;
    width: 5px;
    height: 5px;
    background: var(--ok);
    box-shadow: 0 0 6px var(--ok);
    opacity: 0.2;
    animation: ping 4s steps(1) infinite;
  }
  .b1 {
    left: 68%;
    top: 24%;
    animation-delay: 0.5s;
  }
  .b2 {
    left: 30%;
    top: 62%;
    animation-delay: 2.4s;
  }
  .b3 {
    left: 74%;
    top: 70%;
    animation-delay: 3.3s;
  }
  .cap {
    position: absolute;
    left: 50%;
    top: calc(100% + 4px);
    translate: -50% 0;
    font-family: var(--mono);
    font-size: 10px;
    letter-spacing: 0.3em;
    color: var(--dim);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes ping {
    0% {
      opacity: 1;
    }
    25% {
      opacity: 0.2;
    }
  }

  /* a scan line sweeps down once per page change */
  .scan {
    z-index: 26;
    left: var(--mc-side, 236px);
    right: 0;
    top: 0;
    height: 56px;
    background: linear-gradient(to bottom, transparent, color-mix(in srgb, var(--accent) 22%, transparent) 85%, var(--accent) 98%, transparent 100%);
    opacity: 0;
    translate: 0 -60px;
    animation: sweep 0.75s steps(14, end) 1;
  }
  @keyframes sweep {
    0% {
      opacity: 1;
      translate: 0 -60px;
    }
    90% {
      opacity: 1;
    }
    100% {
      opacity: 0;
      translate: 0 100vh;
    }
  }

  /* boot screen: two shutters clack open */
  .boot {
    inset: 0;
    z-index: 90;
    animation: boot-out 1.9s steps(1) forwards;
  }
  .shutter {
    position: absolute;
    left: 0;
    right: 0;
    height: 50%;
    background: #060707;
  }
  .shutter.top {
    top: 0;
    border-bottom: 2px solid var(--mc-orange);
    animation: up 0.35s steps(5, end) 1.45s forwards;
  }
  .shutter.bot {
    bottom: 0;
    border-top: 2px solid var(--mc-orange);
    animation: down 0.35s steps(5, end) 1.45s forwards;
  }
  .log {
    position: absolute;
    left: 50%;
    top: 50%;
    translate: -50% -50%;
    margin: 0;
    z-index: 2;
    font-family: var(--mono);
    font-size: 15px;
    line-height: 1.7;
    letter-spacing: 0.1em;
    color: var(--mc-ice);
    clip-path: inset(0 0 0 0);
    animation: type 1.2s steps(14, end) both, fade 0.15s steps(1) 1.4s forwards;
  }
  @keyframes type {
    from {
      clip-path: inset(0 0 100% 0);
    }
  }
  @keyframes fade {
    to {
      opacity: 0;
    }
  }
  @keyframes up {
    to {
      translate: 0 -100%;
    }
  }
  @keyframes down {
    to {
      translate: 0 100%;
    }
  }
  @keyframes boot-out {
    99% {
      visibility: visible;
    }
    100% {
      visibility: hidden;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .sweep,
    .radar i,
    .scan,
    .lamp {
      animation: none;
    }
    .scan {
      display: none;
    }
  }
</style>
