<script lang="ts" module>
  import type { Game } from '../../state/api';
  /** Stable per-game hue for the placeholder and the hero wash. */
  export const hue = (g: Game) => (g.appid * 47) % 360;
</script>

<script lang="ts">
  import { artSrc } from '../../state/art';
  import type { Snippet } from 'svelte';

  let { game, style = '', children }: { game: Game; style?: string; children?: Snippet } = $props();
  // The disc's own system when hostd knows it (Dolphin plays both); else a name for the emulator.
  const SYSTEMS: Record<string, string> = { wii: 'Wii', gamecube: 'GameCube', dolphin: 'Wii' };
  let failed = $state(false);
  const h = $derived(hue(game));
  $effect(() => {
    void game.cover;
    failed = false; // a different game in the same slot gets another try
  });
</script>

<div class="cover" style="background:{game.emulator && !(game.cover && !failed) ? `linear-gradient(160deg,hsl(${h} 45% 26%),#12151c)` : `linear-gradient(145deg,hsl(${h} 62% 48%),hsl(${(h + 50) % 360} 55% 24%))`};{style}">
  {#if game.cover && !failed}
    <img {@attach artSrc(game.cover)} alt="" loading="lazy" decoding="async" onerror={() => (failed = true)} />
  {:else if game.emulator}
    <span class="card"><i class="badge">{SYSTEMS[game.system ?? ''] ?? SYSTEMS[game.emulator] ?? game.emulator}</i><b class="name">{game.name}</b></span>
  {:else}
    <span class="shape a"></span><span class="shape b"></span><b>{game.name.trim().charAt(0).toUpperCase() || '?'}</b>
  {/if}
  {@render children?.()}
</div>

<style>
  .cover { border-radius: 16px; position: relative; overflow: hidden; display: grid; place-items: center; color: #fff; aspect-ratio: 2 / 3; }
  .cover::before { content: ''; position: absolute; inset: 0; background: radial-gradient(90% 70% at 20% 10%, rgba(255, 255, 255, .28), transparent 60%); }
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  b { font-size: 44px; font-weight: 700; letter-spacing: -.04em; opacity: .92; text-shadow: 0 6px 24px rgba(0, 0, 0, .35); position: relative; }
  .card { position: absolute; inset: 0; display: flex; flex-direction: column; justify-content: flex-end; gap: 8px; padding: 10% 10% 12%; }
  .name { font-size: 15px; line-height: 1.2; letter-spacing: -.01em; text-align: left; overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 4; line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden; }
  .badge { align-self: flex-start; font-style: normal; font-size: 11px; font-weight: 600; padding: 2px 8px; border-radius: 999px; background: #0008; color: #fff; }
  .shape { position: absolute; border-radius: 50%; background: rgba(255, 255, 255, .16); aspect-ratio: 1; }
  .a { width: 60%; right: -18%; top: -14%; }
  .b { width: 38%; left: -10%; bottom: 6%; }
</style>
