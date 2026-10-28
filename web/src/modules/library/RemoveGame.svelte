<script lang="ts">
  // "Remove from library" for an emulator game. It only forgets the card: the file stays on disk, and there is no
  // delete-file option. Notes, tags and playtime stay too, and come back if the game is added again.
  import { Dialog } from 'bits-ui';
  import { api, type Game } from '../../state/api';
  import { gameKey } from '../../state/gameId';
  import { refreshGames } from '../../state/games';
  import { toast } from '../../shell/Toasts.svelte';

  let { game, onremoved }: { game: Game; onremoved: () => void } = $props();
  let open = $state(false);
  let busy = $state(false);
  let failed = $state('');

  async function remove() {
    if (busy) return;
    busy = true;
    failed = '';
    try {
      await api.removeGame(gameKey(game));
    } catch (e) {
      failed = `Could not remove it: ${(e as Error).message}`;
      busy = false;
      return;
    }
    busy = false;
    open = false;
    toast(`Removed ${game.name} from the library. The file stays on disk.`);
    void refreshGames();
    onremoved();
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Trigger class="btn ghost">Remove from library</Dialog.Trigger>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim rs-dim" />
    <Dialog.Content class="rs rs-q glass">
      <Dialog.Title class="rs-t">Remove {game.name} from the library?</Dialog.Title>
      <Dialog.Description class="lab">The file stays on disk. Your notes and play time are kept, and come back if you add the game again.</Dialog.Description>
      {#if failed}<p class="rm-err" role="alert">{failed}</p>{/if}
      <div class="rs-a">
        <Dialog.Close class="btn ghost">Cancel</Dialog.Close>
        <button class="btn" aria-disabled={busy} aria-busy={busy} onclick={remove}>{busy ? 'Removing…' : 'Remove'}</button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .rm-err { color: var(--warn); margin: 0; font-size: 13px; }
</style>
