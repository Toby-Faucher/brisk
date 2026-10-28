// The mapping store for one emulator controller type: load, autosave (one save in flight, the latest draft wins), reset
// and the restart flow (quit, GET, PUT without rev, launch). Pure of the DOM: the card passes `toast` in, as quit.ts does.
import { get, writable, type Readable } from 'svelte/store';
import { api, ApiError, type Bindings, type ControllerType, type ControlsData } from '../../../state/api';
import type { ToastOpts } from '../../../state/quit';
import { idsOf } from './mapping';

export const SAVED = 'Saved. Used the next time a game starts.';
export const SAVE_FAILED = 'Could not save the mapping.';
export const LOAD_FAILED = 'Could not load the mapping.';
export const CONFLICT = 'Another device changed the mapping. Showing the latest.';
export const RESTART_FAILED = { close: 'Could not close the game', save: 'Could not save the mapping', start: 'Could not start the game' };

type Draft = { bindings: Bindings; deadZone: ControlsData['deadZone'] };
export type Note = { text: string; tone: 'info' | 'error' };
export type State = {
  /** hidden: hostd said 404 (no Dolphin on this machine), the card shows nothing. */
  load: 'loading' | 'ready' | 'hidden' | 'error';
  data: ControlsData | null;
  /** What the diagram shows: the last confirmed state plus edits not yet confirmed. */
  draft: Draft | null;
  saving: boolean;
  /** The last save was confirmed and nothing newer is waiting. */
  saved: boolean;
  /** Something was saved this visit: with a running game the restart bar offers to apply it. */
  changed: boolean;
  restarting: boolean;
  note: Note | null;
  /** The restart step that failed, for the bar. */
  restartError: string | null;
};

export type ControlsDeps = {
  api?: Pick<typeof api, 'emulatorControls' | 'putEmulatorControls' | 'resetEmulatorControls' | 'quit' | 'launch'>;
  toast?: (text: string, opts?: ToastOpts) => void;
};

const draftOf = (d: ControlsData): Draft => ({ bindings: { ...d.bindings }, deadZone: { ...d.deadZone } });
const start: State = {
  load: 'loading',
  data: null,
  draft: null,
  saving: false,
  saved: false,
  changed: false,
  restarting: false,
  note: null,
  restartError: null,
};

export type Target = { appid: number; name: string };

export function createMappings(type: ControllerType, deps: ControlsDeps = {}) {
  const c = deps.api ?? api;
  const toast = deps.toast ?? (() => {});
  const store = writable<State>({ ...start });
  const set = (p: Partial<State>) => store.update((s) => ({ ...s, ...p }));
  const cur = () => get(store);

  let dirty = false; // an edit is waiting for a save
  let running: Promise<void> | null = null; // the save loop
  let failed: Draft | null = null; // what the last failed save tried to write, for Try again

  /** A fresh server state replaces the confirmed one and the view. */
  const take = (d: ControlsData, p: Partial<State> = {}) => set({ data: d, draft: draftOf(d), ...p });

  async function load() {
    set({ load: 'loading', note: null });
    try {
      take(await c.emulatorControls(type), { load: 'ready' });
    } catch (e) {
      set(e instanceof ApiError && e.status === 404 ? { load: 'hidden' } : { load: 'error', note: { text: LOAD_FAILED, tone: 'error' } });
    }
  }

  /** Back from another tab or window: show what is on the machine, unless an edit is on its way. */
  async function refresh() {
    const s = cur();
    if (s.load !== 'ready' || dirty || running || s.restarting) return;
    try {
      const d = await c.emulatorControls(type);
      if (!dirty && !running) take(d);
    } catch {}
  }

  function pump(): Promise<void> {
    return (running ??= (async () => {
      while (dirty) {
        dirty = false;
        const s = cur();
        if (!s.data || !s.draft) break;
        const sent = s.draft;
        set({ saving: true, saved: false });
        try {
          const r = await c.putEmulatorControls(type, { rev: s.data.rev, ...sent });
          // A newer edit is waiting: keep showing it, but carry hostd's rev into the next PUT.
          set({ data: r, saving: false, saved: !dirty, changed: true, ...(dirty ? {} : { draft: draftOf(r) }) });
        } catch (e) {
          dirty = false;
          if (e instanceof ApiError && e.status === 409) {
            const now = (e.body as { current?: ControlsData } | undefined)?.current;
            set({ saving: false, note: { text: CONFLICT, tone: 'info' } });
            if (now) take(now);
            else await load();
          } else {
            failed = sent;
            set({ saving: false, draft: s.data ? draftOf(s.data) : null, note: { text: SAVE_FAILED, tone: 'error' } });
          }
        }
      }
    })().finally(() => (running = null)));
  }

  function edit(next: Draft, note: Note | null) {
    set({ draft: next, note, saved: false });
    dirty = true;
    void pump();
  }

  /** Bind through mapping.bind's result; the caller names the swap in its own message. */
  function setBindings(bindings: Bindings, note: Note | null) {
    const s = cur();
    if (s.draft) edit({ ...s.draft, bindings }, note);
  }

  function setDeadZone(id: string, n: number) {
    const s = cur();
    if (!s.draft || s.draft.deadZone[id] === n) return;
    edit({ ...s.draft, deadZone: { ...s.draft.deadZone, [id]: n } }, null);
  }

  async function reset() {
    await running;
    dirty = false;
    set({ saving: true, saved: false, note: null });
    try {
      take(await c.resetEmulatorControls(type), {
        saving: false,
        saved: true,
        changed: true,
        note: { text: 'Reset to the default.', tone: 'info' },
      });
    } catch {
      set({ saving: false, note: { text: SAVE_FAILED, tone: 'error' } });
    }
  }

  /** Try again after a failed load or a failed save. */
  async function retry() {
    if (cur().load === 'error') return load();
    if (failed) {
      const d = failed;
      failed = null;
      edit(d, null);
    }
  }

  /** Quit, write again after Dolphin's exit rewrite, launch. `force` is the Force quit retry. */
  async function restart(target: Target, force = false) {
    if (cur().restarting) return;
    await running;
    const wanted = cur().draft;
    if (!wanted) return;
    set({ restarting: true, restartError: null });
    const fail = (step: keyof typeof RESTART_FAILED) => set({ restarting: false, restartError: RESTART_FAILED[step] });
    try {
      const q = await c.quit(target.appid, force);
      if (q.state === 'still-running') {
        toast(`Still running: ${target.name} (try Force quit)`, {
          ms: 30_000,
          action: { label: 'Force quit', run: () => void restart(target, true) },
        });
        return fail('close');
      }
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 409)) return fail('close'); // 409: it had already closed
    }
    try {
      const now = await c.emulatorControls(type);
      // A control hostd now reads as plain keeps its file value; only still-custom ones may go as null.
      const bindings = Object.fromEntries(idsOf(type).map((id) => [id, wanted.bindings[id] ?? now.bindings[id]])) as Bindings;
      take(await c.putEmulatorControls(type, { bindings, deadZone: wanted.deadZone }));
    } catch {
      return fail('save');
    }
    try {
      await c.launch(target.appid);
    } catch {
      return fail('start');
    }
    set({ restarting: false, changed: false, saved: false, note: { text: 'Restarted the game with the new mapping.', tone: 'info' } });
  }

  return {
    setNote: (text: string, tone: Note['tone'] = 'info') => set({ note: { text, tone }, saved: false }),
    state: store as Readable<State>,
    load,
    refresh,
    retry,
    reset,
    restart,
    setBindings,
    setDeadZone,
    /** Resolves when the save loop is idle (tests, and the restart flow). */
    idle: () => running ?? Promise.resolve(),
  };
}
