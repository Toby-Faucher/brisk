<script lang="ts">
  import Icon from '../../ui/Icon.svelte';

  let { path }: { path: string } = $props();
  // Opens the existing Command menu the way a user does: the Ctrl+K keydown that shell/keys.ts listens for.
  const open = () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true }));
</script>

{#if !/^\/stream/.test(path)}
  <button type="button" class="cmd-field" onclick={open} aria-keyshortcuts="Control+K">
    <Icon name="search" size={14} />
    <span class="ph">Search games, jump to a page, run a command</span>
    <span class="ks"><kbd>Ctrl</kbd><kbd>K</kbd></span>
  </button>
{/if}
