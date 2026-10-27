<script lang="ts">
  import { Dialog } from 'bits-ui';
  import { api } from '../../state/api';
  import { refreshNotes } from '../../state/notes';
  import { buildBackup, MAX_BACKUP_BYTES, parseBackup } from '../../state/notesBackup';
  import Card from '../../ui/Card.svelte';

  type Parsed = Extract<ReturnType<typeof parseBackup>, { ok: true }>;
  let error = $state('');
  let progress = $state('');
  let busy = $state(false);
  let ask = $state(false);
  let parsed = $state<Parsed | null>(null);
  const nTags = $derived(Object.keys(parsed?.tags ?? {}).length);
  const nGames = $derived(Object.keys(parsed?.games ?? {}).length);
  const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 's'}`;

  async function download() {
    error = progress = '';
    busy = true;
    try {
      const now = new Date();
      const blob = new Blob([JSON.stringify(buildBackup(await api.notes(), now), null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `brisk-notes-${now.toISOString().slice(0, 10)}.json`;
      a.click();
      URL.revokeObjectURL(url);
    } catch {
      error = 'Could not download: the host did not answer.';
    } finally {
      busy = false;
    }
  }

  async function pick(e: Event & { currentTarget: HTMLInputElement }) {
    const input = e.currentTarget;
    const f = input.files?.[0];
    input.value = ''; // the same file can be picked again
    if (!f) return;
    error = progress = '';
    parsed = null;
    if (f.size > MAX_BACKUP_BYTES) return void (error = 'That file is bigger than 2 MiB.');
    const r = parseBackup(await f.text().catch(() => ''));
    if (!r.ok) return void (error = r.error);
    parsed = r;
    ask = true;
  }

  async function restore() {
    const p = parsed;
    ask = false;
    if (!p) return;
    const jobs: [string, () => Promise<unknown>][] = [
      ...Object.entries(p.tags).map(([id, t]) => [id, () => api.putTag(id, t)] as [string, () => Promise<unknown>]),
      ...Object.entries(p.games).map(([appid, g]) => [appid, () => api.putGameNote(Number(appid), g)] as [string, () => Promise<unknown>]),
    ];
    busy = true;
    let done = 0;
    try {
      for (const [, send] of jobs) {
        progress = `Importing ${done + 1} of ${jobs.length}…`;
        await send();
        done++;
      }
      progress = `Imported ${plural(nTags, 'tag')} and ${plural(nGames, 'game')}.`;
    } catch (e) {
      progress = '';
      error = `Stopped after importing ${done} of ${jobs.length} entries (tags first, then games): ${e instanceof Error ? e.message : 'the host did not answer'}.`;
    } finally {
      busy = false;
      parsed = null;
      await refreshNotes();
    }
  }
</script>

<Card cols={2}>
  <h4>Notes and tags</h4>
  <div class="opt"><span>Back up <small class="lab">a JSON file of all tags and game notes</small></span><button type="button" class="btn ghost" disabled={busy} onclick={download}>Download notes and tags</button></div>
  <div class="opt"><span><label for="notes-restore">Restore from file</label> <small class="lab" id="notes-restore-hint">max 2 MiB; replaces entries with the same id, deletes nothing</small></span><input id="notes-restore" type="file" accept="application/json,.json" aria-describedby="notes-restore-hint" disabled={busy} onchange={pick} /></div>
  {#if progress}<p class="note" role="status">{progress}</p>{/if}
  {#if error}<p class="note err" role="alert">{error}</p>{/if}
</Card>

<Dialog.Root bind:open={ask}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content class="rs glass">
      <Dialog.Title class="rs-t">Restore notes and tags?</Dialog.Title>
      <Dialog.Description class="lab">This imports {plural(nTags, 'tag')} and {plural(nGames, 'game')}. Existing entries with the same id are replaced. Nothing is deleted.</Dialog.Description>
      <div class="rs-a">
        <Dialog.Close class="btn ghost">Cancel</Dialog.Close>
        <button class="btn" onclick={() => void restore()}>Restore</button>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .opt { display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-top: 14px; }
  .opt input[type='file'] { max-width: 50%; font: inherit; font-size: 13px; }
  .note { margin: 10px 0 0; font-size: 12px; color: var(--muted); }
  .err { color: var(--bad); }
</style>
