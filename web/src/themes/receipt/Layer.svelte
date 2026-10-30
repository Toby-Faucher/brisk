<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // The header line of every receipt (CSS reads --rc-stamp and --rc-order) and the slip on the desk.
  const pad = (n: number) => String(n).padStart(2, '0');
  const stamp = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  let now = $state(new Date());
  const order = $derived(String(([...path].reduce((a, c) => a * 31 + c.charCodeAt(0), 7) % 9000) + 1000));
  $effect(() => {
    const root = document.documentElement;
    root.style.setProperty('--rc-stamp', `'${stamp(now)}'`);
    root.style.setProperty('--rc-order', `'${order}'`);
  });
  onMount(() => {
    const t = setInterval(() => (now = new Date()), 30000);
    return () => {
      clearInterval(t);
      document.documentElement.style.removeProperty('--rc-stamp');
      document.documentElement.style.removeProperty('--rc-order');
    };
  });

  // A short "printer status" line that clicks out after each page change.
  const LINES = ['PAPER OK', 'HEAD WARM', 'INK: PLENTY', 'CUTTER READY', 'NO JAMS', 'ROLL: 40M LEFT'];
  const status = $derived(LINES[order.length ? Number(order) % LINES.length : 0]);
</script>

{#if !stream}
  <div class="rc-printer" aria-hidden="true">
    <span class="led"></span>
    <b>BRISK-80 THERMAL</b>
    <em>{status}</em>
    <i class="slot"></i>
  </div>
  <div class="rc-slip" aria-hidden="true">
    <p>*** SHIFT SLIP ***</p>
    <p>{stamp(now)}</p>
    <p class="dash">- - - - - - - - - -</p>
    <p>1 x FUN .... 0.00</p>
    <p>1 x LAG .... 0.00</p>
    <p class="dash">- - - - - - - - - -</p>
    <i class="bars"></i>
    <p>THANK YOU,<br />COME AGAIN</p>
  </div>
  <i class="rc-ring"></i>
{/if}

<style>
  .rc-printer { position: fixed; z-index: 30; left: 0; right: 0; top: 0; height: 36px; pointer-events: none; display: flex; align-items: center; gap: 14px; padding: 0 22px; font: 700 11.5px/1 'Courier Prime', ui-monospace, monospace; letter-spacing: 0.14em; color: #9d98ab; background: linear-gradient(#34323b, #222127 70%, #17161a); border-bottom: 2px solid #0b0b0d; box-shadow: 0 6px 18px #000c; }
  .rc-printer em { font-style: normal; font-weight: 400; margin-left: auto; color: #8e8a9b; }
  .led { width: 8px; height: 8px; border-radius: 50%; background: #5cf08a; box-shadow: 0 0 8px #5cf08a; animation: rc-blink 3.2s steps(1) infinite; }
  .slot { position: absolute; left: 50%; bottom: -1px; width: min(940px, 92vw); height: 6px; translate: -50% 0; background: #050506; border-radius: 0 0 3px 3px; box-shadow: 0 2px 0 #4a4852; }
  .rc-slip { position: fixed; z-index: 1; right: max(10px, calc((100vw - 1140px) / 2 - 150px)); top: 130px; width: 144px; padding: 10px 4px 14px; rotate: 4deg; background: #e9e5d9; color: #625d52; font: 400 10.5px/1.35 'Courier Prime', ui-monospace, monospace; text-align: center; pointer-events: none; filter: drop-shadow(0 6px 8px #000a); clip-path: polygon(0 0, 100% 0, 100% calc(100% - 6px), 93% 100%, 86% calc(100% - 6px), 79% 100%, 72% calc(100% - 6px), 65% 100%, 58% calc(100% - 6px), 51% 100%, 44% calc(100% - 6px), 37% 100%, 30% calc(100% - 6px), 23% 100%, 16% calc(100% - 6px), 9% 100%, 0 calc(100% - 6px)); }
  .rc-slip p { margin: 0; white-space: nowrap; }
  .rc-slip .dash { letter-spacing: -0.5px; }
  .bars { display: block; height: 26px; margin: 8px 6px 6px; background: repeating-linear-gradient(90deg, #625d52 0 2px, #0000 2px 4px, #625d52 4px 5px, #0000 5px 8px, #625d52 8px 11px, #0000 11px 12px); }
  .rc-ring { position: fixed; z-index: 0; left: max(30px, calc((100vw - 1140px) / 2 - 120px)); bottom: 70px; width: 96px; height: 96px; border-radius: 50%; pointer-events: none; border: 9px solid #ffffff0d; box-shadow: inset 0 0 0 2px #ffffff08, 0 0 0 2px #ffffff06; }
  @media (max-width: 1360px) { .rc-slip, .rc-ring { display: none; } }
  @keyframes rc-blink { 0%, 90% { opacity: 1; } 91%, 100% { opacity: 0.25; } }
  @media (prefers-reduced-motion: reduce) { .led { animation: none; } }
</style>
