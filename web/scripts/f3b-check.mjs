// Motion pass 2, F3b: Machines status transitions (#13) and the "Add a module" card swelling into the picker (#9).
// Assertions + seeked screenshots in headless Chrome against `astro preview` (mocked /api, own port), killed by PID:
//   npx astro preview --host 127.0.0.1 --port 4490 &   then   node scripts/f3b-check.mjs [section...]
// Sections: status stream picker reduce fail. Screenshots go to $OUT.
import { chromium } from 'playwright-core';
import { exe } from './browser.mjs';
import fs from 'node:fs';
import path from 'node:path';
import { axe } from './axe.mjs';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4490';
const OUT = process.env.OUT ?? '/tmp/f3b-shots';
const only = process.argv.slice(2);
const want = (s) => !only.length || only.includes(s);
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x !== '' ? ` (${x})` : ''}`));

const games = Array.from({ length: 8 }, (_, i) => ({
  appid: 1000 + i,
  name: `Game ${i}`,
  installed: true,
  sizeBytes: 1e9,
  lastPlayed: 1700000000 - i * 1e5,
  playtimeMin: 90,
  cover: '',
  running: false,
}));
const mkHost = () => ({ name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null });

const browser = await chromium.launch({ executablePath: exe });
async function mk({ blockAnime = false, ...opts } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  const state = { host: mkHost() };
  await ctx.route('**/api/**', async (r) => {
    const p = new URL(r.request().url()).pathname;
    const json =
      p === '/api/games'
        ? { games, ownedAvailable: true }
        : p === '/api/host'
          ? state.host
          : p === '/api/stream'
            ? { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null }
            : p === '/api/prefs'
              ? { prefs: {} }
              : p === '/api/sessions'
                ? { sessions: [] }
                : {};
    await r.fulfill({ json });
  });
  // the lazy anime.js chunks (nothing here needs them: both effects are native)
  if (blockAnime) await ctx.route(/\/_astro\/(animation|easings|scope|svg)\.[^/]*\.js$/, (r) => r.abort());
  await ctx.addInitScript(() => {
    window.__cls = 0;
    new PerformanceObserver((l) => l.getEntries().forEach((e) => !e.hadRecentInput && (window.__cls += e.value))).observe({
      type: 'layout-shift',
      buffered: true,
    });
  });
  const page = await ctx.newPage();
  const errs = [];
  page.on('console', (m) => m.type() === 'error' && !/Failed to load resource|ERR_FAILED/.test(m.text()) && errs.push(m.text()));
  page.on('pageerror', (e) => errs.push(String(e)));
  return { ctx, page, errs, state };
}
const poll = (page) => page.evaluate(() => document.dispatchEvent(new Event('visibilitychange'))); // HostPill refreshes $host on it
const shot = (page, name, clip) => page.screenshot({ path: path.join(OUT, `${name}.png`), clip });
// script-made animations on elements matching `sel`, with their animated properties
const anims = (page, sel) =>
  page.evaluate(
    (sel) =>
      document
        .getAnimations()
        .filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition) && a.effect?.target?.matches?.(sel))
        .map((a) => ({
          where: a.effect.target.closest('aside') ? 'pill' : 'card',
          props: [...new Set(a.effect.getKeyframes().flatMap((k) => Object.keys(k)))]
            .filter((k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k))
            .sort()
            .join(),
          dur: a.effect.getTiming().duration,
          iter: a.effect.getTiming().iterations,
        })),
    sel,
  );
const rows = (page) =>
  page.evaluate(() => {
    const c = document.querySelector('.grid > *');
    return {
      h: c.getBoundingClientRect().height,
      dts: [...c.querySelectorAll('dt')].map((d) => Math.round(d.getBoundingClientRect().top)).join(),
    };
  });

// ---- #13 status transitions on /machines
if (want('status')) {
  const { page, state, errs } = await mk({ deviceScaleFactor: 3 });
  await page.goto(BASE + '/machines');
  await page.waitForSelector('.grid .dotw');
  await sleep(900);
  check('status: first paint cross-fades nothing and fades no text', (await anims(page, '.was, .grid dd, .grid .big')).length === 0);
  check(
    'status: .was copies are aria-hidden and rest at opacity 0',
    await page.evaluate(() => {
      const w = [...document.querySelectorAll('.was')];
      return w.length === 2 && w.every((e) => e.getAttribute('aria-hidden') === 'true' && getComputedStyle(e).opacity === '0');
    }),
  );
  check(
    'status: no live region in the Machines card (state changes are not announced)',
    await page.evaluate(() => !document.querySelector('.grid [aria-live], .grid [role="status"], .grid [role="alert"]')),
  );
  const r0 = await rows(page);
  await page.evaluate(() => (window.__cls = 0));
  state.host = { ...state.host, sunshine: 'busy' };
  await poll(page);
  await page.waitForFunction(
    () =>
      document.querySelector('.grid .dot.busy') &&
      document.getAnimations().filter((a) => a.effect?.target?.classList?.contains('was')).length === 2,
  );
  const a1 = await anims(page, '.was');
  check(
    'status: ready -> busy cross-fades the dot on the pill and the card',
    a1.length === 2 && a1.some((a) => a.where === 'pill') && a1.some((a) => a.where === 'card'),
    JSON.stringify(a1),
  );
  check(
    'status: the cross-fade is opacity only, 360 ms, no loop',
    a1.every((a) => a.props === 'opacity' && a.dur === 360 && a.iter === 1),
    JSON.stringify(a1[0]),
  );
  check(
    'status: the old colour is the ready one',
    await page.evaluate(() => [...document.querySelectorAll('.was')].every((w) => w.dataset.s === 'ready')),
  );
  const t1 = await anims(page, '.grid dd, .grid .big');
  check(
    'status: the Sunshine word fades (opacity only), the unchanged big text does not',
    t1.length === 1 && t1[0].props === 'opacity',
    JSON.stringify(t1),
  );
  for (const t of [0, 120, 240]) {
    await page.evaluate(
      (t) =>
        document
          .getAnimations()
          .filter((a) => a.effect?.target?.matches?.('.was, .halo'))
          .forEach((a) => {
            a.pause();
            a.currentTime = t;
          }),
      t,
    );
    const b = await page.locator('.grid .dotw').boundingBox();
    await shot(page, `dot-ready-busy-t${t}`, { x: b.x - 20, y: b.y - 20, width: 48, height: 48 });
  }
  await page.evaluate(() => document.getAnimations().forEach((a) => a.play()));
  await sleep(800);
  check(
    'status: done, .was back at 0, no inline style left on the dot parts',
    (await anims(page, '.was')).length === 0 &&
      (await page.evaluate(
        () =>
          [...document.querySelectorAll('.was, .halo, .grid .dot, .grid dd, .grid .big')].every((e) => !e.getAttribute('style')) &&
          [...document.querySelectorAll('.was')].every((e) => getComputedStyle(e).opacity === '0'),
      )),
  );
  // uptime 1 h -> 2 h, then Sunshine down (the big text changes)
  state.host = { ...state.host, uptimeS: 8000 };
  await poll(page);
  await page.waitForFunction(() => [...document.querySelectorAll('.grid dd')].some((d) => d.textContent === '2 h'));
  const t2 = await anims(page, '.grid dd, .grid .big');
  check(
    'status: uptime 1 h -> 2 h fades the uptime only, no dot fade',
    t2.length === 1 && t2[0].props === 'opacity' && (await anims(page, '.was')).length === 0,
    JSON.stringify(t2),
  );
  await page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => a.effect?.target?.matches?.('.grid dd'))
      .forEach((a) => {
        a.pause();
        a.currentTime = 60;
      }),
  );
  await shot(page, 'uptime-fade-t60', { x: 250, y: 60, width: 580, height: 330 });
  await page.evaluate(() => document.getAnimations().forEach((a) => a.play()));
  await sleep(500);
  check(
    'status: rows do not move and the card keeps its height',
    JSON.stringify(await rows(page)) === JSON.stringify(r0),
    `${JSON.stringify(r0)} -> ${JSON.stringify(await rows(page))}`,
  );
  state.host = { ...state.host, sunshine: 'down' };
  await poll(page);
  await page.waitForFunction(() => document.querySelector('.grid .dot.down'));
  await sleep(30);
  check(
    'status: busy -> down cross-fades from busy and fades the big text + Sunshine word',
    (await anims(page, '.was')).length === 2 &&
      (await anims(page, '.grid dd, .grid .big')).length === 2 &&
      (await page.evaluate(() => document.querySelector('.grid .was').dataset.s === 'busy')),
  );
  await sleep(600);
  await shot(page, 'machines-down', { x: 250, y: 60, width: 580, height: 330 });
  const r2 = await rows(page);
  check(
    'status: "Sunshine is down" keeps the card height and rows (no layout jump)',
    JSON.stringify(r2) === JSON.stringify(r0),
    `${JSON.stringify(r0)} -> ${JSON.stringify(r2)}`,
  );
  // Chrome scores the right-aligned Sunshine word getting longer ("Ready" -> "Streaming") as ~0.00015 per change, with
  // or without motion (its line box keeps its rect); anything that really moves would be far above this
  check(
    'status: layout shift ~0 through all of it (only the Sunshine word reflowing in place)',
    (await page.evaluate(() => window.__cls)) < 0.001,
    String(await page.evaluate(() => window.__cls)),
  );
  await poll(page);
  await sleep(200);
  check('status: the same state polled again does nothing', (await anims(page, '.was, .grid dd, .grid .big')).length === 0);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- /stream: the pill dot is mounted there; a state change animates nothing
if (want('stream')) {
  const { page, state, errs } = await mk();
  await page.goto(BASE + '/stream');
  await sleep(1200);
  state.host = { ...state.host, sunshine: 'down' };
  await poll(page);
  await sleep(300);
  check(
    'stream: host state change makes no script-made animation at all',
    (await page.evaluate(
      () => document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition)).length,
    )) === 0,
  );
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- #9 the "Add a module" card swells into the picker and back
const card = (page) =>
  page.evaluate(() => {
    const r = document.querySelector('.ghostcard').getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height };
  });
const mp = (page) =>
  page.evaluate(() => {
    const e = document.querySelector('.mp');
    if (!e) return null;
    const r = e.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height };
  });
const near = (a, b, d = 2) => a && b && ['x', 'y', 'w', 'h'].every((k) => Math.abs(a[k] - b[k]) <= d);
const r1 = (b) => b && Object.fromEntries(Object.entries(b).map(([k, v]) => [k, Math.round(v)]));
if (want('picker')) {
  const { page, ctx, errs } = await mk();
  await page.goto(BASE + '/');
  await page.waitForSelector('.ghostcard');
  await sleep(1200);
  await page.locator('.ghostcard').scrollIntoViewIfNeeded();
  await sleep(600);
  const c = await card(page);
  await page.mouse.move(5, 5);
  await sleep(400); // no hover lift
  const c0 = await card(page);
  await page.evaluate(() => (window.__cls = 0));
  await page.locator('.ghostcard').focus();
  await page.keyboard.press('Enter');
  await page.waitForSelector('.mp');
  const g = await anims(page, '.mp');
  check(
    'picker: one script-made FLIP on the panel, translate + scale only',
    g.length === 1 && g[0].props === 'scale,translate',
    JSON.stringify(g),
  );
  check(
    'picker: entry opacity is the CSS transition (no width/height/left/top animated)',
    await page.evaluate(() =>
      document
        .getAnimations()
        .filter((a) => a.effect?.target?.classList?.contains('mp'))
        .every(
          (a) =>
            a.effect.getKeyframes().every((k) => !('width' in k || 'height' in k || 'left' in k || 'top' in k)) &&
            (!(a instanceof CSSTransition) || a.transitionProperty === 'opacity'),
        ),
    ),
  );
  await page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => a.effect?.target?.classList?.contains('mp'))
      .forEach((a) => {
        a.pause();
        a.currentTime = 0;
      }),
  );
  const s0 = await mp(page);
  check('picker: at t=0 the panel lies exactly on the card', near(s0, c0), `${JSON.stringify(r1(s0))} vs card ${JSON.stringify(r1(c0))}`);
  await shot(page, 'picker-0-t0');
  for (const t of [80, 200]) {
    await page.evaluate(
      (t) =>
        document
          .getAnimations()
          .filter((a) => a.effect?.target?.classList?.contains('mp'))
          .forEach((a) => {
            a.currentTime = t;
          }),
      t,
    );
    await shot(page, `picker-1-t${t}`);
  }
  await page.evaluate(() => document.getAnimations().forEach((a) => a.play()));
  await sleep(900);
  const end = await mp(page);
  check(
    'picker: settles centred, wider than the card',
    end && Math.abs(end.x + end.w / 2 - 720) < 2 && end.w > 300,
    JSON.stringify(r1(end)),
  );
  await shot(page, 'picker-2-open');
  const bad = await axe(page);
  check('picker: axe clean with the picker open', bad.length === 0, bad.join(' | '));
  check('picker: focus is inside the dialog', await page.evaluate(() => document.querySelector('.mp').contains(document.activeElement)));
  const left = await page.evaluate(() => {
    const e = document.querySelector('.mp');
    return {
      style: e.style.cssText,
      wc: getComputedStyle(e).willChange,
      tf: getComputedStyle(e).transform,
      tr: getComputedStyle(e).translate,
      sc: getComputedStyle(e).scale,
      n: e.getAnimations().length,
    };
  });
  // Bits writes pointer-events, contain and its --bits-* depth vars inline; ours are only --mp-* (set when closing)
  check(
    'picker: nothing left behind on the panel (no inline transition/translate/scale, no will-change, at rest)',
    !/transition|translate|scale|will-change|--mp-/.test(left.style) &&
      left.wc === 'auto' &&
      left.tf === 'none' &&
      left.tr === 'none' &&
      left.sc === 'none' &&
      left.n === 0,
    JSON.stringify(left),
  );
  check('picker: the card did not move', near(await card(page), c0, 0.5));
  // Tab stays inside (Done is the only control)
  await page.keyboard.press('Tab');
  await page.keyboard.press('Tab');
  check('picker: Tab is trapped', await page.evaluate(() => document.querySelector('.mp').contains(document.activeElement)));
  // close: shrinks back onto the card through Bits' data-ending-style
  await page.keyboard.press('Escape');
  await page.waitForSelector('.mp[data-ending-style]');
  const ex = await page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => a.effect?.target?.classList?.contains('mp'))
      .map((a) => (a instanceof CSSTransition ? a.transitionProperty : 'script'))
      .sort()
      .join(),
  );
  check('picker: the exit is CSS transitions Bits can see (opacity, scale, translate)', ex === 'opacity,scale,translate', ex);
  await page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => a.effect?.target?.classList?.contains('mp'))
      .forEach((a) => {
        a.pause();
        a.currentTime = 100;
      }),
  );
  const mid = await mp(page);
  check('picker: mid-exit the panel is between its spot and the card', mid && mid.w < end.w && mid.w > c0.w, JSON.stringify(r1(mid)));
  await shot(page, 'picker-3-exit-t100');
  await page.evaluate(() =>
    document.getAnimations().forEach((a) => {
      a.currentTime = a.effect.getTiming().duration - 1;
    }),
  );
  const last = await mp(page);
  check('picker: the exit ends on the card', near(last, c0, 4), `${JSON.stringify(r1(last))} vs ${JSON.stringify(r1(c0))}`);
  await shot(page, 'picker-4-exit-end');
  await page.evaluate(() => document.getAnimations().forEach((a) => a.play()));
  await page.waitForFunction(() => !document.querySelector('.mp'), null, { timeout: 2000 });
  check(
    'picker: unmounted after the exit; focus back on the card',
    await page.evaluate(() => document.activeElement?.classList.contains('ghostcard')),
  );
  check(
    'picker: layout shift 0 for open + close',
    (await page.evaluate(() => window.__cls)) === 0,
    String(await page.evaluate(() => window.__cls)),
  );
  // mouse open + Done, and a close in the middle of the swell
  await page.click('.ghostcard');
  await page.waitForSelector('.mp');
  await sleep(80);
  await page.keyboard.press('Escape');
  await page.waitForFunction(() => !document.querySelector('.mp'), null, { timeout: 2000 });
  check(
    'picker: Esc mid-swell closes cleanly, focus back',
    await page.evaluate(
      () =>
        document.activeElement?.classList.contains('ghostcard') &&
        document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition)).length === 0,
    ),
  );
  await page.click('.ghostcard');
  await page.waitForSelector('.mp');
  await sleep(700);
  await page.getByRole('button', { name: 'Done' }).click();
  await page.waitForFunction(() => !document.querySelector('.mp'), null, { timeout: 2000 });
  check('picker: Done closes, focus back on the card', await page.evaluate(() => document.activeElement?.classList.contains('ghostcard')));
  check('picker: the page did not scroll under it', Math.abs(c.y - (await card(page)).y) < 0.5);
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

// ---- reduced motion: nothing moves, everything still works
if (want('reduce')) {
  const { page, ctx, state, errs } = await mk({ reducedMotion: 'reduce' });
  await page.goto(BASE + '/machines');
  await page.waitForSelector('.grid .dotw');
  await sleep(700);
  state.host = { ...state.host, sunshine: 'down', uptimeS: 9000 };
  await poll(page);
  await page.waitForSelector('.grid .dot.down');
  await sleep(100);
  check(
    'reduce: state + uptime change animate nothing; the dot is down',
    (await anims(page, '.was, .halo, .grid dd, .grid .big')).length === 0,
  );
  await page.goto(BASE + '/');
  await page.waitForSelector('.ghostcard');
  await sleep(800);
  await page.click('.ghostcard');
  await page.waitForSelector('.mp');
  const c0 = await card(page);
  check(
    'reduce: the picker is just there (no animation, centred at once)',
    (await page.evaluate(() => document.querySelector('.mp').getAnimations().length === 0)) && !near(await mp(page), c0),
  );
  await page.keyboard.press('Escape');
  await sleep(30);
  check(
    'reduce: Esc unmounts at once, focus back on the card',
    await page.evaluate(() => !document.querySelector('.mp') && document.activeElement?.classList.contains('ghostcard')),
  );
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

// ---- the anime.js chunks fail to load: neither effect needs them
if (want('fail')) {
  const { page, ctx, state, errs } = await mk({ blockAnime: true });
  await page.goto(BASE + '/machines');
  await page.waitForSelector('.grid .dotw');
  await sleep(700);
  state.host = { ...state.host, sunshine: 'busy' };
  await poll(page);
  await page.waitForSelector('.grid .dot.busy');
  await sleep(30);
  check('fail: dot still cross-fades (native)', (await anims(page, '.was')).length === 2);
  await page.goto(BASE + '/');
  await page.waitForSelector('.ghostcard');
  await sleep(800);
  await page.click('.ghostcard');
  await page.waitForSelector('.mp');
  await sleep(30);
  check('fail: picker still swells (native)', (await anims(page, '.mp')).length === 1);
  check('no errors', errs.length === 0, errs.join('|'));
  await ctx.close();
}

await browser.close();
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
