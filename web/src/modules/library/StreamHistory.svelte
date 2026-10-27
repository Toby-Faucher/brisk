<script lang="ts">
  // "Streamed 12 times · 14 h · last 3 d ago · usually Crisp", plus a one-click suggestion for a cheaper preset.
  import { api, type Preset, type SessionRec } from '../../state/api';
  import { homeOrder, prefs, refreshPrefs } from '../../state/prefs';
  import { toast } from '../../shell/Toasts.svelte';
  import { fmtAgo } from '../home/home';
  import { gameHistory, fmtTotal, recentTrouble, recommendPreset } from './streamHistory';

  let { appid }: { appid: number } = $props();

  const NAME: Record<Preset, string> = { fast: 'Fast', balanced: 'Balanced', crisp: 'Crisp', custom: 'Custom' };

  // Loaded once when the panel opens; nothing shows while loading or on error. hostd sends at most the last 100 sessions.
  let sessions = $state<SessionRec[]>([]);
  $effect(() => {
    const id = appid;
    sessions = [];
    api.sessions().then(
      (v) => id === appid && (sessions = v.sessions),
      () => {},
    );
  });

  const h = $derived(gameHistory(sessions, appid));
  const rec = $derived(recommendPreset(sessions, appid));
  const trouble = $derived(recentTrouble(sessions, appid));
  const shown = $derived(rec && $prefs[appid]?.preset !== rec ? rec : null);
  const dur = $derived(fmtTotal(h.totalMs));

  let saving = $state(false);
  async function use(preset: Preset) {
    if (saving) return;
    saving = true;
    const id = appid; // the panel may switch games while the save is in flight
    try {
      await refreshPrefs(); // so a saved codec is known before it is kept
      if ($homeOrder === undefined) { // still undefined: the GET failed, so a saved codec is unknown
        toast('Could not load your saved settings', { error: true });
        return;
      }
      const saved = await api.putPrefs(id, { preset, codec: $prefs[id]?.codec ?? 'h264' });
      prefs.update((m) => ({ ...m, [id]: saved }));
      toast(`Saved ${NAME[preset]} for this game`);
    } catch {
      toast(`Could not save ${NAME[preset]}`, { error: true });
    } finally {
      saving = false;
    }
  }
</script>

{#if h.count > 0}
  <section class="sec" aria-label="Stream history">
    <p class="lab">Streamed {h.count} {h.count === 1 ? 'time' : 'times'} · {dur} · last {fmtAgo(h.last)}{h.usualPreset ? ` · usually ${NAME[h.usualPreset]}` : ''}</p>
    {#if shown}
      <p class="sug">
        Streaming was rough in {trouble.bad} of the last {trouble.of} sessions (dropped frames or a failed check). Try {NAME[shown]}?
        <button class="btn ghost" aria-disabled={saving} onclick={() => use(shown)}>Use {NAME[shown]}</button>
      </p>
    {/if}
  </section>
{/if}

<style>
  .sec { padding: 14px 22px 0; display: grid; gap: 10px; }
  .sug { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; margin: 0; }
</style>
