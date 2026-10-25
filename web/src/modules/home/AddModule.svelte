<script lang="ts">
  import { toast } from '../../shell/Toasts.svelte';
  import Icon from '../../ui/Icon.svelte';
</script>

<button class="ghostcard" onclick={() => toast('Drop a folder with a module.ts into web/src/modules', { ms: 5000 })}>
  <Icon name="plus" size={20} />
  <span>Add a module</span>
</button>

<style>
  .ghostcard { grid-column: span 2; border: 1px dashed var(--line); border-radius: var(--r-card); background: transparent; color: var(--muted); display: grid; place-content: center; justify-items: center; gap: 4px; cursor: pointer; transition: transform .25s var(--ease), border-color .25s, color .25s; }
  .ghostcard:hover { transform: translateY(-3px); border-color: #3a4152; color: var(--fg); }
  @media (prefers-reduced-motion: reduce) { .ghostcard:hover { transform: none; } }
</style>
