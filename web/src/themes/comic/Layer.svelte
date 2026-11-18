<script lang="ts">
  import { onMount } from 'svelte';

  // Page furniture (a corner caption and a dog-ear) plus the sound effects: a primary button press pops a word.
  // Never on /stream, never catches a click, nothing at all under reduced motion for the words.
  let { path }: { path: string } = $props();
  const on = $derived(!/^\/stream/.test(path));
  const CAPTIONS: [string, string][] = [
    ['/library', 'THE STORY SO FAR...'],
    ['/machines', 'MEANWHILE, ON THE HOST...'],
    ['/history', 'LAST ISSUE...'],
    ['/settings', 'THE FINE PRINT...'],
    ['/', 'MEANWHILE, AT HOME BASE...'],
  ];
  const caption = $derived(CAPTIONS.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p)))?.[1] ?? 'ISSUE #1');
  const WORDS = ['POW!', 'WHAM!', 'ZAP!', 'BAM!', 'KAPOW!', 'ZOOM!', 'BOOM!', 'THWIP!'];
  let fx: HTMLElement | undefined = $state();
  let n = 0;

  onMount(() => {
    const calm = matchMedia('(prefers-reduced-motion: reduce)');
    const hit = (e: MouseEvent) => {
      const t = e.target instanceof Element ? e.target.closest<HTMLElement>('.btn:not(.ghost)') : null;
      if (!t || calm.matches || !fx || /^\/stream/.test(location.pathname)) return;
      if ((t as HTMLButtonElement).disabled || t.getAttribute('aria-disabled') === 'true') return;
      const r = t.getBoundingClientRect();
      const w = document.createElement('span');
      w.className = 'fx-word';
      w.textContent = WORDS[n++ % WORDS.length];
      w.style.left = `${Math.min(Math.max(r.left + r.width / 2, 90), innerWidth - 90)}px`;
      w.style.top = `${Math.max(r.top - 8, 125)}px`;
      w.style.setProperty('--tilt', `${(n % 2 ? -1 : 1) * (6 + (n % 3) * 4)}deg`);
      fx.append(w);
      w.addEventListener('animationend', () => w.remove(), { once: true });
      // the page jolts 2px; the Web Animations API keeps it clear of the page-in animation on main
      document.querySelector('main')?.animate(
        [{ translate: '0 0' }, { translate: '2px -2px' }, { translate: '-2px 1px' }, { translate: '1px 2px' }, { translate: '0 0' }],
        { duration: 160 },
      );
    };
    document.addEventListener('click', hit, true);
    return () => document.removeEventListener('click', hit, true);
  });
</script>

{#if on}
  <div class="comic-cap" aria-hidden="true">{caption}</div>
  <div class="comic-fx" aria-hidden="true" bind:this={fx}><div class="comic-ear"></div></div>
{/if}
