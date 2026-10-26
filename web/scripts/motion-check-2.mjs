// Load-entrance assertions + screenshots in headless Chrome against `astro preview` (mocked /api, no hostd).
//   npx astro preview --host 127.0.0.1 --port 4422 &   then   node scripts/motion-check-2.mjs
// Library's /api/games is delayed by 700 ms to reproduce skeleton -> data. Animations are recorded from document start.
import { chromium } from 'playwright-core';
import fs from 'node:fs';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4422';
const OUT = process.env.OUT ?? '/tmp/motion2-shots';
const exe = process.env.BRISK_CHROMIUM ?? `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x !== '' ? ` (${x})` : ''}`));

const games = Array.from({ length: 139 }, (_, i) => ({ appid: 1000 + i, name: i === 0 ? 'Portal 2' : `Game ${i}`, installed: i < 40, sizeBytes: 1e9, lastPlayed: i < 5 ? 1700000000 - i * 1e5 : null, playtimeMin: 90, cover: '', running: false }));
const sessions = Array.from({ length: 15 }, (_, i) => ({ appid: 1000, name: 'Portal 2', started: 1700000000000 + i * 1e6, ended: 1700000600000 + i * 1e6, host: 'bawn', preset: 'balanced', codec: 'h264', bitrateKbps: 20000, stats: null, spark: [1, 3, 2, 4, 3] }));
const host = { name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null };
const mock = (url) => {
  const p = new URL(url).pathname;
  if (p === '/api/games') return { games, ownedAvailable: true };
  if (p === '/api/host') return host;
  if (p === '/api/stream') return { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null };
  if (p === '/api/prefs') return { prefs: {} };
  if (p === '/api/sessions') return { sessions };
  if (p.startsWith('/api/sessions/')) return { session: sessions[0], series: [] };
  return {};
};

const browser = await chromium.launch({ executablePath: exe });
async function mk({ pause = false, delayGames = 0, ...opts } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  await ctx.route('**/api/**', async (r) => {
    if (delayGames && new URL(r.request().url()).pathname === '/api/games') await sleep(delayGames);
    await r.fulfill({ json: mock(r.request().url()) });
  });
  await ctx.addInitScript((pause) => {
    window.__seen = []; window.__max = 0; window.__cls = 0; window.__t0 = performance.now(); window.__held = [];
    const desc = (t) => `${t.tagName.toLowerCase()}.${String(t.className?.baseVal ?? t.className).split(' ')[0]}${t.tagName === 'H4' || t.tagName === 'H2' ? ':' + t.textContent.trim().slice(0, 12) : ''}`;
    const tick = () => {
      const a = document.getAnimations();
      window.__max = Math.max(window.__max, a.length);
      for (const x of a) {
        const t = x.effect?.target; if (!t || x.__rec) continue; x.__rec = 1;
        const tm = x.effect.getTiming(); const kf = x.effect.getKeyframes();
        window.__seen.push({ at: Math.round(performance.now() - window.__t0), name: x.animationName, el: desc(t), main: !!t.closest('main'), side: !!t.closest('aside'), delay: tm.delay, dur: tm.duration, i: t.style?.getPropertyValue('--i'), props: [...new Set(kf.flatMap((k) => Object.keys(k)))].filter((k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k)).sort().join(), from: kf[0]?.opacity, tl: x.timeline?.constructor?.name });
        if (pause && /^(enter-rise|fade-in)$/.test(x.animationName)) { x.pause(); window.__held.push(x); }
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    try { new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value; }).observe({ type: 'layout-shift', buffered: true }); } catch {}
  }, pause);
  const page = await ctx.newPage();
  const errs = [];
  page.on('console', (m) => m.type() === 'error' && errs.push(m.text()));
  page.on('pageerror', (e) => errs.push(String(e)));
  return { ctx, page, errs };
}
const seen = (page) => page.evaluate(() => window.__seen.map((s) => ({ ...s })));
const reset = (page) => page.evaluate(() => (window.__seen.length = 0));
const enters = (s) => s.filter((x) => x.name === 'enter-rise');
const list = (s) => enters(s).map((x) => `${x.el}@${x.delay}`).slice(0, 14).join(' ');
const ROUTES = [['home', '/'], ['library', '/library'], ['history', '/history'], ['machines', '/machines'], ['settings', '/settings']];
const maxes = {};

// ---- reload of each route
{
  for (const [n, p] of ROUTES) {
    const { page, errs } = await mk({ delayGames: n === 'library' ? 700 : 0 });
    await page.goto(BASE + p);
    if (n === 'library') {
      await sleep(450);
      let s = await seen(page);
      check('library: skeleton phase (t~450ms) has no hero/shelf/card entrance yet', !enters(s).some((x) => /^(div\.hero|h4|div\.slot)/.test(x.el)), list(s));
      check('library: skeleton is showing', await page.locator('.skel').count() === 1);
      await page.waitForSelector('.hero'); const landed = await page.evaluate(() => Math.round(performance.now() - window.__t0));
      await sleep(200);
      s = await seen(page);
      const hero = enters(s).find((x) => x.el.startsWith('div.hero'));
      check('library: hero gets an entrance when data lands (not at load)', !!hero && hero.at > 600, `hero first seen ${hero?.at}ms, hero in DOM by ${landed}ms`);
      check('library: shelf headers + first cards get it too', enters(s).filter((x) => x.el.startsWith('h4')).length === 2 && enters(s).filter((x) => x.el.startsWith('div.slot')).length >= 6, enters(s).filter((x) => x.el.startsWith('div.slot')).length + ' cards');
      check('library: cards animate only after data (all entrances after 600ms except the page chrome)', enters(s).filter((x) => /^(div\.hero|h4|div\.slot)/.test(x.el)).every((x) => x.at > 600));
    }
    // Home holds its grid hidden until the saved card order is in (F3c1); its entrance starts when it shows
    if (n === 'home') await page.waitForSelector('.bento:not([data-hold])');
    await sleep(1400);
    const s = await seen(page);
    const e = enters(s).filter((x) => x.main);
    const delays = e.map((x) => x.delay);
    console.log(`  ${n}: ${e.length} entrances: ${list(s)}`);
    const uniq = [...new Set(delays)].sort((a, b) => a - b);
    check(`${n}: >=3 above-the-fold items have a time-based entrance, 420ms, opacity+translate, from opacity 0`, e.length >= (n === 'machines' ? 2 : 3) && e.every((x) => x.dur === 420 && x.props === 'opacity,translate' && x.from === '0' && x.tl === 'DocumentTimeline'), JSON.stringify(e[0]));
    check(`${n}: delays are multiples of 55ms, increasing, capped at 440`, uniq.every((d, k) => d === k * 55 && d <= 440) && uniq.length >= (n === 'machines' ? 1 : 2) && Math.max(...delays) <= 440, uniq.join(','));
    const m = s.find((x) => x.el.startsWith('main') && x.name === 'fade-in');
    check(`${n}: <main> is an opacity-only 200ms fade`, m && m.dur === 200 && m.props === 'opacity', JSON.stringify(m));
    check(`${n}: nothing in the sidebar animates`, !s.some((x) => x.side && /enter|fade|rise/.test(x.name)));
    check(`${n}: nothing is left hidden after the entrance (opacity 1, translate none, no pending flag)`, await page.evaluate(() => !document.documentElement.hasAttribute('data-enter-pending') && [...document.querySelectorAll('[data-enter]:not(.soon)')].every((e) => { const c = getComputedStyle(e); return c.opacity === '1' && c.translate === 'none'; })));
    check(`${n}: CLS`, true, (await page.evaluate(() => window.__cls)).toFixed(3));
    maxes[n] = await page.evaluate(() => window.__max);
    check(`${n}: no errors`, errs.length === 0, errs.slice(0, 2).join('|'));
    await page.context().close();
  }
}

// ---- below-the-fold stays on the view() timeline (History, short viewport)
{
  const { page } = await mk({ viewport: { width: 640, height: 420 } });
  await page.goto(BASE + '/history'); await sleep(1500);
  const r = await page.evaluate(() => [...document.querySelectorAll('main .reveal')].map((e) => ({ tagged: e.hasAttribute('data-enter'), top: Math.round(e.getBoundingClientRect().top), tl: getComputedStyle(e).animationTimeline, name: getComputedStyle(e).animationName, op: getComputedStyle(e).opacity })));
  const above = r.filter((x) => x.tagged), below = r.filter((x) => !x.tagged);
  check('history 420px: in-view items tagged, below-fold items not', above.length >= 2 && below.length >= 4 && above.every((x) => x.top < 420) && below.every((x) => x.top >= 380), `${above.length} tagged / ${below.length} untagged`);
  check('history 420px: untagged keep animation-timeline view() + rise-scroll and are still transparent', below.every((x) => /view/.test(x.tl) && x.name === 'rise-scroll' && +x.op < 1), JSON.stringify(below[0]));
  check('history 420px: tagged ones run on the document timeline (enter-rise)', above.every((x) => x.name === 'enter-rise' && !/view/.test(x.tl)), JSON.stringify(above[0]));
  await page.locator('main .reveal').last().scrollIntoViewIfNeeded(); await sleep(1200);
  check('history 420px: scrolling to the last card reveals it (opacity 1)', await page.locator('main .reveal').last().evaluate((e) => getComputedStyle(e).opacity === '1'));
  await page.context().close();
}

// ---- library: no re-fire on shelf scroll / chevrons / search; client navigation entrance
{
  const { page, errs } = await mk({ delayGames: 700 });
  await page.goto(BASE + '/library'); await page.waitForSelector('.hero'); await sleep(1600);
  await reset(page);
  await page.getByRole('button', { name: /Scroll Installed right/ }).click(); await sleep(900);
  await page.getByRole('button', { name: /Scroll Installed right/ }).click(); await sleep(900);
  await page.locator('.row').first().evaluate((e) => (e.scrollLeft += 1500)); await sleep(600);
  await page.getByRole('button', { name: /Scroll Installed left/ }).click(); await sleep(900);
  let s = await seen(page);
  check('library: chevrons + shelf scrolling mount new cards without any entrance animation', enters(s).length === 0 && !s.some((x) => x.name === 'rise-scroll' && x.el.startsWith('div.slot')), list(s));
  await page.evaluate(() => window.scrollTo({ top: 400 })); await sleep(500);
  s = await seen(page);
  check('library: page scroll fires no entrance', enters(s).length === 0, list(s));
  await page.evaluate(() => window.scrollTo({ top: 0 }));
  // search: typing crossfades the body at most once per 150ms
  await reset(page);
  await page.evaluate(() => { window.__fades = 0; const o = Element.prototype.animate; Element.prototype.animate = function (...a) { if (this.classList.contains('body')) window.__fades++; return o.apply(this, a); }; });
  const t0 = Date.now();
  await page.locator('input[type=search]').pressSequentially('game 1', { delay: 25 });
  const typed = Date.now() - t0;
  const fades = await page.evaluate(() => window.__fades);
  check('library: typing 6 keys crossfades the grid, throttled to <=1 per 150ms', fades >= 1 && fades <= Math.ceil(typed / 150) + 1, `${fades} fades in ${typed}ms`);
  await sleep(300);
  s = await seen(page);
  const cf = s.find((x) => x.el.startsWith('div.body'));
  check('library: search crossfade is opacity-only, 140ms', !!cf && cf.dur === 140 && cf.props === 'opacity', JSON.stringify(cf));
  check('library: search triggers no entrance rise', enters(s).length === 0, list(s));
  await page.locator('input[type=search]').fill(''); await sleep(300);
  // client navigation
  await page.click('aside a.nav[href="/settings"]'); await page.waitForFunction(() => location.pathname.startsWith('/settings'));
  await reset(page); await page.goto(BASE + '/library'); await sleep(1500);
  await reset(page);
  await page.click('aside a.nav[href="/history"]'); await page.waitForFunction(() => location.pathname.startsWith('/history')); await sleep(1300);
  s = await seen(page);
  check('client nav to History: cards staggered in', enters(s).length >= 3 && new Set(enters(s).map((x) => x.delay)).size >= 3, list(s));
  check('client nav: sidebar does not animate', !s.some((x) => x.side && /enter|fade|rise/.test(x.name)));
  await reset(page);
  await page.click('aside a.nav[href="/stream"]'); await page.waitForFunction(() => location.pathname.startsWith('/stream')); await sleep(900);
  s = await seen(page);
  check('client nav to Stream: no entrance/fade/reveal', !s.some((x) => /^(enter-rise|fade-in|rise-in|rise-scroll)$/.test(x.name)), JSON.stringify(s.map((x) => x.name)));
  check('stream: no pending-hide flag left', await page.evaluate(() => !document.documentElement.hasAttribute('data-enter-pending')));
  check('no errors', errs.length === 0, errs.slice(0, 2).join('|'));
  await page.context().close();
}

// ---- stream reload, reduced motion
{
  const { page } = await mk();
  await page.goto(BASE + '/stream'); await sleep(1000);
  const s = await seen(page);
  check('stream (reload): no entrance/fade/reveal animation', !s.some((x) => /^(enter-rise|fade-in|rise-in|rise-scroll)$/.test(x.name)), JSON.stringify(s.map((x) => x.name)));
  await page.context().close();
}
{
  const { page } = await mk({ reducedMotion: 'reduce' });
  for (const [n, p] of ROUTES) {
    await page.goto(BASE + p); await sleep(1100);
    const s = await seen(page);
    check(`reduced motion ${n}: no entrance / fade / reveal, nothing hidden`, !s.some((x) => /^(enter-rise|fade-in|rise-in|rise-scroll)$/.test(x.name)) && await page.evaluate(() => [...document.querySelectorAll('main .reveal:not(.soon), main .enter')].every((e) => getComputedStyle(e).opacity === '1')), JSON.stringify(s.map((x) => x.name)));
  }
  await page.context().close();
}

// ---- Home: late data fades in
{
  const { page } = await mk({ pause: false });
  await page.route('**/api/host', async (r) => { await sleep(900); await r.fulfill({ json: host }); });
  await page.goto(BASE + '/'); await sleep(500);
  await reset(page);
  await page.waitForFunction(() => document.querySelector('.big')?.textContent === 'Online'); await sleep(150);
  const s = await seen(page);
  check('home: host card content fades in when its data lands', s.some((x) => x.name === 'fade-in' && x.el.startsWith('div.big')), JSON.stringify(s.map((x) => x.el + ':' + x.name)));
  await page.context().close();
}

// ---- LOOK: paused, seek every entrance to fixed absolute times, screenshot
const T = [0, 150, 300, 500];
async function look(name, path, delayGames, ready) {
  const { page } = await mk({ pause: true, delayGames });
  await page.goto(BASE + path);
  await page.waitForSelector(ready); await sleep(500);
  const n = await page.evaluate(() => window.__held.filter((a) => a.animationName === 'enter-rise').length);
  const info = [];
  for (const t of T) {
    const r = await page.evaluate((t) => {
      for (const a of window.__held) { if (a.animationName === 'fade-in' && a.effect.target.tagName === 'MAIN') a.currentTime = 200; else a.currentTime = t; }
      const e = window.__held.filter((a) => a.animationName === 'enter-rise').map((a) => { const c = getComputedStyle(a.effect.target); return { o: +(+c.opacity).toFixed(2), y: c.translate === 'none' ? 0 : parseFloat(c.translate.split(' ')[1]) }; });
      return { absent: e.filter((x) => x.o === 0).length, mid: e.filter((x) => x.o > 0 && x.o < 1).length, done: e.filter((x) => x.o === 1).length, n: e.length };
    }, t);
    await sleep(60);
    await page.screenshot({ path: `${OUT}/${name}-${String(t).padStart(3, '0')}ms.png` });
    info.push({ t, ...r });
  }
  console.log(`  ${name}: ${n} entrance animations; per time (absent/mid/done):`, info.map((i) => `${i.t}ms ${i.absent}/${i.mid}/${i.done}`).join('  '));
  check(`${name}: at 0ms every item is invisible; at 500ms most are visible and the last has not finished before ${T.at(-1)}ms`, info[0].absent === info[0].n && info[0].n >= 5 && info[3].done >= 1 && info[3].absent === 0 && info[1].mid + info[1].absent >= info[1].n - 1, JSON.stringify(info));
  await page.evaluate(() => window.__held.forEach((a) => a.finish())); await sleep(100);
  const fin = await page.evaluate(() => [...document.querySelectorAll('[data-enter]:not(.soon)')].every((e) => getComputedStyle(e).opacity === '1'));
  check(`${name}: at the end every item has opacity 1`, fin);
  await page.context().close();
}
await look('home', '/', 0, '.bento .card');
await look('library', '/library', 700, '.hero');
await look('history', '/history', 0, '.grid .card');

console.log('max concurrent animations per route (incl. smoke/shimmer/main):', JSON.stringify(maxes));
await browser.close();
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
