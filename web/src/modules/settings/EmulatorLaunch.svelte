<script lang="ts">
  // Settings > Emulators > Launch options. The store is global, not per emulator: this is the same editor as Settings > Launch.
  import Card from '../../ui/Card.svelte';
  import DefaultsEditor from '../launch/DefaultsEditor.svelte';

  let _props: { focusKey?: number } = $props(); // the Emulators tab passes it to every section
</script>

<div class="el">
  <Card cols={2} class="elcard">
    <h4 id="el-h">Launch options</h4>
    <p class="note">These options are shared by every game on this machine, emulator games included. They are the same as Settings, Launch (<a class="link" href="#launch">open</a>). A game can override them from its own panel.</p>
    <DefaultsEditor uid="el" />
  </Card>
</div>

<style>
  .el { margin-top: 14px; }
  .el :global(.elcard) { max-width: 760px; }
  .el :global(.elcard:hover) { transform: none; }
  .note { margin: 8px 0 0; font-size: 12.5px; color: var(--muted); }
  .link { color: var(--accent); }
</style>
