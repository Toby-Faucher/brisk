<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // One line of fortune after each page change, keyed by the page. Short and plain.
  const FORTUNES: Record<string, string[]> = {
    '/': ['The cards are laid out. Pick one.', 'A quiet night. Good for a long game.'],
    '/library': ['So many cards. One of them is calling.', 'The deck is shuffled. Draw anything.'],
    '/machines': ['The tower stands. Check who is awake.', 'Every machine has a mood tonight.'],
    '/history': ['The past is a spread. Read it slowly.', 'You played more than you remember.'],
    '/settings': ['Small changes turn the wheel.', 'Adjust the stars to taste.'],
  };
  const FALLBACK = ['The stars are neutral tonight.', 'Draw a card. See what happens.'];
  let note = $state('');
  let showNote = $state(false);
  let seen = 0;
  $effect(() => {
    const p = path;
    if (stream || seen++ === 0) return; // not on /stream, not on the first load
    const list = FORTUNES[p] ?? FALLBACK;
    note = list[Math.floor(Math.random() * list.length)];
    showNote = true;
    const t = setTimeout(() => (showNote = false), 4200);
    return () => clearTimeout(t);
  });

  // Wheel geometry: 12 houses, each with a tick, a small star and a marker on the outer ring.
  const houses = Array.from({ length: 12 }, (_, i) => i * 30);
  // A fixed constellation (x, y in the 1000 box) joined with lines.
  const stars: [number, number][] = [
    [330, 380], [420, 330], [520, 360], [600, 300], [690, 360], [640, 450], [520, 470],
  ];
</script>

{#if !stream}
  <div class="tr-sky" aria-hidden="true">
    <i class="tr-dust"></i>
    <svg class="tr-wheel" viewBox="0 0 1000 1000">
      <g fill="none" stroke="#d9b45e" stroke-width="1.4">
        <circle cx="500" cy="500" r="480" />
        <circle cx="500" cy="500" r="462" stroke-dasharray="2 9" />
        <circle cx="500" cy="500" r="380" />
        <circle cx="500" cy="500" r="300" stroke-dasharray="1 6" />
        <circle cx="500" cy="500" r="190" />
        <circle cx="500" cy="500" r="60" />
        {#each houses as a}
          <line x1="500" y1="120" x2="500" y2="20" transform="rotate({a} 500 500)" />
          <path d="M500 76 l9 14 l-9 14 l-9 -14z" fill="#d9b45e" transform="rotate({a + 15} 500 500)" />
          <circle cx="500" cy="200" r="7" transform="rotate({a + 15} 500 500)" />
        {/each}
        <path d="M500 310 L690 500 L500 690 L310 500z" />
        <path d="M500 310 L630 590 L370 590z" stroke-dasharray="3 7" />
        <polyline points={stars.map((s) => s.join(',')).join(' ')} stroke-width="1" />
      </g>
      <g fill="#f4dc9a">
        {#each stars as [x, y]}<circle cx={x} cy={y} r="5" />{/each}
      </g>
    </svg>
    <i class="tr-shoot"></i>
  </div>
  <div class="tr-note" class:on={showNote} aria-hidden="true">{note}</div>
{/if}

<style>
  .tr-sky { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: radial-gradient(90% 70% at 70% 30%, #1d1552 0%, #0f0b2e 55%, #090618 100%); }
  .tr-sky i, .tr-sky svg { position: absolute; display: block; }
  .tr-dust { inset: 0; opacity: .8; background:
    radial-gradient(1.4px 1.4px at 7% 18%, #f4dc9a, transparent), radial-gradient(1px 1px at 15% 64%, #fff, transparent),
    radial-gradient(1.6px 1.6px at 24% 33%, #f4dc9a, transparent), radial-gradient(1px 1px at 33% 82%, #fff, transparent),
    radial-gradient(1.2px 1.2px at 41% 9%, #fff, transparent), radial-gradient(1px 1px at 50% 48%, #f4dc9a, transparent),
    radial-gradient(1.5px 1.5px at 58% 24%, #fff, transparent), radial-gradient(1px 1px at 66% 72%, #f4dc9a, transparent),
    radial-gradient(1.3px 1.3px at 76% 12%, #fff, transparent), radial-gradient(1px 1px at 83% 55%, #f4dc9a, transparent),
    radial-gradient(1.5px 1.5px at 91% 36%, #fff, transparent), radial-gradient(1px 1px at 96% 88%, #f4dc9a, transparent),
    radial-gradient(1px 1px at 3% 92%, #fff, transparent), radial-gradient(1.2px 1.2px at 47% 95%, #f4dc9a, transparent);
    animation: tr-twinkle 6s ease-in-out infinite alternate; }
  .tr-wheel { width: 138vmin; height: 138vmin; right: -34vmin; top: 50%; margin-top: -69vmin; opacity: .2; animation: tr-turn 360s linear infinite; }
  .tr-shoot { left: 62%; top: 20%; width: 130px; height: 1.5px; opacity: 0; background: linear-gradient(90deg, transparent, #f4dc9a); rotate: 28deg; animation: tr-shoot 41s 7s linear infinite; }
  .tr-note { position: fixed; z-index: 60; left: 50%; bottom: 22px; translate: -50% 12px; opacity: 0; pointer-events: none; padding: 9px 22px; border: 1px solid #d9b45e; border-radius: 999px; font: italic 500 18px/1.3 'Cormorant Garamond', Georgia, serif; color: #f5ecd3; background: #150f3a; box-shadow: inset 0 0 0 3px #0c0922, 0 12px 30px rgb(0 0 0 / .5); transition: opacity .8s ease, translate 1.1s cubic-bezier(.2, .8, .2, 1); }
  .tr-note.on { opacity: 1; translate: -50% 0; }

  @keyframes tr-turn { to { transform: rotate(360deg); } }
  @keyframes tr-twinkle { from { opacity: .45; } to { opacity: .9; } }
  @keyframes tr-shoot { 0%, 94% { opacity: 0; transform: translateX(0); } 95% { opacity: 1; } 100% { opacity: 0; transform: translateX(-320px) translateY(170px); } }
  @media (prefers-reduced-motion: reduce) { .tr-sky *, .tr-note { animation: none !important; transition: none !important; } .tr-shoot { display: none; } }
</style>
