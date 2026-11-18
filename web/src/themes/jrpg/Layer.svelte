<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));

  // Short tips per page. One is typed out in a dialogue box after each page change, then the box closes.
  const TIPS: Record<string, string[]> = {
    '/': ['Your host is resting. Its bars are full.', 'Drag a card by its handle to move it.', 'Press Ctrl+K to open the command menu.'],
    '/library': ['A new game appears. It looks ready to play.', 'Press Ctrl+K and type a name to launch it fast.', 'Tags can sort your games into parties.'],
    '/machines': ['These machines will follow you anywhere.', 'A sleeping host can be woken from here.'],
    '/history': ['Your past adventures are logged here.', 'Open a session to see its frame rate.'],
    '/settings': ['Press Alt+U to try another style.', 'Pick a gold or a green. Both are lucky.'],
  };
  const FALL = ['Press Ctrl+K to open the command menu.'];
  const tipsFor = (p: string) => TIPS[p.replace(/(.)\/$/, '$1')] ?? FALL;

  let shown = $state('');
  let open = $state(false);
  let done = $state(false);
  let seen = 0;
  let n = 0;
  $effect(() => {
    const p = path;
    if (stream) {
      open = false;
      return;
    }
    const list = tipsFor(p);
    const text = list[n++ % list.length];
    const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
    shown = '';
    done = false;
    open = true;
    let i = 0;
    let t: ReturnType<typeof setTimeout>;
    const close = () => (t = setTimeout(() => (open = false), 4200));
    const type = () => {
      i++;
      shown = text.slice(0, i);
      if (i < text.length) t = setTimeout(type, 34);
      else {
        done = true;
        close();
      }
    };
    if (reduce) {
      shown = text;
      done = true;
      close();
    } else t = setTimeout(type, seen++ ? 260 : 900);
    return () => clearTimeout(t);
  });
</script>

{#if open && !stream}
  <div class="jr-dlg" aria-hidden="true">
    <span class="jr-who">Brisk</span>
    <p>{shown}{#if done}<i class="jr-more">&#9660;</i>{/if}</p>
  </div>
{/if}

<style>
  .jr-dlg {
    position: fixed;
    z-index: 30;
    left: 270px;
    right: 40px;
    bottom: 22px;
    max-width: 620px;
    margin: 0 auto;
    min-height: 74px;
    padding: 16px 22px 14px;
    box-sizing: border-box;
    pointer-events: none;
    background: linear-gradient(180deg, #2a47c4, #0a1352);
    border: 5px double #fff;
    border-radius: 12px;
    box-shadow:
      0 0 0 2px #02061c,
      0 5px 0 2px #02061c,
      0 10px 22px #000a;
    color: #fff;
    text-shadow: 2px 2px 0 #02061c;
    font-size: 17px;
    line-height: 1.5;
  }
  .jr-who {
    position: absolute;
    left: 16px;
    top: -17px;
    padding: 0 10px;
    background: #0a1352;
    border: 3px double #fff;
    border-radius: 6px;
    color: #ffd24a;
    font-size: 14px;
  }
  p {
    margin: 4px 0 0;
  }
  .jr-more {
    display: inline-block;
    margin-left: 8px;
    color: #ffd24a;
    font-style: normal;
    font-size: 13px;
  }
  @media (prefers-reduced-motion: no-preference) {
    .jr-dlg {
      animation: jr-pop 0.22s steps(5, end) backwards;
    }
    .jr-more {
      animation: jr-blink 0.8s steps(2, end) infinite;
    }
  }
  @keyframes jr-pop {
    from {
      clip-path: inset(50% 0 50% 0);
    }
  }
  @keyframes jr-blink {
    50% {
      opacity: 0;
    }
  }
  @media (max-width: 800px) {
    .jr-dlg {
      display: none; /* it would cover page controls on a phone */
    }
  }
</style>
