// Platform pass checks (docs/ui-notes.md, Platform features) in headless Chrome against `astro preview` (mocked /api).
//   npx astro preview --host 127.0.0.1 --port 4470 &   then   node scripts/platform-check.mjs [dialog tip vt wake]
// Screenshots go to $OUT.
import { chromium } from 'playwright-core';
import fs from 'node:fs';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4470';
const OUT = process.env.OUT ?? '/tmp/platform-shots';
const exe = process.env.BRISK_CHROMIUM ?? `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const only = process.argv.slice(2);
const want = (s) => !only.length || only.includes(s);
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x !== '' ? ` (${x})` : ''}`));

const games = Array.from({ length: 139 }, (_, i) => ({ appid: 1000 + i, name: i === 0 ? 'Portal 2' : `Game ${i}`, installed: i < 40, sizeBytes: 1e9, lastPlayed: i < 5 ? 1700000000 - i * 1e5 : null, playtimeMin: 90, cover: '', running: i === 0 }));
const sessions = Array.from({ length: 15 }, (_, i) => ({ appid: 1000, name: 'Portal 2', started: 1700000000000 + i * 1e6, ended: 1700000600000 + i * 1e6, host: 'bawn', preset: 'balanced', codec: 'h264', bitrateKbps: 20000, stats: { gate: 'pass' }, spark: [1, 3, 2, 4, 3] }));
const host = { name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null };
const IDLE = { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null };

const browser = await chromium.launch({ executablePath: exe });
async function mk({ stream = () => IDLE, init, ...opts } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  const urls = [];
  await ctx.route('**/api/**', async (r) => {
    const p = new URL(r.request().url()).pathname;
    const json =
      p === '/api/games' ? { games, ownedAvailable: true }
      : p === '/api/host' ? host
      : p === '/api/stream' ? stream()
      : p === '/api/prefs' ? { prefs: { 1000: { preset: 'crisp', codec: 'h264', updated: 1 } } }
      : p === '/api/sessions' ? { sessions }
      : p.startsWith('/api/sessions/') ? { session: sessions[0], series: [] }
      : {};
    await r.fulfill({ json });
  });
  if (init) await ctx.addInitScript(init);
  const page = await ctx.newPage();
  const errs = [];
  page.on('console', (m) => m.type() === 'error' && errs.push(m.text()));
  page.on('pageerror', (e) => errs.push(String(e)));
  page.on('request', (r) => urls.push(r.url()));
  const go = async (path) => (await page.goto(BASE + path), await page.waitForLoadState('networkidle'), await sleep(500));
  return { ctx, page, errs, urls, go };
}

try {
  // ---- 1. dialogs: the exit plays (node still there mid-exit, gone after), focus still returns
  if (want('dialog')) {
    const { ctx, page, go } = await mk();
    const exit = async (name, open, sel, shot) => {
      await open();
      await page.waitForSelector(`${sel}[data-state="open"]`);
      await sleep(800); // entrance done
      const t0 = Date.now();
      await page.keyboard.press('Escape');
      await sleep(50);
      const mid = await page.evaluate((s) => { const el = document.querySelector(s); return el && { ending: el.hasAttribute('data-ending-style'), op: +getComputedStyle(el).opacity, anims: el.getAnimations().length }; }, sel);
      if (shot) await page.screenshot({ path: `${OUT}/${shot}` });
      check(`${name}: still mounted 50 ms after Esc, fading out`, !!mid && mid.ending && mid.anims > 0 && mid.op < 1 && mid.op > 0, JSON.stringify(mid));
      await page.waitForSelector(sel, { state: 'detached', timeout: 2000 });
      check(`${name}: unmounted after the exit (< 400 ms)`, Date.now() - t0 < 400, `${Date.now() - t0} ms`);
    };
    await go('/library');
    await exit('command menu', () => page.keyboard.press('Control+k'), '.cm', 'dialog-exit-command-menu.png');
    const quit = page.getByRole('button', { name: 'Quit Portal 2' }).first();
    await exit('quit dialog', () => quit.click(), '.rs', 'dialog-exit-quit.png');
    check('quit dialog: focus back on its Quit button', await quit.evaluate((b) => b === document.activeElement));
    await go('/machines');
    await exit('restart dialog', () => page.getByRole('button', { name: 'Restart Sunshine' }).click(), '.rs');
    check('restart dialog: focus back on its trigger', await page.getByRole('button', { name: 'Restart Sunshine' }).evaluate((b) => b === document.activeElement));
    await ctx.close();

    const r = await mk({ reducedMotion: 'reduce' });
    await r.go('/library');
    await r.page.keyboard.press('Control+k');
    await r.page.waitForSelector('.cm[data-state="open"]');
    check('reduced motion: command menu has no transition', await r.page.evaluate(() => document.querySelector('.cm').getAnimations().length === 0));
    await r.page.keyboard.press('Escape');
    await sleep(30);
    check('reduced motion: command menu gone at once', await r.page.evaluate(() => !document.querySelector('.cm')));
    await r.ctx.close();
  }
} catch (e) {
  check('platform-check ran', false, e.stack ?? String(e));
} finally {
  await browser.close();
}
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
