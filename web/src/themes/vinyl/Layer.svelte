<script lang="ts">
  // A little turntable in the free space of the side wall. Decoration only: no clicks, nothing on /stream.
  let { path }: { path: string } = $props();
  const on = $derived(!/^\/stream/.test(path));
</script>

{#if on}
  <div class="deck" aria-hidden="true">
    <div class="plinth">
      <div class="platter"><i></i></div>
      <div class="arm"></div>
      <span class="knob"></span>
    </div>
    <p>Side A. Now spinning.</p>
  </div>
{/if}

<style>
  .deck { transition: opacity .15s; position: fixed; left: 24px; bottom: 82px; width: 184px; z-index: 1; pointer-events: none; }
  .plinth { position: relative; height: 128px; border-radius: 8px; background: linear-gradient(160deg, #d9b46a, #a9803a); border: 2px solid #1d1109; box-shadow: 0 8px 12px -4px #000b, 0 1px 0 #fff6 inset; }
  .platter { position: absolute; left: 10px; top: 12px; width: 100px; height: 100px; border-radius: 50%; background: radial-gradient(circle, #efe3c4 0 3%, #0c0907 3.5% 6%, var(--accent) 6.5% 22%, #0c0907 22.5% 25%, transparent 25.5%), conic-gradient(from 20deg, #0000 0 8%, #fff3 12% 14%, #0000 20% 58%, #fff3 62% 64%, #0000 70%), repeating-radial-gradient(circle, #15100c 0 1.5px, #26211b 2.5px 3px, #15100c 4px); box-shadow: 0 0 0 4px #2b180c; }
  .arm { position: absolute; right: 22px; top: 12px; width: 5px; height: 76px; border-radius: 3px; background: linear-gradient(90deg, #cfc6b4, #fff, #8d8574); transform-origin: 50% 6px; rotate: 18deg; box-shadow: 0 4px 4px -2px #0008; }
  .arm::before { content: ''; position: absolute; top: -8px; left: -7px; width: 18px; height: 18px; border-radius: 50%; background: #2b180c; box-shadow: 0 0 0 2px #6d4527; }
  .arm::after { content: ''; position: absolute; bottom: -4px; left: -2px; width: 9px; height: 12px; background: #2b180c; border-radius: 2px; }
  .knob { position: absolute; right: 12px; bottom: 10px; width: 14px; height: 14px; border-radius: 50%; background: #2b180c; box-shadow: 0 0 0 2px #6d4527; }
  p { margin: 8px 0 0; text-align: center; font: 12px / 1.2 'Special Elite', monospace; letter-spacing: 0.06em; color: #e9d6a6; text-transform: uppercase; }
  @media (prefers-reduced-motion: no-preference) {
    .platter { animation: spin 3.6s linear infinite; }
    .arm { animation: drop 2.4s var(--spring-settle, ease-out) backwards; }
  }
  @keyframes spin { to { rotate: 360deg; } }
  @keyframes drop { from { rotate: -8deg; } }
  @media (max-height: 760px), (max-width: 900px) { .deck { display: none; } }
</style>
