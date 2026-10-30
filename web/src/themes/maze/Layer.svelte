<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));
  const calm = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

  // The score ticks up by 10 while the wedge eats. It is only decoration.
  let score = $state(120);
  let ready = $state(false);
  const fmt = (n: number) => String(n).padStart(6, '0');

  onMount(() => {
    if (calm()) return;
    const t = setInterval(() => (score += 10), 700);
    return () => clearInterval(t);
  });

  // "READY!" blinks for a moment after each page change and on first load.
  $effect(() => {
    void path;
    if (stream || calm()) return;
    ready = true;
    const t = setTimeout(() => (ready = false), 1900);
    return () => clearTimeout(t);
  });

  // Where each actor sits when motion is off, and the order they chase round the rim.
  const actors = [
    { c: 'wedge', p: 0.3 },
    { c: 'ghost red', p: 0.27 },
    { c: 'ghost pink', p: 0.245 },
    { c: 'ghost cyan', p: 0.22 },
    { c: 'ghost orange', p: 0.195 },
  ];
</script>

{#if !stream}
  <div class="mz-mask" aria-hidden="true"></div>
  <div class="mz-frame" aria-hidden="true"></div>
  <div class="mz-hud" aria-hidden="true">
    <span class="up"><b>1UP</b> {fmt(score)}</span>
    <span class="hi"><b>HIGH SCORE</b> 030000</span>
    <span class="cr">CREDIT 01</span>
  </div>
  <div class="mz-rim" aria-hidden="true">
    {#each actors as a (a.c)}
      <i class="mz-actor {a.c}" style:--d={`${a.p * 100}%`}
      style:--t={`${-a.p * 46}s`}></i>
    {/each}
  </div>
  {#if ready}<div class="mz-ready" aria-hidden="true">READY!</div>{/if}
{/if}

<style>
  /* the outer wall and the pellet track just inside it */
  /* scrolling content slides under a black rim instead of over the pellets */
  :global(:root[data-ui='maze']) .mz-mask {
    position: fixed;
    inset: 0;
    z-index: 3;
    pointer-events: none;
    border-style: solid;
    border-color: #000;
    border-width: 60px 30px 30px;
  }
  :global(:root[data-ui='maze']) .mz-frame {
    position: fixed;
    inset: 6px;
    z-index: 4;
    pointer-events: none;
    border: 1px solid #2f4bff;
    border-radius: 22px;
    box-shadow:
      0 0 0 4px #000,
      0 0 0 5.5px #2f4bff,
      0 0 16px 3px rgb(47 75 255 / 0.5) inset;
  }
  :global(:root[data-ui='maze']) .mz-frame::after {
    content: '';
    position: absolute;
    inset: 12px;
    border: 4px dotted #ffd2b0;
    border-radius: 12px;
    opacity: 0.75;
  }
  :global(:root[data-ui='maze']) .mz-hud {
    position: fixed;
    z-index: 5;
    top: 32px;
    left: 40px;
    right: 40px;
    display: flex;
    justify-content: space-between;
    pointer-events: none;
    font: 10px/1 'Press Start 2P', ui-monospace, monospace;
    color: #fff;
    text-transform: uppercase;
  }
  :global(:root[data-ui='maze']) .mz-hud b {
    font-weight: 400;
    color: #ff6b7d;
    margin-right: 8px;
  }
  :global(:root[data-ui='maze']) .mz-hud .up b {
    animation: mz-blink 1s steps(1) infinite;
  }
  :global(:root[data-ui='maze']) .mz-hud .cr {
    color: #c4cbff;
  }
  :global(:root[data-ui='maze']) .mz-rim {
    position: fixed;
    inset: 0;
    z-index: 25; /* over the page, under the stream (30), dialogs and toasts */
    pointer-events: none;
  }
  :global(:root[data-ui='maze']) .mz-actor {
    position: absolute;
    top: 0;
    left: 0;
    width: 22px;
    height: 22px;
    offset-path: inset(20px round 14px);
    offset-distance: var(--d);
    offset-rotate: 0deg;
    animation: mz-lap 46s linear infinite;
    animation-delay: var(--t);
  }
  :global(:root[data-ui='maze']) .mz-actor.wedge {
    background: #ffe600;
    border-radius: 50%;
    offset-rotate: auto;
    animation:
      mz-lap 46s linear infinite var(--t),
      mz-chomp 0.3s steps(1) infinite;
  }
  :global(:root[data-ui='maze']) .mz-actor.ghost {
    background: var(--g);
    border-radius: 11px 11px 0 0;
    clip-path: polygon(0 0, 100% 0, 100% 100%, 83% 80%, 66% 100%, 50% 80%, 33% 100%, 16% 80%, 0 100%);
  }
  :global(:root[data-ui='maze']) .mz-actor.ghost::before {
    content: '';
    position: absolute;
    left: 3px;
    right: 3px;
    top: 5px;
    height: 8px;
    background:
      radial-gradient(circle at 30% 50%, #1a1aff 0 1.6px, #fff 2px 3.6px, transparent 4px),
      radial-gradient(circle at 70% 50%, #1a1aff 0 1.6px, #fff 2px 3.6px, transparent 4px);
  }
  :global(:root[data-ui='maze']) .mz-actor.red {
    --g: #ff3b3b;
  }
  :global(:root[data-ui='maze']) .mz-actor.pink {
    --g: #ffb0ff;
  }
  :global(:root[data-ui='maze']) .mz-actor.cyan {
    --g: #36f3ff;
  }
  :global(:root[data-ui='maze']) .mz-actor.orange {
    --g: #ffb552;
  }
  :global(:root[data-ui='maze']) .mz-ready {
    position: fixed;
    z-index: 26;
    left: 50%;
    top: 50%;
    translate: -50% -50%;
    pointer-events: none;
    font: 26px/1 'Press Start 2P', ui-monospace, monospace;
    color: #ffe600;
    text-shadow: 4px 4px 0 #000;
    padding: 18px 22px;
    background: #000;
    border-radius: 14px;
    box-shadow:
      0 0 0 1.5px #2f4bff,
      0 0 0 5px #000,
      0 0 0 6.5px #2f4bff;
    animation: mz-blink 0.38s steps(1) infinite;
  }
  @keyframes mz-lap {
    to {
      offset-distance: 100%;
    }
  }
  @keyframes mz-chomp {
    0%,
    100% {
      clip-path: polygon(50% 50%, 100% 22%, 100% 0, 0 0, 0 100%, 100% 100%, 100% 78%);
    }
    50% {
      clip-path: polygon(50% 50%, 100% 48%, 100% 0, 0 0, 0 100%, 100% 100%, 100% 52%);
    }
  }
  @keyframes mz-blink {
    50% {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    :global(:root[data-ui='maze']) .mz-actor {
      animation: none;
    }
    :global(:root[data-ui='maze']) .mz-actor.wedge {
      clip-path: polygon(50% 50%, 100% 22%, 100% 0, 0 0, 0 100%, 100% 100%, 100% 78%);
    }
  }
</style>
