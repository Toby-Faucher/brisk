import { describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import type { Game } from './api';
import { quitFlow, quitItems, type QuitState } from './quit';

const g = (appid: number, name: string, running: boolean): Game => ({ appid, name, installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 0, playtime2wkMin: 0, cover: '', hero: '', logo: '', achUnlocked: null, achTotal: null, updated: false, running });

function rig(answers: (QuitState | Error)[]) {
  const calls: [number, boolean][] = [];
  const pending: (() => void)[] = [];
  const quit = vi.fn((appid: number, force: boolean) => {
    calls.push([appid, force]);
    const a = answers.shift()!;
    return new Promise<{ state: QuitState }>((ok, no) => pending.push(() => (a instanceof Error ? no(a) : ok({ state: a }))));
  });
  const refresh = vi.fn(async () => {});
  const toast = vi.fn();
  const flow = quitFlow({ quit, refresh, toast });
  const answer = async () => {
    pending.shift()!();
    await new Promise((r) => setTimeout(r));
  };
  return { flow, calls, refresh, toast, answer };
}

const PG = g(2076580, 'Pepper Grinder', true);

describe('quit', () => {
  it('quits only after the confirm, and cancel does nothing', async () => {
    const r = rig(['closed']);
    r.flow.request(PG);
    expect(get(r.flow.ask)).toEqual({ game: PG, force: false });
    r.flow.cancel();
    expect(get(r.flow.ask)).toBeNull();
    expect(r.calls).toEqual([]);

    r.flow.request(PG);
    const done = r.flow.confirm();
    expect(get(r.flow.ask)).toBeNull(); // the dialog closes at once
    expect(r.calls).toEqual([[2076580, false]]);
    await r.answer();
    await done;
  });

  it('is busy while quitting and ignores a second request for the same game', async () => {
    const r = rig(['closed']);
    r.flow.request(PG);
    const done = r.flow.confirm();
    expect(get(r.flow.busy).has(PG.appid)).toBe(true);
    r.flow.request(PG); // a second click while busy opens no second confirm
    expect(get(r.flow.ask)).toBeNull();
    await r.answer();
    await done;
    expect(get(r.flow.busy).has(PG.appid)).toBe(false);
    expect(r.calls).toHaveLength(1);
  });

  it('closed: refreshes the games and toasts Closed', async () => {
    const r = rig(['closed']);
    r.flow.request(PG);
    const done = r.flow.confirm();
    await r.answer();
    await done;
    expect(r.toast).toHaveBeenCalledWith('Closed Pepper Grinder');
    expect(r.refresh).toHaveBeenCalled();
  });

  it('still running: toasts with a Force quit action that asks again, then forces', async () => {
    const r = rig(['still-running', 'closed']);
    r.flow.request(PG);
    const done = r.flow.confirm();
    await r.answer();
    await done;
    const [text, opts] = r.toast.mock.calls[0];
    expect(text).toBe('Still running: Pepper Grinder (try Force quit)');
    expect(opts.action.label).toBe('Force quit');
    expect(r.refresh).toHaveBeenCalled();

    opts.action.run();
    expect(get(r.flow.ask)).toEqual({ game: PG, force: true }); // a second confirm, not a silent retry
    expect(r.calls).toHaveLength(1);
    const again = r.flow.confirm();
    await r.answer();
    await again;
    expect(r.calls).toEqual([[2076580, false], [2076580, true]]);
    expect(r.toast).toHaveBeenLastCalledWith('Closed Pepper Grinder');
  });

  it('an API error is an error toast and clears busy', async () => {
    const r = rig([new Error('not running')]);
    r.flow.request(PG);
    const done = r.flow.confirm();
    await r.answer();
    await done;
    expect(r.toast).toHaveBeenCalledWith('Could not quit Pepper Grinder: not running', { error: true });
    expect(get(r.flow.busy).size).toBe(0);
  });

  it('no user-facing copy says kill', async () => {
    const r = rig(['still-running', new Error('x')]);
    r.flow.request(PG);
    let d = r.flow.confirm();
    await r.answer();
    await d;
    r.flow.request(PG, true);
    d = r.flow.confirm();
    await r.answer();
    await d;
    const items = quitItems([PG], get(r.flow.stuck), () => {});
    const copy = JSON.stringify([r.toast.mock.calls, items.map((i) => i.label)]);
    expect(copy).not.toMatch(/kill/i);
  });

  it('a quit already under way is an info toast, not an error', async () => {
    const busy = Object.assign(new Error('already quitting'), { status: 409 });
    const r = rig([busy]);
    r.flow.request(PG);
    const done = r.flow.confirm();
    await r.answer();
    await done;
    expect(r.toast).toHaveBeenCalledWith('Already closing Pepper Grinder…');
    expect(r.refresh).toHaveBeenCalled();
  });

  it('the Force quit action stays at least 20 s', async () => {
    const r = rig(['still-running']);
    r.flow.request(PG);
    const done = r.flow.confirm();
    await r.answer();
    await done;
    expect(r.toast.mock.calls[0][1].ms).toBeGreaterThanOrEqual(20_000);
  });

  it('command menu offers Quit only for running games', () => {
    const asked: Game[] = [];
    const items = quitItems([PG, g(1, 'Hades', false), g(3, 'Celeste', true)], new Set(), (x) => asked.push(x));
    expect(items.map((i) => i.label)).toEqual(['Quit Pepper Grinder', 'Quit Celeste']);
    items[1].run();
    expect(asked).toEqual([g(3, 'Celeste', true)]);
  });

  it('command menu adds Force quit while a game is still running after a quit, until it closes', async () => {
    const r = rig(['still-running', 'closed']);
    const labels = () => quitItems([PG], get(r.flow.stuck), () => {}).map((i) => i.label);
    expect(labels()).toEqual(['Quit Pepper Grinder']);
    r.flow.request(PG);
    let d = r.flow.confirm();
    await r.answer();
    await d;
    expect(labels()).toEqual(['Quit Pepper Grinder', 'Force quit Pepper Grinder']);
    const asked: [Game, boolean][] = [];
    quitItems([PG], get(r.flow.stuck), (x, force) => asked.push([x, force]))[1].run();
    expect(asked).toEqual([[PG, true]]);
    // not offered for a game that is no longer running
    expect(quitItems([{ ...PG, running: false }], get(r.flow.stuck), () => {})).toEqual([]);
    r.flow.request(PG, true);
    d = r.flow.confirm();
    await r.answer();
    await d;
    expect(labels()).toEqual(['Quit Pepper Grinder']);
  });
});
