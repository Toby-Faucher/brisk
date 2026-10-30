<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  // Off on /stream: the picture there is the product, no overlay may touch it.
  const off = $derived(path === '/stream' || path.startsWith('/stream/'));

  const TIPS = [
    'PRESS CTRL+K FOR THE COMMAND MENU',
    'ALT+U FLIPS THROUGH THE UI STYLES',
    'SETTINGS > GENERAL > STYLE PICKS THE LOOK',
    'THE HOST PILL SWITCHES MACHINES',
    'PINNED TAGS GET A CARD ON HOME',
    'ESC CLOSES ANY WINDOW',
  ];
  let tip = $state(0);
  onMount(() => {
    const t = setInterval(() => (tip = (tip + 1) % TIPS.length), 5000);
    return () => clearInterval(t);
  });
</script>

{#if !off}
  <!-- Duotone + 4-level posterize for covers; referenced from theme.css as url(#crt-duo). -->
  <svg width="0" height="0" aria-hidden="true" focusable="false" style="position:absolute">
    <filter id="crt-duo" color-interpolation-filters="sRGB">
      <feColorMatrix type="matrix" values="0.33 0.55 0.12 0 0  0.33 0.55 0.12 0 0  0.33 0.55 0.12 0 0  0 0 0 1 0" />
      <feComponentTransfer>
        <feFuncR type="discrete" tableValues="0 0 0.05 0.15" />
        <feFuncG type="discrete" tableValues="0.04 0.3 0.7 1" />
        <feFuncB type="discrete" tableValues="0 0.05 0.15 0.4" />
      </feComponentTransfer>
    </filter>
  </svg>
  <div class="crt" aria-hidden="true">
    <div class="scan"></div>
    <div class="vig"></div>
    <div class="flick"></div>
  </div>
  <div class="bar" aria-hidden="true">
    <span class="coin">INSERT COIN<i></i></span>
    <span class="hint">{TIPS[tip]}</span>
    <span class="score">1UP</span>
  </div>
{/if}

<style>
  .crt,
  .crt div {
    position: fixed;
    inset: 0;
    pointer-events: none;
  }
  .crt {
    z-index: 90;
  }
  .scan {
    background: repeating-linear-gradient(to bottom, rgba(0, 0, 0, 0) 0 2px, rgba(0, 0, 0, 0.16) 2px 3px);
  }
  .vig {
    background:
      radial-gradient(ellipse at center, transparent 55%, rgba(0, 0, 0, 0.55) 100%),
      radial-gradient(ellipse at center, rgba(57, 255, 102, 0.04), transparent 70%);
    box-shadow: inset 0 0 70px rgba(0, 0, 0, 0.7);
  }
  .flick {
    background: rgba(125, 255, 148, 0.025);
    opacity: 0;
  }
  @media (prefers-reduced-motion: no-preference) {
    .flick {
      animation: flick 3.7s steps(1) infinite;
    }
    .scan {
      background-size: 100% 3px;
      animation: roll 9s linear infinite;
    }
  }
  @keyframes flick {
    0%,
    100% {
      opacity: 0.2;
    }
    7% {
      opacity: 1;
    }
    9% {
      opacity: 0.1;
    }
    52% {
      opacity: 0.6;
    }
    54% {
      opacity: 0.15;
    }
  }
  @keyframes roll {
    to {
      background-position: 0 3px;
    }
  }
  .bar {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 89;
    height: 32px;
    display: flex;
    align-items: center;
    gap: 24px;
    padding: 0 16px;
    background: #000;
    border-top: 2px solid #1fa83d;
    color: #7dff94;
    font-family: 'Press Start 2P', monospace;
    font-size: 9px;
    font-size-adjust: none;
    pointer-events: none;
    white-space: nowrap;
    overflow: hidden;
  }
  .coin {
    color: #ffc233;
    flex: none;
  }
  .coin i {
    display: inline-block;
    width: 9px;
    height: 12px;
    margin-left: 8px;
    vertical-align: -2px;
    background: #ffc233;
  }
  .hint {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .score {
    flex: none;
    color: #4fd96b;
  }
  @media (prefers-reduced-motion: no-preference) {
    .coin i {
      animation: blink 1s steps(1) infinite;
    }
    .hint {
      animation: type 5s steps(40, end) infinite;
    }
  }
  @keyframes blink {
    50% {
      opacity: 0;
    }
  }
  @keyframes type {
    0% {
      clip-path: inset(0 100% 0 0);
    }
    20%,
    100% {
      clip-path: inset(0);
    }
  }
  @media (max-width: 700px) {
    .score {
      display: none;
    }
  }
</style>
