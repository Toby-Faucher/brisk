<script lang="ts">
  // Settings > Launch: the defaults every game starts with. A game's panel can override single fields.
  import Card from '../../ui/Card.svelte';
  import DefaultsEditor from '../launch/DefaultsEditor.svelte';
</script>

<div class="lt">
  <Card cols={2} class="ltcard">
    <h4 id="lt-h">Launch options for all games</h4>
    <p class="note">Wrappers, environment variables and arguments used when a game starts on this machine. Each game can override single fields from its own panel. Changes are used the next time a game starts.</p>
    <DefaultsEditor uid="lt" />
  </Card>
</div>

<style>
  .lt { margin-top: 14px; }
  .lt :global(.ltcard) { max-width: 760px; }
  .lt :global(.ltcard:hover) { transform: none; }
  .note { margin: 8px 0 0; font-size: 12.5px; color: var(--muted); }
</style>
