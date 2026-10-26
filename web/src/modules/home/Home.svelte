<script lang="ts">
  import { onMount, tick, type Component } from 'svelte';
  import { enter } from '../../shell/reveal';
  import { watchGames } from '../../state/games';
  import { ACCENT_HEX, settings } from '../../state/settings';
  import { loadModules } from '../../shell/registry';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import Smoke from '../../ui/Smoke.svelte';
  import HostCard from './HostCard.svelte';
  import ResumeCard from './ResumeCard.svelte';
  import QuickActions from './QuickActions.svelte';
  import RecentSessions from './RecentSessions.svelte';
  import AddModule from './AddModule.svelte';

  const mods = loadModules();
  const soon = mods.filter((m) => m.soon);
  let widgets: Component[] = $state([]);

  onMount(() => {
    Promise.all(mods.flatMap((m) => (m.widget ? [m.widget().then((w) => w.default)] : []))).then((w) => ((widgets = w), void tick().then(enter)), () => {});
    return watchGames();
  });
</script>

<div class="bg" aria-hidden="true"><Smoke accent={ACCENT_HEX[$settings.accent]} /></div>

<div class="bento">
  <HostCard />
  <ResumeCard />
  <QuickActions />
  <RecentSessions />
  {#each widgets as W}<W />{/each}
  {#each soon as m (m.id)}
    <Card cols={2} class="soon">
      <h4><Icon name={m.icon} color={m.color} size={16} /> {m.title} <span class="tag">soon</span></h4>
      <div class="lab note">Coming in a later version.</div>
    </Card>
  {/each}
  <AddModule />
</div>

<style>
  /* Faint full-window smoke behind everything (the sidebar is translucent). */
  .bg { position: fixed; inset: 0; z-index: -1; opacity: .1; pointer-events: none; }
  .bento { display: grid; grid-template-columns: repeat(6, 1fr); grid-auto-rows: 120px; grid-auto-flow: dense; gap: 12px; }
  .bento :global(.soon) { opacity: .7; }
  .note { margin-top: 8px; }
</style>
