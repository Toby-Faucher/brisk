<script lang="ts">
  // Desktop furniture for the Classic Mac style: menu bar clock, disk and game icons, a trash can with opinions,
  // a window title that follows the page, and a watch cursor while the page changes. Never mounted on /stream.
  import { onMount } from 'svelte';
  import { games } from '../../state/games';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));
  const TITLES: [string, string][] = [
    ['/library', 'Library'],
    ['/machines', 'Machines'],
    ['/history', 'History'],
    ['/settings', 'Settings'],
    ['/', 'Brisk HD'],
  ];
  const title = $derived(TITLES.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p)))?.[1] ?? 'Brisk');
  $effect(() => {
    const root = document.documentElement;
    root.style.setProperty('--sm-title', `'${title}'`);
    return () => root.style.removeProperty('--sm-title');
  });

  // The watch cursor shows for a moment after every page change.
  let seen = 0;
  $effect(() => {
    void path;
    if (seen++ === 0 || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const root = document.documentElement;
    root.classList.add('sm-wait');
    const t = setTimeout(() => root.classList.remove('sm-wait'), 650);
    return () => (clearTimeout(t), root.classList.remove('sm-wait'));
  });

  let now = $state(new Date());
  onMount(() => {
    const t = setInterval(() => (now = new Date()), 15_000);
    return () => clearInterval(t);
  });
  const time = $derived(now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }));

  const recent = $derived(
    ($games?.games ?? [])
      .filter((g) => g.lastPlayed)
      .sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0))
      .slice(0, 3),
  );

  const JOKES = ['The Trash is empty.', 'Nothing to throw away.', 'Your games are safe.', 'Trash is still empty. Nice.'];
  let joke = $state('');
  let jokeN = 0;
  let wiggle = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const poke = () => {
    joke = JOKES[jokeN++ % JOKES.length];
    wiggle = false;
    requestAnimationFrame(() => (wiggle = true));
    clearTimeout(timer);
    timer = setTimeout(() => ((joke = ''), (wiggle = false)), 2600);
  };
  onMount(() => () => clearTimeout(timer));
</script>

{#if !stream}
  <time class="sm-clock" aria-hidden="true">{time}</time>
  <nav class="sm-desk" aria-label="Desktop">
    <a class="sm-ic" href="/"><i class="disk"></i><span>Brisk HD</span></a>
    {#each recent as g (g.appid)}
      <a class="sm-ic" href="/library"><i class="doc"></i><span>{g.name}</span></a>
    {/each}
  </nav>
  <div class="sm-trash">
    {#if joke}<p class="sm-say" role="status">{joke}</p>{/if}
    <button class="sm-ic" class:wiggle onclick={poke} aria-label="Trash"><i class="can"></i><span>Trash</span></button>
  </div>
{/if}
