<script lang="ts">
  import { games } from '../../state/games';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import { count } from '../../ui/motion';
  import { DASH } from '../home/home';

  const installed = $derived($games?.games.filter((g) => g.installed).length);
</script>

<Card cols={2}>
  <a href="/library" class="fill" aria-label="Open the library"></a>
  <h4><Icon name="lib" color="var(--c-lib)" size={16} /> Library</h4>
  <div class="big"><span class="n" {@attach count(installed, String)}>{installed ?? DASH}</span> <span class="lab">installed</span></div>
  <div class="lab">{$games ? `${$games.games.length} games on this host` : 'Games are not available here'}</div>
</Card>

<style>
  .fill { position: absolute; inset: 0; }
  .n { font-variant-numeric: tabular-nums; }
  .big { font-size: 24px; font-weight: 650; letter-spacing: -.03em; margin-top: 6px; }
</style>
