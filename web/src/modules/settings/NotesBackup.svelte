<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { Dialog } from 'bits-ui';
  import { api } from '../../state/api';
  import { refreshNotes } from '../../state/notes';
  import { backupName, buildBackup, MAX_BACKUP_BYTES, parseBackup, planRestore } from '../../state/notesBackup';
  import Card from '../../ui/Card.svelte';

  type Parsed = Extract<ReturnType<typeof parseBackup>, { ok: true }>;
  let error = $state('');
  let progress = $state('');
  let busy = $state(false);
  let running = $state(false);
  let ask = $state(false);
  let parsed: Parsed | null = null;
  // what the dialog says; kept after the restore starts so the text does not change while the dialog closes
  let info = $state({ tags: 0, games: 0, replaced: 0 });
  let status: HTMLElement | undefined = $state();
  let alertEl: HTMLElement | undefined = $state();
  let focusStatus = false;
  let stop = false;
  let revoke: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => clearTimeout(revoke));
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
      a.download = backupName(now);
      a.click();
      revoke = setTimeout(() => URL.revokeObjectURL(url), 1000); // after the browser has started the download
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
    const tags = Object.keys(r.tags).length;
    const games = Object.keys(r.games).length;
    if (!tags && !games) return void (progress = 'Nothing to import: the file has no tags and no games with notes.');
    busy = true;
    try {
      const plan = planRestore(await api.notes().then((n) => ({ tags: n.tags ?? {}, games: n.games ?? {} })), r.tags, r.games);
      if (plan.error) return void (error = plan.error);
      info = { tags, games, replaced: plan.replaced };
    } catch {
      return void (error = 'Could not check the notes on the host, so nothing was imported.');
    } finally {
      busy = false;
    }
    parsed = r;
    ask = true;
  }

  async function restore() {
    const p = parsed;
    parsed = null; // a second click does nothing
    ask = false;
    if (!p) return;
    const jobs: (() => Promise<unknown>)[] = [
      ...Object.entries(p.tags).map(([id, t]) => () => api.putTag(id, t)),
      ...Object.entries(p.games).map(([appid, g]) => () => api.putGameNote(Number(appid), g)),
    ];
    busy = running = true;
    stop = false;
    focusStatus = true;
    let done = 0;
    try {
      for (const send of jobs) {
        if (stop) throw new Error('stopped');
        progress = `Importing ${done + 1} of ${jobs.length}…`;
        await send();
        done++;
      }
      progress = `Imported ${plural(info.tags, 'tag')} and ${plural(info.games, 'game')}.`;
    } catch (e) {
      progress = '';
      const why = stop ? 'you stopped it' : e instanceof Error ? e.message : 'the host did not answer';
      error = `Stopped after ${done} of ${jobs.length} entries (tags first, then games): ${why}.`;
    } finally {
      busy = running = false;
      await tick();
      (error ? alertEl : status)?.focus(); // the Stop button is gone: the final message takes focus
      await refreshNotes();
    }
  }
</script>

<Card cols={2}>
  <h4>Notes and tags</h4>
  <div class="opt"><span>Back up <small class="lab">a JSON file of all tags and game notes</small></span><button type="button" class="btn ghost" disabled={busy} onclick={download}>Download notes and tags</button></div>
  <div class="opt"><span><label for="notes-restore">Restore from file</label> <small class="lab" id="notes-restore-hint">max 2 MiB. Each tag and game in the file replaces the one here, note and tags included. Tags and games that are not in the file are kept.</small></span><input id="notes-restore" type="file" accept="application/json,.json" aria-describedby="notes-restore-hint" disabled={busy} onchange={pick} /></div>
  <p class="note" role="status" tabindex="-1" bind:this={status}>{progress}</p>
  {#if running}<button type="button" class="btn ghost" onclick={() => (stop = true)}>Stop restoring</button>{/if}
  {#if error}<p class="note err" role="alert" tabindex="-1" bind:this={alertEl}>{error}</p>{/if}
</Card>

<Dialog.Root bind:open={ask}>
  <Dialog.Portal>
    <Dialog.Overlay class="cm-dim" />
    <Dialog.Content
      class="rs glass"
      onCloseAutoFocus={(e) => {
        if (!focusStatus) return;
        e.preventDefault(); // the file input is disabled by now; the progress text takes focus while it runs
        focusStatus = false;
        if (running) status?.focus();
      }}
    >
      <Dialog.Title class="rs-t">Restore notes and tags?</Dialog.Title>
      <Dialog.Description class="lab">This imports {plural(info.tags, 'tag')} and {plural(info.games, 'game')}. Each tag and game in the file replaces the one here, note and tags included. Tags and games that are not in the file are kept.{#if info.replaced} {info.replaced} of the games in the file replace a game that is already here.{/if}</Dialog.Description>
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
  .note:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .err { color: var(--bad); }
</style>
