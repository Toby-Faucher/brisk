<script lang="ts">
  // A page with one Quit button and the shared confirm, as GameCard / Machines use it.
  import type { Game } from '../state/api';
  import QuitDialog, { askQuit } from '../shell/QuitDialog.svelte';
  let { game }: { game: Game } = $props();
</script>

<button onclick={() => askQuit(game)}>Quit {game.name}</button>
<QuitDialog />
