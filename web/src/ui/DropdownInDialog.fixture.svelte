<script lang="ts">
  import { Dialog } from 'bits-ui';
  import Dropdown from './Dropdown.svelte';
  let { open = $bindable(true) }: { open?: boolean } = $props();
  let v = $state('a');
</script>

<Dialog.Root bind:open>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content class="mp glass" style="position:fixed;z-index:41;top:10vh;left:10vw"><Dialog.Title>T</Dialog.Title><Dropdown bind:value={v} options={[['a', 'Alpha'], ['b', 'Beta']]} label="Sort games" /></Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>
