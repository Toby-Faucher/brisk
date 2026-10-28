// The save loop for launch options, for the defaults and for one game: load, autosave after a short pause (one write
// at a time, the latest edit wins), a 400 shows hostd's plain problems, a 409 shows what changed elsewhere. Pure of the DOM.
import { get, writable, type Readable } from 'svelte/store';
import { api, ApiError, type GameLaunch, type LaunchDefaults } from '../../state/api';
import { allInherit, formOf, gsProblems, inheritOf, optionsOf, overrideOf, withAllow, type Form, type Inherit } from './form';

export const SAVED = 'Saved. Used the next time a game starts.';
export const SAVE_FAILED = 'Could not save the launch options.';
export const LOAD_FAILED = 'Could not load the launch options.';
export const CONFLICT = 'Another device changed these options. Showing the latest.';
export const BACK_TO_DEFAULTS = 'Back to the defaults.';

export type Note = { text: string; tone: 'info' | 'error' };
export type State<S, D> = {
  load: 'loading' | 'ready' | 'error';
  server: S | null;
  /** What the form shows: the last confirmed state plus edits not yet confirmed. */
  draft: D | null;
  saving: boolean;
  saved: boolean;
  /** hostd refused the draft: its reasons, in plain words. */
  problems: string[];
  note: Note | null;
};

type Cfg<S extends { rev: string }, D> = {
  read: () => Promise<S>;
  write: (d: D, rev: string) => Promise<S>;
  draftOf: (s: S) => D;
  /** After a save: bring the parts the server decides (inherited fields) back into the draft. */
  merge?: (d: D, s: S) => D;
  /** Client-side problems; any means nothing is sent. */
  check?: (d: D) => string[];
  reset?: () => Promise<S>;
  /** A 409's `current`, completed with what only another route knows. */
  adopt?: (current: unknown) => S;
  debounce?: number;
};

function makeStore<S extends { rev: string }, D>(c: Cfg<S, D>) {
  const store = writable<State<S, D>>({
    load: 'loading',
    server: null,
    draft: null,
    saving: false,
    saved: false,
    problems: [],
    note: null,
  });
  const set = (p: Partial<State<S, D>>) => store.update((s) => ({ ...s, ...p }));
  const cur = () => get(store);
  let dirty = false;
  let running: Promise<void> | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const take = (s: S, p: Partial<State<S, D>> = {}) => set({ server: s, draft: c.draftOf(s), problems: [], ...p });

  async function load() {
    set({ load: 'loading', note: null });
    try {
      take(await c.read(), { load: 'ready' });
    } catch {
      set({ load: 'error', note: { text: LOAD_FAILED, tone: 'error' } });
    }
  }

  function pump(): Promise<void> {
    return (running ??= (async () => {
      while (dirty) {
        dirty = false;
        const s = cur();
        if (!s.server || !s.draft) break;
        const sent = s.draft;
        const bad = c.check?.(sent) ?? [];
        if (bad.length) {
          set({ problems: bad, saved: false });
          continue;
        }
        set({ saving: true, saved: false, problems: [] });
        try {
          const r = await c.write(sent, s.server.rev);
          const d = cur().draft;
          set({ server: r, saving: false, saved: !dirty, note: null, ...(c.merge && d ? { draft: c.merge(d, r) } : {}) });
        } catch (e) {
          dirty = false;
          const body = e instanceof ApiError ? (e.body as { problems?: unknown; current?: unknown } | undefined) : undefined;
          if (e instanceof ApiError && e.status === 409 && body?.current) {
            take(c.adopt ? c.adopt(body.current) : (body.current as S), { saving: false, note: { text: CONFLICT, tone: 'info' } });
          } else if (e instanceof ApiError && e.status === 409) {
            set({ saving: false, note: { text: e.message, tone: 'error' } }); // for example: too many games have overrides
          } else if (e instanceof ApiError && e.status === 400) {
            const p = Array.isArray(body?.problems) ? body.problems.filter((x): x is string => typeof x === 'string') : [];
            set({ saving: false, problems: p.length ? p : [e.message] });
          } else {
            set({ saving: false, note: { text: SAVE_FAILED, tone: 'error' } });
          }
        }
      }
    })().finally(() => (running = null)));
  }

  /** `now`: a click on a box or switch saves at once; typing waits for a pause. */
  function edit(d: D, now = false) {
    set({ draft: d, saved: false, note: null, problems: [] });
    dirty = true;
    clearTimeout(timer);
    timer = setTimeout(() => void pump(), now ? 0 : (c.debounce ?? 600));
  }

  async function reset() {
    if (!c.reset) return;
    clearTimeout(timer);
    dirty = false;
    await running;
    set({ saving: true, saved: false, note: null });
    try {
      take(await c.reset(), { saving: false, saved: true, note: { text: BACK_TO_DEFAULTS, tone: 'info' } });
    } catch {
      set({ saving: false, note: { text: SAVE_FAILED, tone: 'error' } });
    }
  }

  /** Try again after a failed load or save. */
  function retry() {
    if (cur().load === 'error') return load();
    dirty = true;
    return pump();
  }

  return {
    state: store as Readable<State<S, D>>,
    load,
    edit,
    reset,
    retry /** Saves a waiting edit now and resolves when the loop is idle (tests, and leaving the page). */,
    flush: async () => {
      clearTimeout(timer);
      if (dirty) void pump();
      await running;
    },
  };
}

export type DefaultsDraft = { form: Form; allow: boolean };
export type DefaultsDeps = { api?: Pick<typeof api, 'launchDefaults' | 'putLaunchDefaults'>; debounce?: number };

export function createDefaults(deps: DefaultsDeps = {}) {
  const a = deps.api ?? api;
  return makeStore<LaunchDefaults, DefaultsDraft>({
    debounce: deps.debounce,
    read: () => a.launchDefaults(),
    draftOf: (s) => ({ form: formOf(s.options), allow: s.allowCustomCommands }),
    check: (d) => gsProblems(d.form.gs),
    // Turning the switch off clears the command in this same save.
    write: (d, rev) => a.putLaunchDefaults({ rev, allowCustomCommands: d.allow, options: optionsOf(withAllow(d.form, d.allow)) }),
    merge: (d) => (d.allow ? d : { ...d, form: withAllow(d.form, false) }),
  });
}

export type GameState = GameLaunch & { envAllowed: string[] };
export type GameDraft = { form: Form; inherit: Inherit };
export type GameDeps = { api?: Pick<typeof api, 'gameLaunch' | 'putGameLaunch' | 'resetGameLaunch' | 'launchDefaults'>; debounce?: number };

export function createGameLaunch(id: string, deps: GameDeps = {}) {
  const a = deps.api ?? api;
  let env: string[] = []; // the allowed names: only the defaults route says, so keep them from the first load
  const withEnv = (g: GameLaunch): GameState => ({ ...g, envAllowed: env });
  return makeStore<GameState, GameDraft>({
    debounce: deps.debounce,
    read: async () => {
      const [g, d] = await Promise.all([a.gameLaunch(id), a.launchDefaults().catch(() => null)]);
      env = d?.envAllowed ?? env;
      return withEnv(g);
    },
    draftOf: (s) => ({ form: formOf(s.effective), inherit: inheritOf(s.override) }),
    check: (d) => gsProblems(d.form.gs),
    // Every field on Inherit is the same as no override at all: that is a reset, not an empty override.
    write: async (d, rev) => {
      return withEnv(
        allInherit(d.inherit) ? await a.resetGameLaunch(id) : await a.putGameLaunch(id, { rev, override: overrideOf(d.form, d.inherit) }),
      );
    },
    // An inherited field shows the default as hostd now sees it.
    merge: (d, s) => {
      const f = formOf(s.effective);
      const form = { ...d.form };
      if (d.inherit.wrappers) Object.assign(form, { mango: f.mango, gamemode: f.gamemode, gs: f.gs });
      if (d.inherit.env) form.env = f.env;
      if (d.inherit.custom) form.custom = f.custom;
      if (d.inherit.extra) form.extra = f.extra;
      return { ...d, form };
    },
    reset: async () => withEnv(await a.resetGameLaunch(id)),
    adopt: (c) => withEnv(c as GameLaunch),
  });
}
