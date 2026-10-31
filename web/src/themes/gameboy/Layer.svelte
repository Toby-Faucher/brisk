<script lang="ts" module>
  let booted = false; // the logo scroll plays once per page load, not on every client navigation
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));
  let boot = $state(false);
  let down = $state<string | null>(null);
  let cheat = $state(false);
  let seq = '';

  onMount(() => {
    if (booted || matchMedia('(prefers-reduced-motion: reduce)').matches || matchMedia('(max-width: 899px)').matches) return;
    booted = true;
    try {
      if (sessionStorage.getItem('brisk.gb-boot')) return;
      sessionStorage.setItem('brisk.gb-boot', '1');
    } catch {}
    boot = true;
    const t = setTimeout(() => (boot = false), 2600);
    return () => clearTimeout(t);
  });

  // The buttons send the keys the app already listens for. A synthetic key does not press a focused control, so A also clicks it.
  const KEYS: Record<string, { key: string; code?: string; ctrlKey?: boolean; altKey?: boolean }> = {
    a: { key: 'Enter', code: 'Enter' },
    b: { key: 'Escape', code: 'Escape' },
    start: { key: 'k', code: 'KeyK', ctrlKey: true },
    select: { key: 'u', code: 'KeyU', altKey: true },
    up: { key: 'ArrowUp', code: 'ArrowUp' },
    down: { key: 'ArrowDown', code: 'ArrowDown' },
    left: { key: 'ArrowLeft', code: 'ArrowLeft' },
    right: { key: 'ArrowRight', code: 'ArrowRight' },
  };
  const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]):not([type=hidden]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  // nearest focusable in the pressed direction, inside the open dialog or the screen
  function step(dir: string, from: Element | null) {
    const root = document.querySelector('[role=dialog], [role=menu]') ?? document.querySelector('.app');
    if (!root) return;
    const all = [...root.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((e) => e.offsetParent !== null && !e.closest('[hidden], [inert]'));
    if (!all.length) return;
    const cur = from && root.contains(from) && from !== root ? (from as HTMLElement).getBoundingClientRect() : null;
    if (!cur) return all[0].focus();
    const cx = cur.left + cur.width / 2, cy = cur.top + cur.height / 2;
    let best: HTMLElement | null = null, bestScore = Infinity;
    for (const e of all) {
      if (e === from) continue;
      const r = e.getBoundingClientRect();
      const dx = r.left + r.width / 2 - cx, dy = r.top + r.height / 2 - cy;
      const main = dir === 'up' ? -dy : dir === 'down' ? dy : dir === 'left' ? -dx : dx;
      if (main <= 2) continue;
      const side = dir === 'up' || dir === 'down' ? Math.abs(dx) : Math.abs(dy);
      const score = main + side * 2.5;
      if (score < bestScore) ((bestScore = score), (best = e));
    }
    best?.focus();
    best?.scrollIntoView({ block: 'nearest' });
  }

  function press(id: string) {
    const k = KEYS[id];
    const target = id === 'start' || id === 'select' ? window : (document.activeElement ?? document.body);
    const ev = new KeyboardEvent('keydown', { ...k, bubbles: true, cancelable: true });
    target.dispatchEvent(ev);
    if (ev.defaultPrevented) return;
    const el = document.activeElement as HTMLElement | null;
    const typing = !!el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName);
    if (id === 'a') {
      if (el && el !== document.body && /^(A|BUTTON|SUMMARY)$/.test(el.tagName)) el.click();
      else if (el?.getAttribute('role') === 'option' || el?.getAttribute('role') === 'menuitem') el.click();
      else step('down', null);
    } else if (id === 'b') {
      if (!document.querySelector('[role=dialog], [role=menu]') && history.length > 1) history.back();
    } else if (id in { up: 1, down: 1, left: 1, right: 1 } && !typing) step(id, el);
  }

  // a real key press lights the matching button for a moment
  const BY_KEY: Record<string, string> = { Enter: 'a', Escape: 'b', ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' };
  let off: ReturnType<typeof setTimeout>;
  const TOKEN: Record<string, string> = { ArrowUp: 'U', ArrowDown: 'D', ArrowLeft: 'L', ArrowRight: 'R', b: 'B', B: 'B', Escape: 'B', a: 'A', A: 'A', Enter: 'A' };
  function lit(e: KeyboardEvent) {
    // up up down down left right left right B A
    const t = TOKEN[e.key];
    if (t && !e.ctrlKey && !e.altKey) {
      seq = (seq + t).slice(-10);
      if (seq === 'UUDDLRLRBA' && !stream) {
        cheat = true;
        seq = '';
        setTimeout(() => (cheat = false), 2600);
      }
    }
    const id = e.ctrlKey && e.key.toLowerCase() === 'k' ? 'start' : e.altKey && e.code === 'KeyU' ? 'select' : BY_KEY[e.key];
    if (!id) return;
    down = id;
    clearTimeout(off);
    off = setTimeout(() => (down = null), 140);
  }

  const btn = (id: string) => ({ 'data-k': id, 'data-down': down === id ? '' : undefined });
</script>

<svelte:window onkeydown={lit} />

<!-- The two filters every other rule leans on. Both snap colour to #0f380f #306230 #8bac0f #9bbc0f. -->
<svg class="gb-defs" width="0" height="0" aria-hidden="true" focusable="false">
  <filter id="gb-lcd" x="0" y="0" width="100%" height="100%" color-interpolation-filters="sRGB">
    <feColorMatrix type="matrix" values="0.2126 0.7152 0.0722 0 0  0.2126 0.7152 0.0722 0 0  0.2126 0.7152 0.0722 0 0  0 0 0 1 0" />
    <feComponentTransfer>
      <feFuncR type="discrete" tableValues="0.0588 0.0588 0.0588 0.0588 0.0588 0.188 0.188 0.188 0.188 0.188 0.545 0.545 0.545 0.608 0.608 0.608 0.608 0.608 0.608 0.608" />
      <feFuncG type="discrete" tableValues="0.2196 0.2196 0.2196 0.2196 0.2196 0.384 0.384 0.384 0.384 0.384 0.6745 0.6745 0.6745 0.7373 0.7373 0.7373 0.7373 0.7373 0.7373 0.7373" />
      <feFuncB type="discrete" tableValues="0.0588 0.0588 0.0588 0.0588 0.0588 0.188 0.188 0.188 0.188 0.188 0.0588 0.0588 0.0588 0.0588 0.0588 0.0588 0.0588 0.0588 0.0588 0.0588" />
    </feComponentTransfer>
  </filter>
  <filter id="gb-cover" x="0" y="0" width="100%" height="100%" color-interpolation-filters="sRGB">
    <feColorMatrix type="matrix" values="0.2126 0.7152 0.0722 0 0  0.2126 0.7152 0.0722 0 0  0.2126 0.7152 0.0722 0 0  0 0 0 1 0" />
    <feComponentTransfer>
      <feFuncR type="gamma" amplitude="1.15" exponent="0.75" offset="0" />
      <feFuncG type="gamma" amplitude="1.15" exponent="0.75" offset="0" />
      <feFuncB type="gamma" amplitude="1.15" exponent="0.75" offset="0" />
    </feComponentTransfer>
    <feComponentTransfer>
      <feFuncR type="discrete" tableValues="0.0588 0.188 0.545 0.608" />
      <feFuncG type="discrete" tableValues="0.2196 0.384 0.6745 0.7373" />
      <feFuncB type="discrete" tableValues="0.0588 0.188 0.0588 0.0588" />
    </feComponentTransfer>
  </filter>
</svg>

{#if !stream}
  <div class="gb-shell" class:gb-boot-on={boot}>
    <div class="gb-bezel" aria-hidden="true">
      <span class="gb-led"></span><span class="gb-bat">BATTERY</span>
      <span class="gb-rule"></span>
      <span class="gb-dm">DOT MATRIX WITH STEREO SOUND</span>
      <span class="gb-rule r"></span>
    </div>
    <div class="gb-glass" aria-hidden="true"></div>
    {#if cheat}
      <div class="gb-cheat" aria-hidden="true"><b>30 LIVES</b><i>CHEAT ON</i></div>
    {/if}
    {#if boot}
      <div class="gb-boot" aria-hidden="true"><b>Brisk</b><i>TM</i></div>
    {/if}
    <div class="gb-name" aria-hidden="true"><b>Brisk</b> <i>BOY</i><sup>TM</sup></div>
    <div class="gb-grille" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i><i></i></div>

    <!-- mousedown is cancelled so a press never steals focus from the screen -->
    <div class="gb-pad" role="group" aria-label="Handheld buttons" onmousedown={(e) => e.preventDefault()}>
      <div class="gb-dpad">
        <button type="button" tabindex="-1" class="up" aria-label="D-pad up" {...btn('up')} onclick={() => press('up')}></button>
        <button type="button" tabindex="-1" class="left" aria-label="D-pad left" {...btn('left')} onclick={() => press('left')}></button>
        <button type="button" tabindex="-1" class="right" aria-label="D-pad right" {...btn('right')} onclick={() => press('right')}></button>
        <button type="button" tabindex="-1" class="down" aria-label="D-pad down" {...btn('down')} onclick={() => press('down')}></button>
      </div>
      <div class="gb-ab">
        <button type="button" tabindex="-1" class="b" aria-label="B button (Escape)" {...btn('b')} onclick={() => press('b')}></button>
        <button type="button" tabindex="-1" class="a" aria-label="A button (Enter)" {...btn('a')} onclick={() => press('a')}></button>
        <span class="lb">B</span><span class="la">A</span>
      </div>
      <div class="gb-ss">
        <button type="button" tabindex="-1" class="sel" aria-label="Select button (switch style)" {...btn('select')} onclick={() => press('select')}></button>
        <button type="button" tabindex="-1" class="sta" aria-label="Start button (command menu)" {...btn('start')} onclick={() => press('start')}></button>
        <span class="ls">SELECT</span><span class="lt">START</span>
      </div>
    </div>
  </div>
{/if}
