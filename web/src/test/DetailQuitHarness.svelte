<script lang="ts">
  // The Shell's order: the shared Quit confirm is mounted first, the detail panel opens later on top of the page.
  import type { Game } from '../state/api';
  import QuitDialog from '../shell/QuitDialog.svelte';
  import GameDetail from '../modules/library/GameDetail.svelte';
  let { game }: { game: Game } = $props();
  let open = $state(false);
</script>

<QuitDialog />
<button onclick={() => (open = true)}>Open detail</button>
{#if open}<GameDetail {game} busy={false} onclose={() => (open = false)} onplay={() => {}} oninstall={() => {}} />{/if}
