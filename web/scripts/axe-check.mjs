// Accessibility pass (axe-core, WCAG 2.x A/AA; serious/critical fail) in headless Chrome against `astro preview` with a
// mocked /api, so it needs no hostd. The same helper (./axe.mjs) runs inside ui-smoke.mjs against the real hostd.
//   npx astro preview --host 127.0.0.1 --port 4424 &   then   node scripts/axe-check.mjs
import { chromium } from 'playwright-core';
import { axe } from './axe.mjs';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4424';
const exe = process.env.BRISK_CHROMIUM ?? `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x ? `\n    ${x}` : ''}`));

const games = Array.from({ length: 139 }, (_, i) => ({ appid: 1000 + i, name: i === 0 ? 'Portal 2' : `Game ${i}`, installed: i < 40, sizeBytes: 1e9, lastPlayed: i < 5 ? 1700000000 - i * 1e5 : null, playtimeMin: i === 4 ? 5 : 90, cover: '', running: i === 0, coop: i === 1, controller: i === 2 ? 'full' : null, achUnlocked: i === 3 ? 9 : null, achTotal: i === 3 ? 10 : null, updated: i === 0, update: i === 1 ? 'needed' : i === 2 ? 'running' : null }));
const sessions = Array.from({ length: 15 }, (_, i) => ({ appid: 1000, name: 'Portal 2', started: 1700000000000 + i * 1e6, ended: 1700000600000 + i * 1e6, host: 'bawn', preset: 'balanced', codec: 'h264', bitrateKbps: 20000, stats: null, spark: [1, 3, 2, 4, 3] }));
const series = Array.from({ length: 120 }, (_, i) => [1700000000000 + i * 5000, 9 + Math.sin(i / 4) * 2, 58 + Math.sin(i / 6) * 3, 18 + Math.cos(i / 9) * 4]);
// screenshots as inline SVG, so the lightbox shows a real image; achievements and news fill the right pane and its modal
const shot = (c) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080"><rect width="1920" height="1080" fill="${c}"/></svg>`)}`;
const shots = ['#345', '#534', '#453'].map(shot);
const info = { available: true, description: 'A test description & more.', genres: ['Action', 'Puzzle'], controller: 'full', singleplayer: true, coop: true, remotePlayTogether: true, release: '1 Jan, 2020', metacritic: 90, screenshots: shots, screenshotsFull: shots };
const achievements = { available: true, unlocked: 3, total: 9, list: Array.from({ length: 9 }, (_, i) => ({ id: `a${i}`, name: `Achievement ${i + 1}`, description: i % 2 ? '' : 'Do the thing.', icon: '', unlockedAt: i < 3 ? 1700000000 + i : null })) };
const news = { items: [{ title: 'Patch 1.2 notes', url: 'https://news.example.test/1', date: 1700000000, patch: true }, { title: 'Community update', url: 'https://news.example.test/2', date: 1699000000, patch: false }] };
// every tag color, so the chips' contrast is measured (Game 1 carries them all)
const colors = ['gray', 'red', 'orange', 'yellow', 'green', 'teal', 'blue', 'purple', 'pink'];
const notes = { tags: Object.fromEntries(colors.map((c, i) => [`t${i}`, { name: `Tag ${c}`, color: c, symbol: 'star', note: i ? '' : 'A tag note', pinned: i === 0 || i === 3 }])), games: { 1001: { note: 'A game note', tags: colors.map((_, i) => `t${i}`).slice(0, 16) }, 1002: { note: '', tags: ['t0'] } } };
const host = { name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null };
// A "live" stream whose WebTransport address answers nothing, so /stream degrades to its failed state.
const live = { state: 'live', preset: 'balanced', codec: 'h264', appid: 1000, wt: 'https://127.0.0.1:4429/', hash: '00'.repeat(32) };

const browser = await chromium.launch({ executablePath: exe });
try {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  let stream = { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null };
  await ctx.route('**/api/**', async (r) => {
    const p = new URL(r.request().url()).pathname;
    const json =
      p === '/api/games' ? { games, ownedAvailable: true }
      : p === '/api/host' ? host
      : p === '/api/stream' ? stream
      : p === '/api/prefs' ? { prefs: {} }
      : p === '/api/notes' ? notes
      : p === '/api/sessions' ? { sessions }
      : p.startsWith('/api/sessions/') ? { session: sessions[0], series }
      : p.endsWith('/info') ? info
      : p.endsWith('/achievements') ? achievements
      : p.endsWith('/news') ? news
      : p.endsWith('/local') ? { screenshots: [], saves: null }
      : {};
    await r.fulfill({ json });
  });
  // One standard pad, so the Controller tab draws its diagram (and the faint extra-button legend) for axe to see.
  await ctx.addInitScript(() => {
    const pad = { id: 'Xbox 360 Controller (STANDARD GAMEPAD Vendor: 045e Product: 028e)', index: 0, connected: true, mapping: 'standard', timestamp: 1, axes: [0, 0, 0, 0], buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })) };
    navigator.getGamepads = () => [pad, null, null, null];
  });
  const page = await ctx.newPage();
  const scan = async (name) => {
    await sleep(600); // entrance animations settle (contrast is measured on the final colours)
    const bad = await axe(page);
    check(`axe: ${name}`, bad.length === 0, bad.join('\n    '));
  };
  const go = async (path) => (await page.goto(BASE + path), await page.waitForLoadState('networkidle'));

  for (const [name, path] of [['home', '/'], ['library', '/library'], ['machines', '/machines'], ['history', '/history'], ['history session', `/history?session=${sessions[0].started}`]]) {
    await go(path);
    if (path === '/') check('home: two pinned tag cards', (await page.locator('[data-card^="tag-"]').count()) === 2);
    await scan(name);
  }
  for (const tab of ['general', 'streaming', 'controller', 'about']) {
    await go(`/settings#${tab}`);
    await scan(`settings #${tab}`);
  }
  await go('/stream');
  await scan('stream idle');
  stream = live;
  await go('/stream');
  await page.getByText('Lost the connection').waitFor({ timeout: 30000 });
  await scan('stream degraded (failed state)');
  stream = { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null };

  await go('/library');
  await page.keyboard.press('Control+k');
  await page.getByPlaceholder('Do something…').waitFor();
  await scan('command menu open');
  await page.keyboard.type('portal');
  await page.getByRole('option', { name: /^Open Portal 2/ }).waitFor();
  await scan('command menu with game entries');
  await page.keyboard.press('Escape');

  await page.getByRole('combobox', { name: 'Sort games' }).click();
  await page.locator('.dd-menu[role="listbox"]').waitFor();
  await scan('sort dropdown open');
  await page.keyboard.press('Escape');

  await page.getByRole('button', { name: 'Quit Portal 2' }).first().click();
  await page.getByRole('dialog').waitFor();
  await scan('quit dialog open');
  await page.keyboard.press('Escape');

  await page.getByRole('button', { name: /^Game 1, / }).first().click();
  await page.getByText('Full controller support').waitFor();
  await scan('game detail panel open');
  await page.getByRole('button', { name: 'Open Game 1 screenshot 2' }).click();
  await page.getByRole('dialog', { name: 'Screenshot 2 of 3' }).waitFor();
  await scan('screenshot lightbox open');
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'View all 9 achievements' }).click();
  await page.getByRole('searchbox', { name: 'Search achievements' }).waitFor();
  await scan('achievements modal open');
  await page.getByRole('searchbox', { name: 'Search achievements' }).fill('zzz');
  await page.getByText('No matches').waitFor();
  await scan('achievements modal, no matches');
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Edit tag Tag teal' }).click();
  await page.getByRole('radiogroup', { name: 'Symbol' }).waitFor();
  await scan('tag editor open');
  await page.getByRole('button', { name: 'Delete tag' }).click();
  await page.getByText('It is on 1 game.').waitFor();
  await scan('tag delete confirm open');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');

  await go('/machines');
  await page.getByRole('button', { name: 'Restart Sunshine' }).click();
  await page.getByRole('dialog').waitFor();
  await scan('restart dialog open');
} catch (e) {
  check('axe-check ran', false, e.stack ?? String(e));
} finally {
  await browser.close();
}
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
