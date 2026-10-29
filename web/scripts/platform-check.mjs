// Platform pass checks (docs/ui-notes.md, Platform features) in headless Chrome against `astro preview` (mocked /api).
//   npx astro preview --host 127.0.0.1 --port 4470 &   then   node scripts/platform-check.mjs [dialog tip vt wake]
// Screenshots go to $OUT.
import { chromium } from 'playwright-core';
import { exe } from './browser.mjs';
import fs from 'node:fs';
import { axe } from './axe.mjs';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4470';
const OUT = process.env.OUT ?? '/tmp/platform-shots';
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
    // the fading backdrop lets clicks through: a chip clicked 30 ms after Esc reacts
    await page.keyboard.press('Control+k');
    await page.waitForSelector('.cm-dim[data-state="open"]');
    await sleep(800);
    const chip = await page.getByRole('button', { name: 'Installed', exact: true }).boundingBox();
    await page.keyboard.press('Escape');
    await sleep(30);
    const under = await page.evaluate(([x, y]) => document.elementFromPoint(x, y)?.className ?? '', [chip.x + chip.width / 2, chip.y + chip.height / 2]);
    await page.mouse.click(chip.x + chip.width / 2, chip.y + chip.height / 2);
    await sleep(600);
    check('backdrop exit: a click 30 ms after Esc reaches the page', String(under).includes('fchip') && (await page.getByRole('button', { name: 'Installed', exact: true }).getAttribute('aria-pressed')) === 'true', `hit ${under}`);
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

  // ---- 2. tooltips: popover + anchor positioning, hover (300 ms) and keyboard focus, Esc, flip at the edge
  if (want('tip')) {
    const tipOf = (page, sel) => page.evaluate((sel) => {
      const el = document.querySelector(sel);
      const tip = document.getElementById(el.getAttribute('aria-describedby') ?? '') ?? el.nextElementSibling;
      const r = el.getBoundingClientRect(), t = tip?.getBoundingClientRect();
      return { described: el.hasAttribute('aria-describedby'), top: Math.round(r.top), tipTop: t && Math.round(t.top), role: tip?.getAttribute('role'), open: !!tip?.matches(':popover-open'), text: tip?.textContent, above: t && t.bottom <= r.top + 1, below: t && t.top >= r.bottom - 1, dx: t && Math.round(t.left + t.width / 2 - (r.left + r.width / 2)) };
    }, sel);
    for (const fallback of [false, true]) {
      const tag = fallback ? 'no anchor positioning: ' : '';
      const { ctx, page, go, errs } = await mk({ init: fallback ? () => { const s = CSS.supports.bind(CSS); CSS.supports = (...a) => !String(a[0]).includes('anchor') && s(...a); } : undefined });
      await go('/settings#general');
      const sw = '.sw[aria-label="pink"]';
      await page.hover(sw);
      await sleep(120);
      check(`${tag}hover: no tip before 300 ms`, !(await tipOf(page, sw)).open);
      await sleep(350);
      let t = await tipOf(page, sw);
      check(`${tag}hover: tip shown above, centred, role=tooltip; not a description (it repeats the swatch's name)`, t.open && t.above && Math.abs(t.dx) <= 2 && t.role === 'tooltip' && t.text === 'pink' && !t.described, JSON.stringify(t));
      if (!fallback) {
        await page.screenshot({ path: `${OUT}/tooltip-hover.png`, clip: { x: 220, y: 60, width: 520, height: 220 } });
        const bad = await axe(page);
        check('axe with a tip open', bad.length === 0, bad.join(' | '));
      }
      await page.mouse.move(5, 890);
      await sleep(250);
      check(`${tag}leave: tip hidden`, !(await tipOf(page, sw)).open);
      await page.focus('.sw[aria-checked="true"]');
      await page.keyboard.press('ArrowRight'); // keyboard focus (focus-visible) on the next swatch
      const cur = '.sw[aria-checked="true"]';
      await sleep(60);
      check(`${tag}keyboard focus: tip shown at once`, (await tipOf(page, cur)).open);
      await page.keyboard.press('Escape');
      await sleep(250);
      check(`${tag}Esc: tip hidden`, !(await tipOf(page, cur)).open);
      if (!fallback) {
        // the running card's Quit button pinned to the top edge: a top tip must flip below it
        await go('/library');
        const q = '.wrap .quit';
        await page.getByRole('button', { name: 'Show all' }).last().click(); // a taller page, so the shelf can reach the top
        await sleep(300);
        await page.evaluate((q) => { const b = document.querySelector(q); window.scrollBy({ top: b.getBoundingClientRect().top - 2, behavior: 'instant' }); }, q);
        const box = await page.locator(q).boundingBox();
        await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2); // not page.hover: it would scroll
        await sleep(450);
        t = await tipOf(page, q);
        check('edge: tip flips below a trigger at the top of the viewport; it describes the Quit button', t.open && t.below && t.described, JSON.stringify(t));
        await page.screenshot({ path: `${OUT}/tooltip-flipped.png`, clip: { x: 220, y: 0, width: 400, height: 140 } });
      }
      check(`${tag}no page errors`, errs.length === 0, errs.join(' | '));
      await ctx.close();
    }
  }

  // ---- 3. Library view transition: chips and search, names only on the open grid, cost while typing
  if (want('vt')) {
    const spy = () => {
      window.__vt = [];
      window.__long = [];
      window.__ev = [];
      new PerformanceObserver((l) => window.__long.push(...l.getEntries().map((e) => e.duration))).observe({ type: 'longtask' });
      new PerformanceObserver((l) => window.__ev.push(...l.getEntries().filter((e) => /key|input/.test(e.name)).map((e) => e.duration))).observe({ type: 'event', durationThreshold: 16 });
      const orig = document.startViewTransition?.bind(document);
      if (!orig || window.__noVT) return void delete Document.prototype.startViewTransition;
      document.startViewTransition = (arg) => {
        const vt = orig(arg);
        const rec = { types: arg?.types, named: document.querySelectorAll('[style*="view-transition-name"]').length, anims: 0 };
        window.__vt.push(rec);
        vt.ready.then(() => (rec.anims = document.getAnimations().filter((a) => a.effect?.pseudoElement?.startsWith('::view-transition')).length), () => {});
        return vt;
      };
    };
    const vts = (page) => page.evaluate(() => window.__vt.map((x) => ({ ...x })));
    const leftNames = (page) => page.evaluate(() => document.querySelectorAll('[style*="view-transition-name"]').length);
    {
      const { ctx, page, go, errs } = await mk({ init: spy });
      await go('/library');
      await page.getByRole('button', { name: 'Installed', exact: true }).click();
      await sleep(80);
      let v = await vts(page);
      check('filter chip: one view transition of type library (no grid open: nothing captured, the root stays live)', v.length === 1 && v[0].types?.[0] === 'library' && v[0].anims === 0, JSON.stringify(v));
      check('filter chip: no names on shelf-row cards', v[0]?.named === 0);
      await sleep(600);
      check('filter chip: names removed after', (await leftNames(page)) === 0);
      await page.getByRole('button', { name: 'All', exact: true }).click();
      await sleep(600);
      await page.getByRole('button', { name: 'Show all' }).first().click(); // Installed as a grid
      await sleep(300);
      await page.evaluate(() => (window.__vt.length = 0));
      await page.getByRole('button', { name: 'Recent', exact: true }).click();
      await sleep(60);
      await page.screenshot({ path: `${OUT}/library-vt-mid.png` });
      v = await vts(page);
      const gridCards = await page.evaluate(() => document.querySelectorAll('.grid [data-gid]').length);
      check('grid: on-screen grid cards named (vt-<appid>), at most 60, and they animate', v.length === 1 && v[0].named > 0 && v[0].named <= 60 && v[0].anims > 0, `anims ${v[0]?.anims}, named ${v[0]?.named}, grid now ${gridCards}`);
      await sleep(600);
      check('grid: names removed after', (await leftNames(page)) === 0);
      await page.getByRole('button', { name: 'All', exact: true }).click();
      await sleep(600);
      await page.evaluate(() => (window.__vt.length = 0));
      await page.getByLabel('Search games').pressSequentially('game 1', { delay: 80 });
      await sleep(600);
      v = await vts(page);
      // search had one (result set changed, once per 300 ms) but the slowest key went from ~40 to 136-168 ms: dropped
      check('search: plain swap, no view transition while typing', v.length === 0, `${v.length} for 6 keys`);
      check('search: names removed after', (await leftNames(page)) === 0);
      check('no page errors', errs.length === 0, errs.join(' | '));
      await ctx.close();
    }
    {
      const { ctx, page, go } = await mk({ init: spy, reducedMotion: 'reduce' });
      await go('/library');
      await page.getByRole('button', { name: 'Installed', exact: true }).click();
      await sleep(100);
      check('reduced motion: no view transition', (await vts(page)).length === 0);
      await ctx.close();
    }
    // A card clicked ~100 ms after a chip click (the open grid's cards still animating) reacts. Control: with the root
    // captured again (pointer-events: none on ::view-transition kept) the same click is swallowed, so the check is real.
    const clickDuring = async (control) => {
      const { ctx, page, go } = await mk({ init: spy });
      await go('/library');
      if (control) await page.addStyleTag({ content: 'html:active-view-transition-type(library) { view-transition-name: root !important; }' });
      await page.getByRole('button', { name: 'Show all' }).first().click(); // Installed as a grid: its cards get names
      await sleep(300);
      await page.getByRole('button', { name: 'Not installed', exact: true }).click();
      await sleep(100);
      const box = await page.evaluate(() => {
        const r = document.querySelector('.g.off').getBoundingClientRect();
        return { x: r.x + r.width / 2, y: r.y + r.height / 2, running: document.getAnimations().some((a) => a.effect?.pseudoElement?.startsWith('::view-transition') && a.playState === 'running') };
      });
      await page.mouse.click(box.x, box.y);
      await sleep(400);
      const r = { running: box.running, expanded: await page.evaluate(() => document.querySelectorAll('[role=dialog]').length === 1) }; // the card opened its panel
      await ctx.close();
      return r;
    };
    const real = await clickDuring(false);
    const ctl = await clickDuring(true);
    check('chip transition: a card clicked 100 ms in (transition running) reacts', real.running && real.expanded, JSON.stringify(real));
    check('chip transition: control with the root captured swallows that click', ctl.running && !ctl.expanded, JSON.stringify(ctl));
    {
      // two chip clicks 50 ms apart: the first is skipped, the second wins, names come off, no errors
      const { ctx, page, go, errs } = await mk({ init: spy });
      await go('/library');
      await page.getByRole('button', { name: 'Show all' }).first().click();
      await sleep(300);
      await page.getByRole('button', { name: 'Recent', exact: true }).click();
      await sleep(50);
      await page.getByRole('button', { name: 'Installed', exact: true }).click({ force: true });
      await sleep(600);
      const pressed = await page.evaluate(() => document.querySelector('.fchip[aria-pressed="true"]')?.textContent);
      check('second chip click during a transition: skipped then replaced, last chip wins, names removed', (await vts(page)).length === 2 && pressed === 'Installed' && (await leftNames(page)) === 0 && errs.length === 0, `${pressed}; ${errs.join(' | ')}`);
      await ctx.close();
    }
    {
      // an update callback that throws: names still come off, no unhandled rejection
      const { ctx, page, go, errs } = await mk({ init: () => { const o = document.startViewTransition.bind(document); document.startViewTransition = (a) => o({ ...a, update: async () => { await a.update(); throw new Error('boom'); } }); } });
      await go('/library');
      await page.getByRole('button', { name: 'Show all' }).first().click();
      await sleep(300);
      await page.getByRole('button', { name: 'Recent', exact: true }).click();
      await sleep(600);
      check('failed update: names removed, no unhandled rejection', (await leftNames(page)) === 0 && errs.length === 0, errs.join(' | '));
      await ctx.close();
    }
    // Typing cost with both grids open (worst case), with and without startViewTransition; 3 runs each, median.
    // Also the filter chip click with both grids open: no long task.
    const typing = async (noVT) => {
      const { ctx, page, go } = await mk({ init: noVT ? () => { window.__noVT = 1; } : undefined });
      await ctx.addInitScript(spy);
      await go('/library');
      for (let i = 0; i < 2; i++) await page.getByRole('button', { name: 'Show all' }).first().click();
      await sleep(400);
      const t0 = await page.evaluate(() => ((window.__long.length = 0), (window.__ev.length = 0), performance.now()));
      await page.getByLabel('Search games').pressSequentially('game 1', { delay: 80 });
      await page.waitForFunction(() => document.querySelectorAll('[data-gid]').length && !document.querySelector('[style*="view-transition-name"]'));
      await sleep(700);
      const r = await page.evaluate((t0) => ({ long: Math.max(0, ...window.__long), ev: Math.max(0, ...window.__ev), evSum: window.__ev.reduce((a, b) => a + b, 0), n: window.__vt.length, cards: document.querySelectorAll('[data-gid]').length }), t0);
      await page.getByLabel('Search games').fill('');
      await sleep(400);
      await page.evaluate(() => (window.__long.length = 0));
      await page.getByRole('button', { name: 'Not installed', exact: true }).click();
      await sleep(800);
      r.chipLong = await page.evaluate(() => Math.max(0, ...window.__long));
      await ctx.close();
      return r;
    };
    const med = (a, k) => a.map((x) => x[k]).sort((x, y) => x - y)[1];
    const runs = { on: [], off: [] };
    for (let i = 0; i < 3; i++) runs.off.push(await typing(true)), runs.on.push(await typing(false));
    const on = { long: med(runs.on, 'long'), ev: med(runs.on, 'ev'), evSum: med(runs.on, 'evSum'), chipLong: med(runs.on, 'chipLong') };
    const off = { long: med(runs.off, 'long'), ev: med(runs.off, 'ev'), evSum: med(runs.off, 'evSum'), chipLong: med(runs.off, 'chipLong') };
    console.log(`     typing "game 1" (6 keys, 80 ms apart, both grids open): with VT ${JSON.stringify(on)} vts=${runs.on.map((r) => r.n)} | without ${JSON.stringify(off)}`);
    // search has no view transition, so with/without run the same code: information only, not an assertion
    console.log(`     (info) slowest key event ${on.ev} vs ${off.ev} ms, longest task ${on.long} vs ${off.long} ms`);
    check('filter chip with both grids open: no long task over 50 ms', on.chipLong < 50, `${on.chipLong} ms (plain swap ${off.chipLong} ms)`);
  }

  // ---- 4. Screen Wake Lock on /stream (stubbed navigator.wakeLock, a fake WebTransport that connects and idles)
  if (want('wake')) {
    const LIVE = { state: 'live', preset: 'balanced', codec: 'h264', appid: 1000, wt: 'https://127.0.0.1:4479/', hash: '00'.repeat(32), started: 1, elapsedS: 0 };
    const fake = (off) => {
      if (off) localStorage.setItem('brisk.settings', JSON.stringify({ keepAwake: false }));
      window.__wl = [];
      window.__vis = 'visible';
      Object.defineProperty(document, 'visibilityState', { get: () => window.__vis });
      Object.defineProperty(document, 'hidden', { get: () => window.__vis !== 'visible' });
      Object.defineProperty(navigator, 'wakeLock', { value: { request: async (type) => { const s = { released: false, release: async () => { s.released = true; window.__wl.push('release'); } }; window.__wl.push(`request:${type}`); window.__lock = s; return s; } } });
      const never = () => new ReadableStream({});
      window.WebTransport = class {
        constructor() { this.ready = Promise.resolve(); this.closed = new Promise((r) => (this.end = r)); this.datagrams = { readable: never(), writable: new WritableStream() }; this.incomingUnidirectionalStreams = never(); }
        createBidirectionalStream() { return new Promise(() => {}); }
        createUnidirectionalStream() { return new Promise(() => {}); }
        close() { this.end?.(); }
      };
    };
    const wl = (page) => page.evaluate(() => window.__wl.join(' '));
    const js = new Map();
    {
      const { ctx, page, go, errs } = await mk({ stream: () => LIVE, init: fake });
      page.on('response', async (r) => { if (/\.js(\?|$)/.test(r.url())) js.set(r.url(), (await r.body().catch(() => Buffer.alloc(0))).length); });
      await go('/stream');
      await page.waitForSelector('.stream[data-state="live"]', { timeout: 10000 });
      await sleep(200);
      const total = [...js.values()].reduce((a, b) => a + b, 0);
      const names = [...js.keys()].map((u) => u.split('/').pop());
      console.log(`     /stream (live) JS: ${names.length} files, ${total} bytes uncompressed: ${names.join(' ')}`);
      check('/stream loads no heavy chunks (anime, layerchart, mediabunny, charts)', !names.some((n) => /anime|layerchart|Charts|mediabunny|motion\./i.test(n)));
      check('live: one screen lock requested', (await wl(page)) === 'request:screen', await wl(page));
      await page.evaluate(() => ((window.__lock.released = true), (window.__vis = 'hidden'), document.dispatchEvent(new Event('visibilitychange'))));
      await sleep(100);
      check('hidden: nothing asked while hidden', (await wl(page)) === 'request:screen', await wl(page));
      await page.evaluate(() => ((window.__vis = 'visible'), document.dispatchEvent(new Event('visibilitychange'))));
      await sleep(100);
      check('visible again: lock asked again', (await wl(page)) === 'request:screen request:screen', await wl(page));
      await page.evaluate(() => (window.__wl.length = 0));
      await page.mouse.move(700, 2); // ghost bar
      await page.getByRole('button', { name: 'Disconnect' }).click();
      await page.waitForURL('**/library');
      await sleep(200);
      check('disconnect: lock released', (await wl(page)) === 'release', await wl(page));
      check('no page errors', errs.length === 0, errs.join(' | '));
      await ctx.close();
    }
    {
      const { ctx, page, go } = await mk({ stream: () => LIVE, init: fake });
      await ctx.addInitScript(() => localStorage.setItem('brisk.settings', JSON.stringify({ keepAwake: false })));
      await go('/stream');
      await page.waitForSelector('.stream[data-state="live"]', { timeout: 10000 });
      await sleep(200);
      check('setting off: no lock requested', (await wl(page)) === '', await wl(page));
      await ctx.close();
    }
    {
      const { ctx, page, go } = await mk({ stream: () => ({ ...LIVE, wt: null }), init: fake }); // failed state
      await go('/stream');
      await page.waitForSelector('.stream[data-state="failed"]', { timeout: 10000 });
      check('failed stream: no lock', (await wl(page)) === '', await wl(page));
      await ctx.close();
    }
  }
} catch (e) {
  check('platform-check ran', false, e.stack ?? String(e));
} finally {
  await browser.close();
}
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
