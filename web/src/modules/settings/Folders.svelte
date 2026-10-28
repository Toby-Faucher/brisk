<script lang="ts">
  // Emulators.svelte passes every view a focusKey; this one has nothing to focus.
  const _props: { focusKey?: number } = $props();
  // Settings > Emulators > Dolphin > Games folders. hostd does not tell the folder yet, so it is described in words.
  import ScanGames from '../library/ScanGames.svelte';
</script>

<section class="fold" aria-label="Games folder">
  <h3>Games folder</h3>
  <p>Each machine keeps its games in the <code>Games</code> folder in its home folder. Games you add from the Library are saved there, in one folder per system, picked from the file itself. For example <code>wii</code> and <code>gamecube</code>.</p>
  <p class="lab">You can also copy game files into that folder yourself. Press Scan to add them to the Library. Removing a game from the Library never deletes its file.</p>
  <ScanGames />
</section>

<style>
  .fold { margin-top: 14px; padding: 16px; border-radius: 16px; background: var(--panel); border: 1px solid var(--line); display: grid; gap: 4px; max-width: 640px; }
  h3 { margin: 0 0 6px; font-size: 15px; }
  p { margin: 0 0 10px; }
  code { font-family: var(--mono, ui-monospace, monospace); font-size: 12.5px; }
</style>
