<script lang="ts" module>
  // Module state survives the Shell remount that every page change causes, so the counters keep going.
  let health = $state(100);
  let armor = $state(0);
  let ammo = $state(50);
  let keys = $state({ blue: false, yellow: false, red: false });
  let visits = 0;
  let lastPath = '';
  const seen = new Set<string>();
  const KEY_PAGE: Record<string, 'blue' | 'yellow' | 'red'> = { '/library': 'blue', '/machines': 'yellow', '/history': 'red' };
</script>

<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  let mood = $state<'idle' | 'hurt' | 'grin'>('idle');
  let look = $state({ x: 0, y: 0 });
  let msg = $state('');
  let flashes = $state<{ id: number; x: number; y: number }[]>([]);
  let n = 0;
  const timers: ReturnType<typeof setTimeout>[] = [];
  const later = (f: () => void, ms: number) => timers.push(setTimeout(f, ms));
  const say = (s: string, ms = 2200) => {
    msg = s;
    later(() => msg === s && (msg = ''), ms);
  };
  const calm = () => later(() => (mood = 'idle'), 420);
  const reduce = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

  // One page change: the screen jolts, a little health goes, a pickup may turn up.
  $effect(() => {
    const p = path.replace(/(.)\/$/, '$1');
    if (stream) {
      lastPath = p;
      return;
    }
    if (lastPath && lastPath !== p) {
      health = Math.max(12, health - 3);
      if (!reduce())
        document.querySelector('main')?.animate(
          [
            { translate: '0 0' },
            { translate: '-7px 4px' },
            { translate: '6px -5px' },
            { translate: '-4px -3px' },
            { translate: '3px 3px' },
            { translate: '0 0' },
          ],
          { duration: 340, easing: 'steps(6, end)' },
        );
      mood = 'hurt';
      calm();
    }
    if (!seen.has(p)) {
      seen.add(p);
      armor = Math.min(100, armor + 20);
      const k = KEY_PAGE[p];
      if (k && !keys[k]) {
        keys[k] = true;
        mood = 'grin';
        say(`Got the ${k} keycard.`);
        calm();
      }
    }
    lastPath = p;
    visits++;
  });

  $effect(() => {
    if (stream) return;
    let raf = 0;
    const move = (e: PointerEvent) => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        look = { x: (e.clientX / innerWidth - 0.5) * 2, y: (e.clientY / innerHeight - 0.5) * 2 };
      });
    };
    const down = (e: PointerEvent) => {
      if (!(e.target as Element | null)?.closest('a, button, [role="button"], [role="option"], [role="tab"]')) return;
      const id = n++;
      flashes = [...flashes, { id, x: e.clientX, y: e.clientY }];
      later(() => (flashes = flashes.filter((f) => f.id !== id)), 220);
      mood = 'hurt';
      calm();
      ammo = Math.max(0, ammo - 1);
      if (ammo === 0) {
        say('Reloading.', 1000);
        later(() => {
          ammo = 50;
          health = Math.min(100, health + 25);
          mood = 'grin';
          calm();
        }, 1000);
      }
    };
    // Number keys pick a weapon slot, which is the nav link of that number. Never while typing, in a dialog or on /stream.
    const key = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || !/^[1-9]$/.test(e.key)) return;
      const t = e.target as HTMLElement | null;
      if (t?.closest('input, textarea, select, button, [contenteditable], dialog, [role="dialog"]')) return;
      document.querySelectorAll<HTMLElement>('.side a.nav')[Number(e.key) - 1]?.click();
    };
    addEventListener('pointermove', move, { passive: true });
    addEventListener('pointerdown', down, { passive: true });
    addEventListener('keydown', key);
    return () => {
      cancelAnimationFrame(raf);
      removeEventListener('pointermove', move);
      removeEventListener('pointerdown', down);
      removeEventListener('keydown', key);
    };
  });
  $effect(() => () => timers.forEach(clearTimeout));

  const pad = (v: number) => String(v).padStart(3, ' ');
</script>

{#if !stream}
  <div class="fps-bar" aria-hidden="true" data-hurt={health < 50 ? '' : undefined}>
    {#if msg}<div class="fps-msg">{msg}</div>{/if}
    <div class="fps-box fps-hp"><b>{pad(health)}<i>%</i></b><span>Health</span></div>
    <div class="fps-box fps-ar"><b>{pad(armor)}<i>%</i></b><span>Armor</span></div>
    <div class="fps-face-wrap">
      <svg class="fps-face {mood}" viewBox="0 0 64 72" style="--lx:{look.x * 2.2}px; --ly:{look.y * 1.4}px">
        <path d="M10 22 Q10 6 32 6 Q54 6 54 22 L54 46 Q54 66 32 68 Q10 66 10 46Z" fill="#c98a55" stroke="#2a1608" stroke-width="2.5" />
        <path d="M9 24 Q8 4 32 4 Q56 4 55 24 L50 24 Q46 14 32 14 Q18 14 14 24Z" fill="#3a2212" stroke="#2a1608" stroke-width="2" />
        <path class="fps-jaw" d="M12 50 Q14 64 32 66 Q50 64 52 50 Q46 60 32 60 Q18 60 12 50Z" fill="#a96d3f" />
        <g class="fps-brows" stroke="#2a1608" stroke-width="4" stroke-linecap="round">
          <path class="b1" d="M15 24 L28 28" /><path class="b2" d="M49 24 L36 28" />
        </g>
        <g class="fps-eyes">
          <rect x="16" y="29" width="13" height="8" rx="2" fill="#f4e6c8" stroke="#2a1608" stroke-width="1.5" />
          <rect x="35" y="29" width="13" height="8" rx="2" fill="#f4e6c8" stroke="#2a1608" stroke-width="1.5" />
          <rect class="pu" x="20.5" y="30.5" width="5" height="5" fill="#1b1008" style="translate: var(--lx) var(--ly)" />
          <rect class="pu" x="38.5" y="30.5" width="5" height="5" fill="#1b1008" style="translate: var(--lx) var(--ly)" />
        </g>
        <path d="M32 33 L29 46 L35 46" fill="none" stroke="#7a4a26" stroke-width="2.5" stroke-linejoin="round" />
        <g class="m-idle"><path d="M21 54 L43 54" stroke="#2a1608" stroke-width="3.5" stroke-linecap="round" /></g>
        <g class="m-hurt">
          <path d="M19 51 L45 51 L42 59 L22 59Z" fill="#1b0a06" stroke="#2a1608" stroke-width="2" stroke-linejoin="round" />
          <path d="M23 51 V56 M28 51 V57 M33 51 V57 M38 51 V56" stroke="#f4e6c8" stroke-width="2.2" />
        </g>
        <g class="m-grin">
          <path d="M18 51 Q32 62 46 51 Q32 56 18 51Z" fill="#1b0a06" stroke="#2a1608" stroke-width="2" stroke-linejoin="round" />
          <path d="M22 53 Q32 58 42 53" stroke="#f4e6c8" stroke-width="2.4" fill="none" />
        </g>
        <path class="fps-blood" d="M14 24 L19 30 M47 36 L52 46 M26 62 L30 56 M38 14 L41 20" stroke="#b01808" stroke-width="3" stroke-linecap="round" />
      </svg>
    </div>
    <div class="fps-box fps-am"><b>{pad(ammo)}</b><span>Ammo</span></div>
    <div class="fps-keys">
      {#each ['blue', 'yellow', 'red'] as const as k (k)}
        <i class="fps-key {k}" class:got={keys[k]}></i>
      {/each}
    </div>
  </div>
  {#each flashes as f (f.id)}
    <div class="fps-flash" style="left:{f.x}px; top:{f.y}px"></div>
  {/each}
{/if}

<style>
  .fps-bar {
    position: fixed;
    z-index: 25;
    left: 0;
    right: 0;
    bottom: 0;
    height: 84px;
    display: flex;
    align-items: stretch;
    justify-content: center;
    gap: 6px;
    padding: 8px 14px;
    box-sizing: border-box;
    pointer-events: none;
    background:
      radial-gradient(circle at 8px 8px, #d8b77a 0 2px, #1a0e06 3px 4px, transparent 5px),
      radial-gradient(circle at calc(100% - 8px) 8px, #d8b77a 0 2px, #1a0e06 3px 4px, transparent 5px),
      repeating-linear-gradient(90deg, #ffffff08 0 2px, transparent 2px 7px), linear-gradient(180deg, #5b4a35, #34281b 40%, #261c12);
    border-top: 4px solid #8a7550;
    box-shadow:
      0 -3px 0 #130b05,
      0 -12px 24px #000a;
    font-family: 'Teko', ui-sans-serif, sans-serif;
  }
  .fps-box {
    flex: 1 1 0;
    max-width: 280px;
    min-width: 0;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    align-items: flex-end;
    padding: 0 12px 2px;
    background: linear-gradient(180deg, #0c0604, #1d0e08);
    border: 3px solid;
    border-color: #130b05 #8a7550 #8a7550 #130b05;
    position: relative;
  }
  .fps-box b {
    font-weight: 600;
    font-size: 56px;
    line-height: 0.9;
    color: #ff3b2a;
    text-shadow: 0 0 10px #ff3b2a66, 3px 3px 0 #000;
    letter-spacing: 0.04em;
    white-space: pre;
  }
  .fps-box i {
    font-style: normal;
    font-size: 36px;
  }
  .fps-box span {
    position: absolute;
    top: 0;
    left: 6px;
    font-size: 15px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: #d8b77a;
    line-height: 1.1;
  }
  .fps-face-wrap {
    flex: 0 0 96px;
    display: grid;
    place-items: center;
    background: #0c0604;
    border: 3px solid;
    border-color: #130b05 #8a7550 #8a7550 #130b05;
  }
  .fps-face {
    height: 62px;
    width: auto;
  }
  .fps-face .m-hurt,
  .fps-face .m-grin,
  .fps-face .fps-blood {
    display: none;
  }
  .fps-face.hurt .m-idle,
  .fps-face.grin .m-idle {
    display: none;
  }
  .fps-face.hurt .m-hurt,
  .fps-face.grin .m-grin {
    display: inline;
  }
  .fps-face.hurt .b1 {
    d: path('M15 21 L28 29');
  }
  .fps-face.hurt .b2 {
    d: path('M49 21 L36 29');
  }
  .fps-face.grin .b1 {
    d: path('M15 26 L28 25');
  }
  .fps-face.grin .b2 {
    d: path('M49 26 L36 25');
  }
  [data-hurt] .fps-face .fps-blood {
    display: inline;
  }
  .fps-keys {
    flex: 0 0 84px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 6px;
    padding: 0 8px;
    background: #0c0604;
    border: 3px solid;
    border-color: #130b05 #8a7550 #8a7550 #130b05;
  }
  .fps-key {
    height: 14px;
    border: 2px solid #3a2c1c;
    background: #1a120a;
    position: relative;
  }
  .fps-key.got.blue {
    background: #3b6dff;
    border-color: #bcd0ff;
  }
  .fps-key.got.yellow {
    background: #ffd21f;
    border-color: #fff0a0;
  }
  .fps-key.got.red {
    background: #ff3b2a;
    border-color: #ffb0a8;
  }
  .fps-msg {
    position: absolute;
    left: 50%;
    bottom: calc(100% + 12px);
    translate: -50% 0;
    padding: 2px 16px 0;
    font-size: 26px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: #ffd21f;
    background: #130b05cc;
    border: 2px solid #8a7550;
    text-shadow: 2px 2px 0 #000;
    white-space: nowrap;
  }
  .fps-flash {
    position: fixed;
    z-index: 60;
    width: 90px;
    height: 90px;
    margin: -45px 0 0 -45px;
    pointer-events: none;
    background:
      radial-gradient(circle, #fff 0 12%, #ffe27a 22%, #ff8a1a 40%, transparent 62%),
      conic-gradient(from 10deg, transparent 0 8%, #ffd21f 8% 12%, transparent 12% 25%, #ffd21f 25% 29%, transparent 29% 42%, #ffd21f 42% 46%, transparent 46% 58%, #ffd21f 58% 62%, transparent 62% 75%, #ffd21f 75% 79%, transparent 79% 92%, #ffd21f 92% 96%, transparent 96%);
    mask: radial-gradient(circle, #000 55%, transparent 70%);
  }
  @media (prefers-reduced-motion: no-preference) {
    .fps-flash {
      animation: fps-boom 0.2s steps(4, end) forwards;
    }
    .fps-msg {
      animation: fps-blink 0.5s steps(2, end) 3;
    }
  }
  @keyframes fps-boom {
    from {
      scale: 0.4;
      rotate: 0deg;
    }
    to {
      scale: 1.2;
      rotate: 40deg;
      opacity: 0;
    }
  }
  @keyframes fps-blink {
    50% {
      opacity: 0.35;
    }
  }
  @media (max-width: 800px) {
    .fps-bar {
      height: 64px;
      gap: 3px;
      padding: 5px 6px;
    }
    .fps-box {
      padding: 0 4px 2px;
    }
    .fps-box b {
      font-size: 28px;
    }
    .fps-box i {
      font-size: 20px;
    }
    .fps-box span {
      font-size: 12px;
    }
    .fps-face-wrap {
      flex-basis: 56px;
    }
    .fps-face {
      height: 46px;
    }
    .fps-keys {
      flex-basis: 38px;
      padding: 0 4px;
    }
  }
</style>
