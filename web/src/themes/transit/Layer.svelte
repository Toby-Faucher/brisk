<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // The signboard reads its stops from the sidebar, so a new module shows up here by itself.
  let here = $state({ name: '', letter: '', color: '#3a424d' });
  let next = $state<{ name: string; color: string; letter: string } | null>(null);
  const stop = (el: Element | null | undefined) => {
    const name = (el?.textContent ?? '').trim();
    return { name, letter: name[0] ?? '', color: el ? getComputedStyle(el).getPropertyValue('--ic').trim() || '#3a424d' : '#3a424d' };
  };
  $effect(() => {
    void path;
    const links = [...document.querySelectorAll('.side nav a.nav')];
    const i = links.findIndex((a) => a.getAttribute('aria-current') === 'page');
    here = i < 0 ? { name: path.replace(/^\//, '').split('/')[0] || 'Home', letter: '', color: '#3a424d' } : stop(links[i]);
    next = i < 0 ? null : links[i + 1] ? stop(links[i + 1]) : null;
  });

  const NOTES = [
    'Good service on all lines',
    'Please stand clear of the doors',
    'Mind the gap between you and the couch',
    'Your games are on time',
    'Quiet carriage: no spoilers please',
    'Have your ticket ready. It is free',
    'This line is for people who like frames',
    'Next train in 0 min. It is the one you are on',
  ];
  const tape = [...NOTES, ...NOTES].map((n) => `${n}      •      `).join('');

  // Split-flap clock: each digit tile flips when it changes.
  let now = $state(new Date());
  onMount(() => {
    const t = setInterval(() => (now = new Date()), 5000);
    return () => clearInterval(t);
  });
  const hhmm = $derived(now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false }));
</script>

{#if !stream}
  <div class="tr-bar" aria-hidden="true">
    <div class="tr-here">
      <span class="tr-bul" style:--c={here.color}>{here.letter}</span>
      <b>{here.name}</b>
    </div>
    <div class="tr-next">
      <span class="tr-k">{next ? 'Next stop' : 'End of line'}</span>
      {#if next}<span class="tr-bul sm" style:--c={next.color}>{next.letter}</span><b>{next.name}</b>{:else}<b>All change</b>{/if}
    </div>
    <div class="tr-tape"><span>{tape}</span></div>
    <div class="tr-clock">
      {#each hhmm as ch, i (i + ch)}<i class:colon={ch === ':'}>{ch}</i>{/each}
    </div>
  </div>
{/if}

<style>
  .tr-bar { position: fixed; z-index: 25; top: 0; left: var(--side-w, 244px); right: 0; height: 48px; display: flex; align-items: center; gap: 22px; padding: 0 16px; pointer-events: none; background: #05070a; border-bottom: 4px solid var(--accent, #5eead4); font-family: 'Overpass', 'Helvetica Neue', Arial, sans-serif; color: #fff; }
  .tr-here, .tr-next { display: flex; align-items: center; gap: 10px; white-space: nowrap; }
  .tr-here b { font-size: 21px; font-weight: 800; }
  .tr-next { padding-left: 22px; border-left: 2px solid #3a424d; font-size: 15px; }
  .tr-next b { font-weight: 700; }
  .tr-k { color: #ffd23f; font-weight: 800; text-transform: uppercase; letter-spacing: .1em; font-size: 12px; }
  .tr-bul { width: 30px; height: 30px; border-radius: 50%; display: grid; place-items: center; background: var(--c); color: #fff; font-weight: 800; font-size: 17px; box-shadow: 0 0 0 2px #fff; }
  .tr-bul.sm { width: 24px; height: 24px; font-size: 13px; }
  .tr-tape { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; color: #c8d0da; font-size: 14px; font-weight: 600; mask-image: linear-gradient(90deg, transparent, #000 6%, #000 94%, transparent); }
  .tr-tape span { display: inline-block; }
  .tr-clock { display: flex; gap: 3px; }
  .tr-clock i { font-style: normal; width: 22px; height: 32px; display: grid; place-items: center; border-radius: 4px; font-weight: 800; font-size: 22px; color: #ffd23f; background: linear-gradient(180deg, #181c22 0 calc(50% - 1px), #000 calc(50% - 1px) calc(50% + 1px), #11151a calc(50% + 1px)); font-variant-numeric: tabular-nums; }
  .tr-clock i.colon { width: 10px; background: none; color: #ffd23f; }
  @media (prefers-reduced-motion: no-preference) {
    .tr-tape span { animation: tr-roll 160s linear infinite; }
    .tr-here, .tr-next { animation: tr-in .5s var(--spring-settle, ease-out) backwards; }
    .tr-next { animation-delay: .12s; }
    .tr-clock i:not(.colon) { animation: tr-flip .5s ease-out; transform-origin: 50% 50%; }
  }
  @keyframes tr-in { from { opacity: 0; translate: -24px 0; } }
  @keyframes tr-roll { to { transform: translateX(-50%); } }
  @keyframes tr-flip { 0% { transform: rotateX(90deg); } 60% { transform: rotateX(-12deg); } 100% { transform: none; } }
  @media (max-width: 900px) { .tr-next, .tr-tape { display: none; } }
</style>
