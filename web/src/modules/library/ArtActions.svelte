<script lang="ts">
  // The game panel's art controls for an emulator game: find or change the art, the details line and the credit.
  import { api, ApiError, type Game } from '../../state/api';
  import { gameCaps, gameKey } from '../../state/gameId';
  import { refreshGames } from '../../state/games';
  import { toast } from '../../shell/Toasts.svelte';

  let { game }: { game: Game } = $props();
  let busy = $state(false);
  const SAY: Record<number, string> = {
    409: 'Art needs a SteamGridDB key on the machine, or it is already looking.',
    429: 'It looked a moment ago. Try again in a little while.',
  };
  const d = $derived(game.details);
  const line = $derived([d?.year, d?.publisher, d?.developer].filter(Boolean).join(' · '));

  async function find() {
    if (busy) return;
    busy = true;
    try {
      await api.artRefresh(gameKey(game));
      toast(`Looking for art for ${game.name}`);
      setTimeout(() => void refreshGames(), 5000);
    } catch (e) {
      toast((e instanceof ApiError && SAY[e.status]) || 'Could not look for art.', { error: true });
    }
    busy = false;
  }
</script>

{#if gameCaps(game).art}
  <section class="sec art" aria-label="Art and details">
    {#if line}<p class="lab">{line}</p>{/if}
    {#if game.needsMatch}<p class="lab">Brisk found several possible games. <a class="link" href="/settings#emulators/dolphin/art">Pick one in Settings</a>.</p>{/if}
    <button class="btn ghost" aria-disabled={busy} aria-busy={busy} onclick={find}>{game.cover ? 'Choose different art' : 'Find art'}</button>
    {#if game.credits?.length}
      <p class="credit">
        {#if game.credits.includes('SteamGridDB')}Art from <a class="link" href="https://www.steamgriddb.com" target="_blank" rel="noopener noreferrer">SteamGridDB</a>.{/if}
        {#if game.credits.includes('GameTDB')} Details from <a class="link" href="https://www.gametdb.com" target="_blank" rel="noopener noreferrer">GameTDB</a>.{/if}
      </p>
    {/if}
  </section>
{/if}

<style>
  /* GameSide's .sec padding is scoped to it, so repeat it here or the block touches the panel edge */
  .art { padding: 14px 22px 0; display: grid; gap: 8px; justify-items: start; }
  .lab { margin: 0; }
  .credit { margin: 0; font-size: 12px; color: var(--muted); }
</style>
