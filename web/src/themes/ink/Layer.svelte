<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // The word that sits on the ink while it floods the screen, and one short line of our own.
  const WORDS: [string, string, string][] = [
    ['/library', 'Library', 'Pick a game. Paint the town.'],
    ['/machines', 'Machines', 'Who is awake out there?'],
    ['/history', 'History', 'Every round, splattered on file.'],
    ['/settings', 'Settings', 'Tweak it till it drips.'],
    ['/', 'Home', 'Your turf. Your colours.'],
  ];
  const word = $derived(WORDS.find(([p]) => (p === '/' ? path === '/' : path.startsWith(p))) ?? WORDS[4]);
</script>

{#if !stream}
  <div class="ik-bg" aria-hidden="true">
    <i class="b b1"></i>
    <i class="b b2"></i>
    <i class="b b3"></i>
    <i class="d d1"></i>
    <i class="d d2"></i>
    <i class="d d3"></i>
  </div>
  <div class="ik-word" aria-hidden="true">
    <b>{word[1]}</b>
    <span>{word[2]}</span>
  </div>
{/if}

<style>
  .ik-bg { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: #140a24; }
  .ik-bg i { position: absolute; display: block; }
  /* big soft puddles in the corners, each one wobbling on its own beat */
  .b { border-radius: 58% 42% 61% 39% / 44% 57% 43% 56%; }
  .b1 { right: -12vw; top: -14vh; width: 52vw; height: 52vh; background: #ff6a13; opacity: 0.2; }
  .b2 { left: 18vw; bottom: -20vh; width: 56vw; height: 40vh; background: #2dff8f; opacity: 0.14; }
  .b3 { right: 8vw; bottom: 8vh; width: 24vw; height: 30vh; background: #a24bff; opacity: 0.22; }
  .d { border-radius: 50%; }
  .d1 { left: 40vw; top: 12vh; width: 26px; height: 26px; background: #ff6a13; opacity: 0.55; }
  .d2 { right: 30vw; top: 46vh; width: 14px; height: 14px; background: #2dff8f; opacity: 0.55; }
  .d3 { left: 8vw; bottom: 22vh; width: 20px; height: 20px; background: #a24bff; opacity: 0.7; }

  .ik-word {
    position: fixed; z-index: 55; left: 50%; top: 44%; pointer-events: none; display: grid; gap: 6px; justify-items: center;
    color: #140a24; opacity: 0; translate: -50% -50%; text-align: center;
  }
  .ik-word b { font: 400 clamp(52px, 9vw, 120px)/1 'Lilita One', 'Fredoka Variable', sans-serif; letter-spacing: 0.02em; text-transform: uppercase; text-shadow: 4px 5px 0 #2dff8f; }
  .ik-word span { font: 600 18px/1.2 'Fredoka Variable', sans-serif; background: #140a24; color: #fff7ee; padding: 5px 16px; border-radius: 99px; rotate: -2deg; }
  @media (prefers-reduced-motion: no-preference) {
    .ik-word { animation: ik-word 0.6s 0.08s both; }
    .b1 { animation: ik-blob 11s ease-in-out infinite alternate; }
    .b2 { animation: ik-blob 14s ease-in-out infinite alternate-reverse; }
    .b3 { animation: ik-blob 9s ease-in-out infinite alternate; }
    .d { animation: ik-bob 5s ease-in-out infinite alternate; }
  }
  @keyframes ik-word {
    0% { opacity: 1; scale: 0.2; rotate: -14deg; }
    30% { opacity: 1; scale: 1.12; rotate: 3deg; }
    42% { scale: 1; rotate: -2deg; }
    80% { opacity: 1; scale: 1; rotate: -2deg; }
    100% { opacity: 0; scale: 1.1; rotate: -4deg; }
  }
  @keyframes ik-blob { to { border-radius: 40% 60% 42% 58% / 58% 42% 56% 44%; translate: 3vw 2vh; rotate: 8deg; } }
  @keyframes ik-bob { to { translate: 0 -18px; } }
  @media (prefers-reduced-motion: reduce) { .ik-word { display: none; } }
</style>
