// One session, one hub, two machines: `oldbox` runs an old hostd (numbers only), `newbox` a new one (string ids).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import oldFx from '../../../crates/brisk-hostd/tests/fixtures/stage1/games-old.json';
import newFx from '../../../crates/brisk-hostd/tests/fixtures/stage1/games-new.json';

const ok = (body: unknown = {}) => new Response(JSON.stringify(body), { status: 200 });

async function setup(hub: { notes?: unknown; prefs?: unknown } = {}) {
  vi.resetModules();
  vi.stubGlobal('sessionStorage', { getItem: () => null, setItem() {} });
  vi.stubGlobal('localStorage', { getItem: () => null, setItem() {} });
  vi.stubGlobal('location', { href: 'http://localhost/' });
  const f = vi.fn(async (url: string) => {
    if (url === '/api/m/oldbox/games') return ok(oldFx);
    if (url === '/api/m/newbox/games') return ok(newFx);
    if (url === '/api/notes') return ok(hub.notes ?? { tags: {}, games: {} });
    if (url === '/api/prefs') return ok({ prefs: hub.prefs ?? {} });
    return ok({});
  });
  vi.stubGlobal('fetch', f);
  const a = await import('./api');
  const last = () => {
    const [url, init] = f.mock.calls.at(-1) as unknown as [string, RequestInit | undefined];
    return { url, method: init?.method, body: init?.body ? JSON.parse(init.body as string) : undefined };
  };
  return { api: a.api, setRoute: a.setRoute, f, last };
}

beforeEach(() => vi.useRealTimers());
afterEach(() => vi.unstubAllGlobals());

describe('a hub with one old and one new machine', () => {
  it('names the game by number on the old machine and by id on the new one', async () => {
    const t = await setup();
    t.setRoute('oldbox');
    await t.api.games();
    await t.api.launch('620');
    expect(t.last()).toEqual({ url: '/api/m/oldbox/launch', method: 'POST', body: { appid: 620 } });
    await t.api.quit('620', true);
    expect(t.last().body).toEqual({ appid: 620, force: true });
    await t.api.install('620');
    expect(t.last().body).toEqual({ appid: 620 });
    await t.api.gameInfo('620');
    expect(t.last().url).toBe('/api/m/oldbox/games/620/info');
    await t.api.launch('3907440760'); // an old emulator number goes through as a number
    expect(t.last().body).toEqual({ appid: 3907440760 });

    t.setRoute('newbox');
    await t.api.games();
    await t.api.launch('steam:620');
    expect(t.last()).toEqual({ url: '/api/m/newbox/launch', method: 'POST', body: { id: 'steam:620' } });
    await t.api.launch('emu:dolphin:mario-kart-wii');
    expect(t.last().body).toEqual({ id: 'emu:dolphin:mario-kart-wii' });
    await t.api.quit('emu:dolphin:mario-kart-wii');
    expect(t.last().body).toEqual({ id: 'emu:dolphin:mario-kart-wii', force: false });
    await t.api.gameInfo('emu:dolphin:mario-kart-wii', true);
    expect(t.last().url).toBe('/api/m/newbox/games/emu%3Adolphin%3Amario-kart-wii/info?refresh=1');
    await t.api.gameLocal('steam:620');
    expect(t.last().url).toBe('/api/m/newbox/games/steam%3A620/local');
  });

  it('turns a Steam key of the other form into the machine form', async () => {
    const t = await setup();
    t.setRoute('oldbox');
    await t.api.games();
    await t.api.launch('steam:620');
    expect(t.last().body).toEqual({ appid: 620 });
    t.setRoute('newbox');
    await t.api.games();
    await t.api.launch('620');
    expect(t.last().body).toEqual({ id: 'steam:620' });
  });

  it('decides per machine from its own games answer, also for forMachine', async () => {
    const t = await setup();
    await t.api.forMachine('newbox').games();
    await t.api.forMachine('oldbox').games();
    t.setRoute('newbox');
    await t.api.launch('620');
    expect(t.last().body).toEqual({ id: 'steam:620' });
    t.setRoute('oldbox');
    await t.api.launch('steam:620');
    expect(t.last().body).toEqual({ appid: 620 });
  });

  it('a stream start names the game the same way and the Desktop names none', async () => {
    const t = await setup();
    t.setRoute('newbox');
    await t.api.games();
    await t.api.startStream({ preset: 'fast', game: '620' });
    expect(t.last().body).toEqual({ preset: 'fast', id: 'steam:620' });
    await t.api.startStream({ preset: 'fast' });
    expect(t.last().body).toEqual({ preset: 'fast' });
    t.setRoute('oldbox');
    await t.api.games();
    await t.api.startStream({ preset: 'fast', game: 'steam:620' });
    expect(t.last().body).toEqual({ preset: 'fast', appid: 620 });
  });

  it('an unknown machine gets numbers for Steam, which every hostd accepts', async () => {
    const t = await setup();
    t.setRoute('mystery');
    await t.api.launch('steam:620');
    expect(t.last().body).toEqual({ appid: 620 });
  });

  it('screenshots use the encoded id too', async () => {
    const t = await setup();
    t.setRoute('newbox');
    await t.api.games();
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: () => 'blob:x' }));
    await t.api.screenshot('steam:620', 'a b.png');
    expect(t.last().url).toBe('/api/m/newbox/games/steam%3A620/screenshots/a%20b.png');
  });
});

describe('notes and prefs go to the hub in the form the hub speaks', () => {
  it('writes numbers to a hub that has shown no ids, ids once it has', async () => {
    const t = await setup({ notes: { tags: {}, games: { '620': { note: 'old', tags: [] } } } });
    t.setRoute('newbox');
    await t.api.games();
    await t.api.putGameNote('steam:620', { note: 'x', tags: [] });
    expect(t.last().url).toBe('/api/notes/games/620');
    await t.api.putPrefs('steam:620', { preset: 'fast', codec: 'h264' });
    expect(t.last().url).toBe('/api/prefs/620');

    await t.api.notes(); // still numbers
    await t.api.putGameNote('steam:620', { note: 'x', tags: [] });
    expect(t.last().url).toBe('/api/notes/games/620');
  });

  it('a hub whose notes have id keys gets ids, even for a game from an old machine', async () => {
    const t = await setup({ notes: { tags: {}, games: { 'steam:730': { note: 'n', tags: [] } } } });
    t.setRoute('oldbox');
    await t.api.games();
    await t.api.notes();
    await t.api.putGameNote('620', { note: 'x', tags: [] });
    expect(t.last().url).toBe('/api/notes/games/steam%3A620');
    await t.api.putPrefs('620', { preset: 'fast', codec: 'h264' });
    expect(t.last().url).toBe('/api/prefs/steam%3A620');
    await t.api.deletePrefs('620');
    expect(t.last()).toMatchObject({ url: '/api/prefs/steam%3A620', method: 'DELETE' });
  });

  it('a hub that is a single new host speaks ids from its games answer', async () => {
    const t = await setup();
    t.f.mockImplementation(async (url: string) => ok(url === '/api/games' ? newFx : {}));
    await t.api.games();
    await t.api.putGameNote('620', { note: 'x', tags: [] });
    expect(t.last().url).toBe('/api/notes/games/steam%3A620');
  });

  it('emulator ids always go as ids', async () => {
    const t = await setup();
    await t.api.putGameNote('emu:dolphin:mario-kart-wii', { note: 'x', tags: [] });
    expect(t.last().url).toBe('/api/notes/games/emu%3Adolphin%3Amario-kart-wii');
  });
});
