<script lang="ts">
  // "Streamed 12 times · 14 h · last 3 d ago · usually Crisp", plus a one-click suggestion for a cheaper preset.
  import { api, type Preset, type SessionRec } from '../../state/api';
  import { prefs } from '../../state/prefs';
  import { toast } from '../../shell/Toasts.svelte';
  import { fmtAgo } from '../home/home';
  import { badCount, gameHistory, recommendPreset } from './streamHistory';

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
  const from = $derived(rec && sessions.filter((s) => s.appid === appid && s.ended != null).sort((a, b) => b.started - a.started)[0]?.preset);
  const shown = $derived(rec && $prefs[appid]?.preset !== rec ? rec : null);
  const dur = $derived(h.totalMs < 3_600_000 ? `${Math.round(h.totalMs / 60_000)} min` : `${Math.round(h.totalMs / 3_600_000)} h`);

  async function use(preset: Preset) {
    try {
      const saved = await api.putPrefs(appid, { preset, codec: $prefs[appid]?.codec ?? 'h264' });
      prefs.update((m) => ({ ...m, [appid]: saved }));
      toast(`Saved ${NAME[preset]} for this game`);
    } catch {
      toast(`Could not save ${NAME[preset]}`);
    }
  }
</script>

{#if h.count > 0}
  <section class="sec" aria-label="Stream history">
    <p class="lab">Streamed {h.count} {h.count === 1 ? 'time' : 'times'} · {dur} · last {fmtAgo(h.last)}{h.usualPreset ? ` · usually ${NAME[h.usualPreset]}` : ''}</p>
    {#if shown && from}
      <p class="sug">
        Frames were dropped in {badCount(sessions, appid).bad} of the last {badCount(sessions, appid).of} sessions at {NAME[from]}. Try {NAME[shown]}?
        <button class="btn ghost" onclick={() => use(shown)}>Use {NAME[shown]}</button>
      </p>
    {/if}
  </section>
{/if}

<style>
  .sec { padding: 14px 22px 0; display: grid; gap: 10px; }
  .sug { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; margin: 0; }
</style>
