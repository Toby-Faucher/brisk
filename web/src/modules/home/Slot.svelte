<script lang="ts">
  import { setContext, type Snippet } from 'svelte';
  import { SLOT, type Slot } from '../../ui/Card.svelte';

  // No markup of its own: the Card inside picks the id up and renders the drag handle (grid spans stay on the card).
  let { id, label, children }: Slot & { children: Snippet } = $props();
  // svelte-ignore state_referenced_locally (one slot per keyed id: the id never changes)
  setContext<Slot>(SLOT, { id, label });
</script>

{@render children()}
