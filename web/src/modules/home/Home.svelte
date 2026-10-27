<script lang="ts">
  import { flushSync, onMount, tick, type Component } from 'svelte';
  import { get } from 'svelte/store';
  import { enter } from '../../shell/reveal';
  import { toast } from '../../shell/Toasts.svelte';
  import { api } from '../../state/api';
  import { watchGames } from '../../state/games';
  import { notes, notesLoaded, refreshNotes, watchNotes } from '../../state/notes';
  import { homeOrder, refreshPrefs } from '../../state/prefs';
  import { ACCENT_HEX, settings } from '../../state/settings';
  import { loadModules } from '../../shell/registry';
  import Card from '../../ui/Card.svelte';
  import Icon from '../../ui/Icon.svelte';
  import Smoke from '../../ui/Smoke.svelte';
  import HostCard from './HostCard.svelte';
  import FriendsCard from './FriendsCard.svelte';
  import ResumeCard from './ResumeCard.svelte';
  import QuickActions from './QuickActions.svelte';
  import RecentSessions from './RecentSessions.svelte';
  import AddModule from './AddModule.svelte';
  import TagCard from './TagCard.svelte';
  import Slot from './Slot.svelte';
  import { orderToStore, reconcileOrder, tagCardIds } from './home';
  import { reorder } from './drag';

  const mods = loadModules();
  const soon = mods.filter((m) => m.soon);
  const FIXED: Record<string, [string, Component]> = { host: ['Host', HostCard], resume: ['Resume', ResumeCard], quick: ['Quick actions', QuickActions], recent: ['Recent sessions', RecentSessions], friends: ['Friends', FriendsCard] };
  let widgets: Record<string, [string, Component]> = $state({});
  // The saved order (hostd prefs) laid over the cards there are; "Add a module" always stays last.
  let saved: string[] = $state([]);
  const ids = $derived(reconcileOrder(saved, [...Object.keys(FIXED), ...Object.keys(widgets), ...tagCardIds($notes.tags), ...soon.map((m) => m.id)]));
  const isTag = (id: string) => id.startsWith('tag-'); // fixed and module ids never do: 'tag-' is reserved for pinned tags
  const label = (id: string) => FIXED[id]?.[0] ?? (isTag(id) ? $notes.tags[id.slice(4)]?.name : undefined) ?? mods.find((m) => m.id === id)?.title ?? id;
  // Hidden (visibility, so the grid keeps its size) until the saved order and the widgets are in: a reorder or a card
  // arriving after first paint would be a layout shift. At most 1.5 s, then whatever is there shows.
  let ready = $state(false);
  let said = $state('');
  let puts = 0;

  onMount(() => {
    const w = Promise.all(mods.flatMap((m) => (m.widget ? [m.widget().then((x) => [m.id, [m.title, x.default]] as [string, [string, Component]])] : []))).then((w) => void (widgets = Object.fromEntries(w)), () => {});
    const show = () => {
      if (ready) return;
      saved = get(homeOrder) ?? [];
      ready = true;
      void tick().then(enter);
    };
    const off = watchNotes();
    const n = get(notesLoaded) === true ? null : refreshNotes(); // pinned tags are cards: wait for them too, so none appears after first paint
    void Promise.all([w, get(homeOrder) ? null : refreshPrefs(), n]).then(show);
    const cap = setTimeout(show, 1500);
    // enter() is idempotent: it tags what is on screen and not yet tagged, so widgets later than the cap rise too
    void Promise.all([w, n]).then(() => void (ready && tick().then(enter)));
    const offGames = watchGames();
    return () => (clearTimeout(cap), off(), offGames());
  });

  const drag = reorder({
    order: () => ids,
    set: (next) => {
      saved = next;
      flushSync();
    },
    drop: (id, next, changed) => {
      said = `${label(id)} card dropped, position ${next.indexOf(id) + 1} of ${next.length}`;
      if (!changed) return;
      const order = orderToStore(next, get(homeOrder) ?? [], get(notesLoaded) === true);
      homeOrder.set(order); // a later visit uses it even before hostd answers
      const n = ++puts; // replies can arrive out of order: only the latest one may set the store
      api.putLayout(order).then(
        (r) => n === puts && homeOrder.set(r.homeOrder),
        () => toast('Could not save the Home layout', { error: true }),
      );
    },
  });
</script>

<div class="bg" aria-hidden="true"><Smoke accent={ACCENT_HEX[$settings.accent]} /></div>

<span id="grip-hint" hidden>Space picks the card up, arrow keys move it, Space or Enter drops it, Escape puts it back.</span>
<!-- one polite line per drop; picking up and each arrow step say nothing (the handle's pressed state shows it) -->
<span class="sr" role="status">{said}</span>

<div class="bento" data-hold={ready ? undefined : ''} {@attach drag}>
  {#each ids as id (id)}
    <Slot {id} label={label(id)}>
      {@const C = FIXED[id]?.[1] ?? widgets[id]?.[1]}
      {#if isTag(id)}
        <TagCard tagId={id.slice(4)} />
      {:else if C}
        <C />
      {:else}
        {@const m = soon.find((m) => m.id === id)!}
        <Card cols={2} class="soon">
          <h4><Icon name={m.icon} color={m.color} size={16} /> {m.title} <span class="tag">soon</span></h4>
          <div class="lab note">Coming in a later version.</div>
        </Card>
      {/if}
    </Slot>
  {/each}
  <AddModule />
</div>

<style>
  /* Faint full-window smoke behind everything (the sidebar is translucent). */
  .bg { position: fixed; inset: 0; z-index: -1; opacity: .1; pointer-events: none; }
  /* position: the cards' offsets (drag.ts hit tests on them) are relative to the grid */
  .bento { display: grid; grid-template-columns: repeat(6, 1fr); grid-auto-rows: 120px; grid-auto-flow: dense; gap: 12px; position: relative; }
  /* CSS-only fallback if the Home chunk never runs: shown after 3 s like the head script's data-enter-pending. A discrete
     switch, not motion: !important so the reduced-motion `animation: none !important` (base.css) does not cancel it. */
  .bento[data-hold] { visibility: hidden; animation: hold-show 0s 3s forwards !important; }
  @keyframes hold-show { to { visibility: visible; } }
  .bento :global(.soon) { opacity: .7; }
  /* picked up (drag.ts): above the others, a little bigger, a shadow; the hover lift is off while held */
  .bento :global([data-lifted]) { z-index: 3; scale: 1.03; background: var(--panel); /* opaque: the cards below must not show through */ box-shadow: 0 18px 40px rgba(0, 0, 0, .45); border-color: color-mix(in srgb, var(--accent) 55%, var(--line)); }
  .bento :global([data-landing]) { z-index: 2; background: var(--panel); }
  /* its own rule: next to `scale` the minifier drops `scale` (it folds both into transform) */
  .bento :global([data-lifted]:hover) { transform: none; }
  /* once a card has moved (drag.ts sets data-sorted): a DOM move would restart the time-based load entrance, so it
     is dropped (it is over by then; the scroll reveal of untagged cards is timeline-driven and unaffected) */
  .bento:global([data-sorted]) :global([data-enter]) { animation: none; }
  .note { margin-top: 8px; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
</style>
