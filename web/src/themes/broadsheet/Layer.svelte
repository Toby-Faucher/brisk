<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // The weather box: a new forecast each time the page changes.
  const FORECAST = [
    ['Fair', 'Frames steady at sixty. Lag: none to speak of.'],
    ['Clearing', 'Early lag will lift by noon. Light packet loss in the east.'],
    ['Bright', 'Bitrate high, spirits higher. No stutter expected.'],
    ['Mild', 'A gentle breeze off the wire. Good weather for a long session.'],
    ['Settled', 'Pressure steady. Controllers dry. No rain on the stream.'],
    ['Cool', 'Fans quiet. A fine day to leave the game running.'],
  ];
  const HEADS = ['Late City Edition', 'Special Edition', 'Third Edition', 'Final Extra', 'Sunday Supplement', 'Evening Edition'];
  const STOP = [
    'Stop press: your game is still where you left it',
    'Local library found safe and well',
    'Frames reported in good order',
    'Controller declared fit for duty',
    'Host says it is not too hot, thank you',
    'Latest: one more level, then bed',
  ];
  let n = $state(0);
  $effect(() => {
    void path;
    n = Math.floor(Math.random() * 1000);
  });
  const fc = $derived(FORECAST[n % FORECAST.length]);
  const edition = $derived(HEADS[n % HEADS.length]);
  const stop = $derived(STOP[(n >> 3) % STOP.length]);
  const temp = $derived(58 + (n % 9));

  // The dateline and issue number are CSS content read from custom properties on <html>.
  onMount(() => {
    const root = document.documentElement;
    const d = new Date();
    const day = Math.floor((d.getTime() - Date.UTC(2026, 0, 1)) / 864e5);
    root.style.setProperty('--bs-date', JSON.stringify(d.toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' })));
    root.style.setProperty('--bs-no', JSON.stringify(String(1000 + day)));
    return () => {
      root.style.removeProperty('--bs-date');
      root.style.removeProperty('--bs-no');
    };
  });
</script>

{#if !stream}
  <div class="bs-ears" aria-hidden="true">
    <div class="bs-wx">
      <b>Weather</b>
      <span class="t">{fc[0]}, {temp}&deg;</span>
      <span>{fc[1]}</span>
    </div>
    <div class="bs-ed">
      <b>{edition}</b>
      <span>{stop}</span>
    </div>
  </div>
{/if}

<style>
  /* Scoped here: only this style mounts the Layer. The ears sit in the masthead's top corners. */
  .bs-ears { position: absolute; z-index: 21; top: 24px; left: 50%; width: min(1240px, 100%); translate: -50% 0; padding: 0 24px; box-sizing: border-box; display: flex; justify-content: space-between; pointer-events: none; font: 400 12px/1.3 'Old Standard TT', Georgia, serif; color: #1a150d; }
  .bs-wx, .bs-ed { width: 190px; border: 1px solid #1a150d; padding: 5px 8px 6px; text-align: center; background: rgb(243 233 207 / .55); }
  .bs-wx { border-bottom-width: 3px; }
  .bs-wx b, .bs-ed b { display: block; font: 700 10px 'Playfair Display', Georgia, serif; letter-spacing: .2em; text-transform: uppercase; border-bottom: 1px solid #1a150d; margin-bottom: 3px; padding-bottom: 2px; }
  .bs-wx .t { display: block; font: 900 17px 'Playfair Display', Georgia, serif; }
  .bs-ed b { font-size: 12px; letter-spacing: .12em; }
  .bs-ed span { font-style: italic; }
  @media (max-width: 1100px) { .bs-ears { display: none; } }
</style>
