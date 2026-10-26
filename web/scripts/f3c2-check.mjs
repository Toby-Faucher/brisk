// Motion pass 2, F3c2 (#14): a History session's cover grows into the detail header (shared-element view transition,
// ui/motion.ts morph) and shrinks back. Assertions + seeked screenshots in headless Chrome against `astro preview`
// (mocked /api, own port), killed by PID:
//   npx astro preview --host 127.0.0.1 --port 4492 &   then   node scripts/f3c2-check.mjs [section...]
// Sections: morph back pop rapid during fail reduce nav stream axe. Screenshots go to $OUT.
import { chromium } from 'playwright-core';
import fs from 'node:fs';
import path from 'node:path';
import { axe } from './axe.mjs';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4492';
const OUT = process.env.OUT ?? '/tmp/claude-1000/-home-toby-Projects/94a1bf04-5503-4de5-ba94-79e19e0c32dd/scratchpad/f3c2';
const exe = process.env.BRISK_CHROMIUM ?? `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const only = process.argv.slice(2);
const want = (s) => !only.length || only.includes(s);
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x !== '' ? ` (${x})` : ''}`));

// game 1000..1005: covers served below; 1004's cover fails to load, 1005 has none
const games = Array.from({ length: 6 }, (_, i) => ({ appid: 1000 + i, name: `Game ${i}`, installed: true, sizeBytes: 1e9, lastPlayed: 1700000000 - i * 1e5, playtimeMin: 90, cover: i === 5 ? '' : `/covers/${1000 + i}.svg`, running: false }));
const T0 = 1700000000000;
const sessions = Array.from({ length: 12 }, (_, i) => ({ appid: 1000 + (i % 6), name: `Game ${i % 6}`, started: T0 - i * 864e5, ended: T0 - i * 864e5 + 3600e3, host: 'bawn', preset: 'balanced', codec: 'hevc', bitrateKbps: 40000, stats: null, spark: [3, 5, 4, 6, 5] }));
const series = Array.from({ length: 20 }, (_, i) => [i * 5, 8 + (i % 4), 60, 38 + (i % 3)]);
const svg = (h) => `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="450"><defs><linearGradient id="g" x2="1" y2="1"><stop offset="0" stop-color="hsl(${h} 70% 55%)"/><stop offset="1" stop-color="hsl(${(h + 60) % 360} 60% 25%)"/></linearGradient></defs><rect width="300" height="450" fill="url(#g)"/><circle cx="150" cy="180" r="80" fill="#fff" opacity=".35"/><text x="150" y="380" font-size="64" text-anchor="middle" fill="#fff" font-family="sans-serif">G</text></svg>`;

const browser = await chromium.launch({ executablePath: exe });
async function mk(opts = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 860 }, ...opts });
  await ctx.route('**/covers/*.svg', (r) => {
    const id = Number(/(\d+)\.svg/.exec(r.request().url())[1]);
    return id === 1004 ? r.abort() : r.fulfill({ contentType: 'image/svg+xml', body: svg((id * 47) % 360) });
  });
  await ctx.route('**/api/**', async (r) => {
    const p = new URL(r.request().url()).pathname;
    const one = /^\/api\/sessions\/(\d+)$/.exec(p);
    const json =
      p === '/api/games' ? { games, ownedAvailable: true }
      : p === '/api/host' ? { name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null }
      : p === '/api/stream' ? { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null }
      : p === '/api/prefs' ? { prefs: {} }
      : p === '/api/sessions' ? { sessions }
      : one ? { session: sessions.find((s) => s.started === +one[1]), series }
      : {};
    await r.fulfill({ json });
  });
  await ctx.addInitScript(() => {
    window.__cls = 0;
    new PerformanceObserver((l) => l.getEntries().forEach((e) => !e.hadRecentInput && (window.__cls += e.value))).observe({ type: 'layout-shift', buffered: true });
    window.__vt = [];
    const orig = document.startViewTransition?.bind(document);
    if (!orig) return;
    document.startViewTransition = (arg) => {
      const vt = orig(arg);
      const rec = { types: arg?.types, named: [...document.querySelectorAll('[style*="view-transition-name"]')].map((e) => e.dataset.cover ?? e.tagName), pseudo: [] , skipped: false };
      window.__vt.push(rec);
      vt.ready.then(() => (rec.pseudo = [...new Set(document.getAnimations().map((a) => a.effect?.pseudoElement).filter(Boolean))]), () => {});
      vt.finished.then(() => {}, () => {});
      return vt;
    };
  });
  const page = await ctx.newPage();
  const errs = [];
  page.on('console', (m) => m.type() === 'error' && !/Failed to load resource|ERR_FAILED/.test(m.text()) && errs.push(m.text()));
  page.on('pageerror', (e) => errs.push(String(e)));
  return { ctx, page, errs };
}
const vts = (page) => page.evaluate(() => window.__vt.map((x) => ({ ...x })));
const leftNames = (page) => page.evaluate(() => document.querySelectorAll('[style*="view-transition-name"], [style=""]').length);
const vtAnims = (page) => page.evaluate(() => document.getAnimations().filter((a) => a.effect?.pseudoElement?.startsWith('::view-transition')).length);
const seek = (page, t) => page.evaluate((t) => document.getAnimations().filter((a) => a.effect?.pseudoElement?.startsWith('::view-transition')).forEach((a) => { a.pause(); a.currentTime = t; }), t);
const play = (page) => page.evaluate(() => document.getAnimations().forEach((a) => a.play()));
const focused = (page) => page.evaluate(() => { const a = document.activeElement; return a?.id || a?.dataset?.session || a?.tagName; });
const boxOf = (page, sel) => page.evaluate((sel) => { const r = document.querySelector(sel)?.getBoundingClientRect(); return r && { x: Math.round(r.x), y: Math.round(r.y), w: Math.round(r.width), h: Math.round(r.height) }; }, sel);
const groupBox = (page) => page.evaluate(() => { const a = document.getAnimations().find((a) => a.effect?.pseudoElement === '::view-transition-group(session-cover)'); return a ? getComputedStyle(document.documentElement, '::view-transition-group(session-cover)').width : null; });
async function list(page) {
  await page.goto(BASE + '/history'); await page.waitForSelector('.grid [data-cover]'); await sleep(1100);
  await page.mouse.move(5, 5);
}
const S = (i) => sessions[i].started;

if (want('morph')) {
  const { page, ctx, errs } = await mk({ deviceScaleFactor: 2 });
  await list(page);
  const from = await boxOf(page, `[data-cover="${S(1)}"]`);
  await page.evaluate(() => (window.__cls = 0));
  await page.locator(`a.fill[data-session="${S(1)}"]`).click();
  await page.waitForFunction(() => document.getAnimations().some((a) => a.effect?.pseudoElement?.startsWith('::view-transition-group')));
  const v = await vts(page);
  check('open: one view transition, type morph', v.length === 1 && v[0].types?.join() === 'morph', JSON.stringify(v));
  check('open: only the clicked cover was named before', v[0]?.named.join() === String(S(1)), v[0]?.named.join());
  check('open: only the session-cover pseudos exist (root not captured)', v[0]?.pseudo.length > 0 && v[0].pseudo.every((p) => /\(session-cover\)$/.test(p)), v[0]?.pseudo.join(' '));
  check('open: URL and heading switched, focus on the detail heading', (await page.evaluate(() => location.search)) === `?session=${S(1)}` && (await focused(page)) === 'session-title');
  for (const t of [0, 60, 140, 260]) {
    await seek(page, t);
    await page.screenshot({ path: path.join(OUT, `open-t${t}.png`), clip: { x: 200, y: 0, width: 1080, height: 420 } });
  }
  check('open: at t=0 the group sits on the list cover', (await groupBox(page)) !== null && (await seek(page, 0), Math.abs(parseFloat(await groupBox(page)) - from.w) <= 1), `${await groupBox(page)} vs ${from.w}px`);
  await play(page); await sleep(900);
  const to = await boxOf(page, '.hcover');
  check('open: ends with the 112 px cover in the header, no animation, no name or empty style left', to?.w === 112 && (await vtAnims(page)) === 0 && (await leftNames(page)) === 0, JSON.stringify(to));
  await page.screenshot({ path: path.join(OUT, 'open-end.png'), clip: { x: 200, y: 0, width: 1080, height: 420 } });
  check('open: no layout shift', (await page.evaluate(() => window.__cls)) < 0.001, String(await page.evaluate(() => window.__cls)));
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('back')) {
  const { page, ctx, errs } = await mk({ deviceScaleFactor: 2 });
  await list(page);
  // a card far down: back must bring it into view and land the cover there
  await page.locator(`a.fill[data-session="${S(10)}"]`).scrollIntoViewIfNeeded();
  await page.locator(`a.fill[data-session="${S(10)}"]`).click(); await sleep(1000);
  await page.evaluate(() => (window.__vt.length = 0));
  await page.getByRole('button', { name: 'History' }).click();
  await page.waitForFunction(() => document.getAnimations().some((a) => a.effect?.pseudoElement?.startsWith('::view-transition-group')));
  const v = await vts(page);
  check('back button: reverse morph, only the header cover named before', v.length === 1 && v[0].named.join() === String(S(10)), JSON.stringify(v));
  for (const t of [0, 100, 260]) {
    await seek(page, t);
    await page.screenshot({ path: path.join(OUT, `back-t${t}.png`) });
  }
  await play(page); await sleep(900);
  check('back button: URL /history, focus back on the session link, which is in view', (await page.evaluate(() => location.search)) === '' && (await focused(page)) === String(S(10)) && await page.locator(`a.fill[data-session="${S(10)}"]`).evaluate((a) => { const r = a.getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight; }));
  check('back button: names gone', (await leftNames(page)) === 0);
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('pop')) {
  const { page, ctx, errs } = await mk();
  await list(page);
  await page.locator(`a.fill[data-session="${S(2)}"]`).click(); await sleep(900);
  await page.evaluate(() => (window.__vt.length = 0));
  await page.goBack(); await sleep(60);
  let v = await vts(page);
  check('browser back: reverse morph of that cover', v.length === 1 && v[0].named.join() === String(S(2)), JSON.stringify(v));
  await sleep(900);
  check('browser back: list, focus on the link', (await page.$('.grid')) !== null && (await focused(page)) === String(S(2)));
  await page.goForward(); await sleep(60);
  v = await vts(page);
  check('browser forward: morph into the header again', v.length === 2 && v[1].named.join() === String(S(2)), JSON.stringify(v));
  await sleep(900);
  check('browser forward: detail, focus on the heading, no names left', (await focused(page)) === 'session-title' && (await leftNames(page)) === 0);
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('rapid')) {
  const { page, ctx, errs } = await mk();
  await list(page);
  // open, back during the flight, open again, back: each one skips the last; it ends consistent
  await page.locator(`a.fill[data-session="${S(0)}"]`).click(); await sleep(50);
  await page.goBack(); await sleep(40);
  await page.goForward(); await sleep(30);
  await page.goBack(); await sleep(20);
  await page.goForward();
  await sleep(1000);
  check('rapid: ends on the detail of the last target, heading focused', (await page.evaluate(() => location.search)) === `?session=${S(0)}` && (await page.$('#session-title')) !== null && (await page.$('.grid')) === null && (await focused(page)) === 'session-title');
  check('rapid: no animations, names or empty styles left', (await vtAnims(page)) === 0 && (await leftNames(page)) === 0);
  check('rapid: never two names at once', (await vts(page)).every((x) => x.named.length <= 1), JSON.stringify(await vts(page)));
  // double click on a card
  await page.goBack(); await sleep(900);
  await page.locator(`a.fill[data-session="${S(3)}"]`).dblclick(); await sleep(1000);
  check('double click: one detail, URL pushed once', (await page.evaluate(() => [location.search, history.length])).join() !== '' && (await page.evaluate(() => location.search)) === `?session=${S(3)}` && (await leftNames(page)) === 0);
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('during')) {
  // the page stays live while the cover flies: the back button clicked 80 ms in reacts (and reverses from where it is)
  const { page, ctx, errs } = await mk();
  await list(page);
  await page.locator(`a.fill[data-session="${S(1)}"]`).click(); await sleep(80);
  check('during: transition still running', (await vtAnims(page)) > 0);
  await page.getByRole('button', { name: 'History' }).click({ timeout: 1000 }); await sleep(40);
  check('during: the click reached the back button (not swallowed)', (await page.evaluate(() => location.search)) === '' && (await vts(page)).length === 2);
  await page.screenshot({ path: path.join(OUT, 'during-back.png') });
  await sleep(900);
  check('during: list again, names gone', (await page.$('.grid')) !== null && (await leftNames(page)) === 0);
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('fail')) {
  const { page, ctx, errs } = await mk({ deviceScaleFactor: 2 });
  await list(page);
  check('fail: broken cover (1004) and missing cover (1005) are placeholders in the list', await page.evaluate((ids) => ids.every((id) => document.querySelector(`[data-cover="${id}"]`)?.tagName === 'DIV'), [S(4), S(5)]));
  for (const i of [4, 5]) {
    await page.evaluate(() => (window.__vt.length = 0));
    await page.locator(`a.fill[data-session="${S(i)}"]`).click(); await sleep(70);
    await seek(page, 80);
    await page.screenshot({ path: path.join(OUT, `placeholder-${i}-t80.png`), clip: { x: 200, y: 0, width: 1080, height: 420 } });
    await play(page); await sleep(900);
    check(`fail ${i}: the placeholder morphs too and the header has the 112 px placeholder`, (await vts(page)).length === 1 && (await page.evaluate(() => document.querySelector('.hcover').tagName)) === 'DIV' && (await boxOf(page, '.hcover')).w === 112);
    await page.getByRole('button', { name: 'History' }).click(); await sleep(900);
  }
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('reduce')) {
  const { page, ctx, errs } = await mk({ reducedMotion: 'reduce' });
  await list(page);
  await page.locator(`a.fill[data-session="${S(1)}"]`).click(); await sleep(30);
  check('reduced motion: no view transition, the detail is there at once, heading focused', (await vts(page)).length === 0 && (await page.$('#session-title')) !== null && (await focused(page)) === 'session-title');
  await page.getByRole('button', { name: 'History' }).click(); await sleep(30);
  check('reduced motion: back instant too, focus on the link', (await vts(page)).length === 0 && (await focused(page)) === String(S(1)));
  await ctx.close();
  // switched on mid-flight: it ends at once
  const b = await mk();
  await list(b.page);
  await b.page.locator(`a.fill[data-session="${S(1)}"]`).click(); await sleep(50);
  await b.page.emulateMedia({ reducedMotion: 'reduce' }); await sleep(80);
  check('reduced motion switched on mid-flight: the transition is over', (await vtAnims(b.page)) === 0 && (await leftNames(b.page)) === 0);
  check('no errors', errs.length === 0 && b.errs.length === 0, [...errs, ...b.errs].join('|'));
  await b.ctx.close();
}

if (want('nav')) {
  // router navigation mid-flight (History destroyed: skipMorph), then back to /history: nothing left behind
  const { page, ctx, errs } = await mk();
  await list(page);
  await page.locator(`a.fill[data-session="${S(1)}"]`).click(); await sleep(60);
  await page.click('aside a.nav[href="/machines"]');
  await page.waitForFunction(() => location.pathname === '/machines'); await sleep(800);
  check('nav: on /machines, no names, no transition left', (await leftNames(page)) === 0 && (await vtAnims(page)) === 0);
  await page.click('aside a.nav[href="/history"]'); await page.waitForSelector('.grid [data-cover]'); await sleep(900);
  await page.evaluate(() => (window.__vt.length = 0));
  await page.locator(`a.fill[data-session="${S(2)}"]`).click(); await sleep(900);
  check('nav: back on /history the morph works again', (await vts(page)).length === 1 && (await leftNames(page)) === 0);
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('cross')) {
  // browser back/forward across pages is still the router's: /machines -> /history -> session -> back x2 -> forward x2
  const { page, ctx, errs } = await mk();
  await page.goto(BASE + '/machines'); await sleep(900);
  await page.click('aside a.nav[href="/history"]'); await page.waitForSelector('.grid [data-cover]'); await sleep(900);
  await page.locator(`a.fill[data-session="${S(1)}"]`).click(); await sleep(900);
  await page.goBack(); await sleep(900);
  check('cross: back to the list entry stays in-page (list shown)', (await page.$('.grid')) !== null && (await page.evaluate(() => location.pathname)) === '/history');
  await page.goBack(); await sleep(1200);
  check('cross: back again leaves for /machines (router swap)', (await page.evaluate(() => location.pathname)) === '/machines' && (await page.$('.grid [data-cover]')) === null && (await page.$('main')) !== null);
  await page.goForward(); await sleep(1200);
  check('cross: forward returns to the History list', (await page.evaluate(() => location.pathname)) === '/history' && (await page.$('.grid [data-cover]')) !== null);
  await page.goForward(); await sleep(900);
  check('cross: forward again opens the session in-page', (await page.evaluate(() => location.search)) === `?session=${S(1)}` && (await page.$('#session-title')) !== null);
  await page.click('aside a.nav[href="/machines"]'); await page.waitForFunction(() => location.pathname === '/machines'); await sleep(900);
  await page.goBack(); await sleep(1200);
  // before F3c2 our entries had null state and the router ignored this: the Machines page stayed under the session URL
  check('cross: a session left for /machines, then browser back, shows that session again', await page.evaluate((s) => location.search === `?session=${s}` && !!document.querySelector('#session-title') && !document.querySelector('.grid .dotw'), S(1)));
  await page.goBack(); await sleep(900);
  check('cross: and back once more is the list, in-page', (await page.$('.grid [data-cover]')) !== null && (await page.evaluate(() => location.search)) === '');
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('stream')) {
  const { page, ctx, errs } = await mk();
  await page.goto(BASE + '/stream'); await sleep(1200);
  check('stream: no view transition and no script animation', (await vts(page)).length === 0 && (await page.evaluate(() => document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition)).length)) === 0);
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

if (want('axe')) {
  const { page, ctx } = await mk();
  await list(page);
  let a = await axe(page);
  check('axe: list clean', a.length === 0, a.join(' | '));
  await page.locator(`a.fill[data-session="${S(1)}"]`).click(); await sleep(1000);
  a = await axe(page);
  check('axe: detail clean', a.length === 0, a.join(' | '));
  check('a11y: the cover link is a real link with a name and href', await page.goBack().then(() => sleep(900)).then(() => page.locator(`a.fill[data-session="${S(1)}"]`).evaluate((e) => e.tagName === 'A' && e.getAttribute('href') === `/history?session=${e.dataset.session}` && !!e.getAttribute('aria-label'))));
  await ctx.close();
}

await browser.close();
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
