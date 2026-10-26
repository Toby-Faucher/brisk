// Motion pass 2, F3c1: Home card drag-to-reorder (#10). Real pointer events (mouse, touch, pen) and the keyboard path in
// headless Chrome against `astro preview` (stubbed /api with a stateful homeOrder, own port), killed by PID:
//   npx astro preview --host 127.0.0.1 --port 4491 &   then   node scripts/f3c1-check.mjs [section...]
// Sections: load nojs pointer touch pen keyboard cancel persist race nav reduce stream axe. Screenshots go to $OUT.
import { chromium } from 'playwright-core';
import fs from 'node:fs';
import path from 'node:path';
import { axe } from './axe.mjs';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4491';
const OUT = process.env.OUT ?? '/tmp/claude-1000/-home-toby-Projects/94a1bf04-5503-4de5-ba94-79e19e0c32dd/scratchpad/f3c1';
const exe = process.env.BRISK_CHROMIUM ?? `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const only = process.argv.slice(2);
const want = (s) => !only.length || only.includes(s);
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x !== '' ? ` (${x})` : ''}`));

const DEFAULT = ['host', 'resume', 'quick', 'recent', 'library', 'machines', 'agents', 'news', 'workouts'];
const games = Array.from({ length: 8 }, (_, i) => ({ appid: 1000 + i, name: `Game ${i}`, installed: true, sizeBytes: 1e9, lastPlayed: 1700000000 - i * 1e5, playtimeMin: 90, cover: '', running: false }));
const host = { name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null };

const browser = await chromium.launch({ executablePath: exe });
// `state.homeOrder` is the stub hostd's prefs.json: it lives across reloads of one context (persistence)
async function mk({ homeOrder = [], prefsDelay = 0, blockHome = false, ...opts } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  const state = { homeOrder, puts: [], deletes: 0, gets: 0, putDelay: [] };
  await ctx.route('**/api/**', async (r) => {
    const req = r.request();
    const p = new URL(req.url()).pathname;
    if (p === '/api/prefs/layout' && req.method() === 'PUT') {
      const ids = req.postDataJSON().homeOrder;
      const wait = state.putDelay[state.puts.length] ?? 0;
      state.puts.push(ids);
      state.homeOrder = ids;
      await sleep(wait);
      return r.fulfill({ json: { homeOrder: ids } });
    }
    if (p === '/api/prefs/layout' && req.method() === 'DELETE') return (state.deletes++, (state.homeOrder = []), r.fulfill({ status: 204 }));
    if (p === '/api/prefs') {
      state.gets++;
      await sleep(prefsDelay);
      return r.fulfill({ json: { prefs: {}, homeOrder: state.homeOrder } });
    }
    const json =
      p === '/api/games' ? { games, ownedAvailable: true }
      : p === '/api/host' ? host
      : p === '/api/stream' ? { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null }
      : p === '/api/sessions' ? { sessions: [] }
      : {};
    await r.fulfill({ json });
  });
  // Astro retries a failed island import once as `?astro-retry=<n>`: block that too
  if (blockHome) await ctx.route(/\/_astro\/Home\.[^/?]*\.js(\?.*)?$/, (r) => r.abort());
  await ctx.addInitScript(() => {
    window.__cls = 0;
    window.__shifts = [];
    new PerformanceObserver((l) => l.getEntries().forEach((e) => (window.__shifts.push({ v: e.value, input: e.hadRecentInput }), !e.hadRecentInput && (window.__cls += e.value)))).observe({ type: 'layout-shift', buffered: true });
  });
  const page = await ctx.newPage();
  const errs = [];
  page.on('console', (m) => m.type() === 'error' && !/Failed to load resource|ERR_FAILED/.test(m.text()) && errs.push(m.text()));
  page.on('pageerror', (e) => errs.push(String(e)));
  return { ctx, page, errs, state };
}
const order = (page) => page.evaluate(() => [...document.querySelectorAll('.bento [data-card]')].map((e) => e.dataset.card));
const home = async (page) => {
  await page.goto(BASE + '/');
  await page.waitForSelector('.bento:not([data-hold]) [data-card="library"]');
  await sleep(1300); // entrance done
  await page.mouse.move(2, 2);
};
const center = (page, sel) => page.evaluate((sel) => { const r = document.querySelector(sel).getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; }, sel);
const grip = (id) => `[data-card="${id}"] [data-grip]`;
const shot = (page, name) => page.screenshot({ path: path.join(OUT, `${name}.png`) });
// script-made animations on cards: which card, animated properties
const anims = (page) => page.evaluate(() => document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition) && a.effect?.target?.closest?.('[data-card]')).map((a) => ({ id: a.effect.target.dataset.card, props: [...new Set(a.effect.getKeyframes().flatMap((k) => Object.keys(k)))].filter((k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k)).sort().join(), dur: a.effect.getTiming().duration, fill: a.effect.getTiming().fill })));
const seek = (page, t) => page.evaluate((t) => document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition) && a.effect.getTiming().duration > 0).forEach((a) => { a.pause(); a.currentTime = t; }), t);
const play = (page) => page.evaluate(() => document.getAnimations().forEach((a) => a.play()));
// nothing of ours left: no data-lifted, no inline style beyond the grid spans, no script animation, handles unpressed
const clean = (page) => page.evaluate(() => {
  const cards = [...document.querySelectorAll('[data-card]')];
  const bad = cards.filter((c) => c.hasAttribute('data-lifted') || c.hasAttribute('data-landing') || !/^(grid-area: span \d+ \/ span \d+;( --i: \d+;)?|grid-column: span \d+; grid-row: span \d+;)$/.test(c.getAttribute('style') ?? '') || getComputedStyle(c).willChange !== 'auto');
  const pressed = [...document.querySelectorAll('[data-grip]')].filter((g) => g.getAttribute('aria-pressed') !== 'false');
  const script = document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition));
  return { bad: bad.map((c) => `${c.dataset.card}:${c.getAttribute('style')}`), pressed: pressed.length, script: script.length, html: document.documentElement.getAttribute('style') ?? '', body: document.body.getAttribute('style') ?? '' };
});
const isClean = (c) => !c.bad.length && !c.pressed && !c.script && !c.html && !c.body;
const said = (page) => page.evaluate(() => document.querySelector('.sr[role="status"]')?.textContent ?? null);

// drag the handle of card `a` onto the middle of card `b` in steps with a real input device
async function dragWith(page, dev, a, b, { steps = 12, hold, frames } = {}) {
  const s = await center(page, grip(a));
  const t = await center(page, `[data-card="${b}"]`);
  const o0 = (await order(page)).join();
  let framed = !frames;
  await dev.down(s);
  for (let i = 1; i <= steps; i++) {
    await dev.move({ x: s.x + ((t.x - s.x) * i) / steps, y: s.y + ((t.y - s.y) * i) / steps });
    // frames right after the first swap, while the neighbours are in flight
    if (!framed && (await order(page)).join() !== o0) (framed = true), await frames();
    await sleep(16);
  }
  if (!framed) await frames();
  if (hold) return;
  await dev.up(t);
}
const mouse = (page) => ({ down: async (p) => (await page.mouse.move(p.x, p.y), await page.mouse.down()), move: (p) => page.mouse.move(p.x, p.y), up: () => page.mouse.up() });
async function cdpDev(page, kind) {
  const c = await page.context().newCDPSession(page);
  if (kind === 'touch')
    return {
      down: (p) => c.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: p.x, y: p.y }] }),
      move: (p) => c.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: p.x, y: p.y }] }),
      up: () => c.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] }),
    };
  const m = (type, p, buttons) => c.send('Input.dispatchMouseEvent', { type, x: p.x, y: p.y, button: type === 'mouseMoved' ? 'none' : 'left', buttons, clickCount: 1, pointerType: 'pen' });
  let last = { x: 0, y: 0 };
  return { down: (p) => ((last = p), m('mousePressed', p, 1)), move: (p) => ((last = p), m('mouseMoved', p, 1)), up: () => m('mouseReleased', last, 0) };
}
const nav = async (page, href) => {
  await page.click(`aside a.nav[href="${href}"]`);
  await page.waitForFunction((h) => location.pathname.replace(/\/$/, '') === (h === '/' ? '' : h), href);
};

// ---- load: handles, ids, hidden until the order is in, no layout shift from a saved order
if (want('load')) {
  const { page, errs, state } = await mk({ homeOrder: ['library', 'recent', 'gone', 'host'], prefsDelay: 900 });
  await page.goto(BASE + '/', { waitUntil: 'commit' });
  await page.waitForSelector('.bento[data-hold]', { state: 'attached' });
  const held = await page.evaluate(() => ({ vis: getComputedStyle(document.querySelector('.bento')).visibility, h: document.querySelector('.bento').getBoundingClientRect().height }));
  check('load: the grid is held hidden while the order loads (visibility, so it keeps its size)', held.vis === 'hidden' && held.h > 100, JSON.stringify(held));
  await page.waitForSelector('.bento:not([data-hold])');
  await sleep(1300);
  const o = await order(page);
  check('load: saved order applied, unknown id dropped, the rest appended in default order', JSON.stringify(o) === JSON.stringify(['library', 'recent', 'host', 'resume', 'quick', 'machines', 'agents', 'news', 'workouts']), o.join());
  check('load: one handle per card, labelled, described, unpressed; Add a module has none', await page.evaluate(() => { const g = [...document.querySelectorAll('[data-grip]')]; return g.length === 9 && g.every((b) => /^Move .+ card$/.test(b.getAttribute('aria-label')) && b.getAttribute('aria-describedby') === 'grip-hint' && b.getAttribute('aria-pressed') === 'false') && !document.querySelector('.ghostcard [data-grip]') && document.querySelector('.bento > :last-child').classList.contains('ghostcard'); }));
  check('load: handle hit target at least 24x24', await page.evaluate(() => { const r = document.querySelector('[data-grip]').getBoundingClientRect(); return r.width >= 24 && r.height >= 24; }));
  check('load: bento spans kept (host 3x2, library 2x1)', await page.evaluate(() => { const s = (id) => document.querySelector(`[data-card="${id}"]`).getAttribute('style'); return /grid-area: span 2 \/ span 3/.test(s('host')) && /grid-area: span 1 \/ span 2/.test(s('library')); }));
  check('load: entrance played for the cards once shown', await page.evaluate(() => document.querySelectorAll('[data-card][data-enter]').length >= 6));
  check('load: layout shift 0 with a saved order', (await page.evaluate(() => window.__cls)) === 0, String(await page.evaluate(() => window.__cls)));
  check('load: one GET /api/prefs (Home and the Resume card share it)', state.gets === 1, String(state.gets));
  await shot(page, 'load-saved-order');
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- the Home chunk never loads: the held grid shows by its CSS-only fallback (~3 s), with or without reduced motion
if (want('nojs')) {
  for (const reducedMotion of ['no-preference', 'reduce']) {
    const { page } = await mk({ blockHome: true, reducedMotion });
    await page.goto(BASE + '/', { waitUntil: 'commit' });
    await page.waitForSelector('.bento[data-hold]', { state: 'attached' });
    await sleep(1000);
    const early = await page.evaluate(() => getComputedStyle(document.querySelector('.bento')).visibility);
    await sleep(2600);
    const late = await page.evaluate(() => ({ vis: getComputedStyle(document.querySelector('.bento')).visibility, hold: document.querySelector('.bento').hasAttribute('data-hold'), op: getComputedStyle(document.querySelector('.bento [data-card]')).opacity }));
    check(`nojs (${reducedMotion}): with the Home chunk blocked the grid is hidden at 1 s and shown by 3.6 s (SSR cards, still data-hold)`, early === 'hidden' && late.vis === 'visible' && late.hold && late.op === '1', JSON.stringify({ early, ...late }));
    if (reducedMotion === 'no-preference') await shot(page, 'nojs-fallback');
    await page.context().close();
  }
}

// ---- mouse: lift, neighbours FLIP, spring drop, saved
if (want('pointer')) {
  const { page, errs, state } = await mk();
  await home(page);
  await page.evaluate(() => (window.__cls = 0, window.__shifts = []));
  const before = await order(page);
  let mid;
  await dragWith(page, mouse(page), 'quick', 'machines', {
    frames: async () => {
      mid = await page.evaluate(() => { const c = document.querySelector('[data-card="quick"]'); return { lifted: c.hasAttribute('data-lifted'), scale: getComputedStyle(c).scale, z: getComputedStyle(c).zIndex, pressed: c.querySelector('[data-grip]').getAttribute('aria-pressed'), style: c.getAttribute('style') }; });
      const a = await anims(page);
      check('pointer: mid-drag the card is lifted (scale, z), handle pressed, no inline translate', mid.lifted && Math.abs(parseFloat(mid.scale) - 1.03) < 0.005 && mid.z === '3' && mid.pressed === 'true' && !/translate/.test(mid.style), JSON.stringify(mid));
      check('pointer: the held card follows by a held translate animation; neighbours FLIP on translate only', a.some((x) => x.id === 'quick' && x.props === 'translate' && x.fill === 'forwards') && a.filter((x) => x.id !== 'quick').length > 0 && a.every((x) => x.props === 'translate'), JSON.stringify(a));
      check('pointer: order changed under the pointer', (await order(page)).indexOf('quick') > before.indexOf('quick'), (await order(page)).join());
      check('pointer: a moved card does not replay the load entrance', await page.evaluate(() => document.getAnimations().filter((a) => a instanceof CSSAnimation && a.animationName === 'enter-rise' && a.effect.target.closest('.bento')).length === 0));
      await shot(page, 'pointer-1-held');
      await seek(page, 0); await shot(page, 'pointer-2-flip-t0');
      await seek(page, 150); await shot(page, 'pointer-3-flip-t150');
      await play(page);
    },
  });
  const a = await anims(page);
  check('pointer: release lands on the settle spring (translate only)', a.some((x) => x.id === 'quick' && x.props === 'translate' && x.dur === 900), JSON.stringify(a));
  check('pointer: the landing card stays above the others while it flies', await page.evaluate(() => { const c = document.querySelector('[data-card="quick"]'); return c.hasAttribute('data-landing') && getComputedStyle(c).zIndex === '2'; }));
  await seek(page, 90); await shot(page, 'pointer-4-drop-t90');
  await play(page); await sleep(1000);
  const after = await order(page);
  check('pointer: dropped after machines', after.indexOf('quick') === after.indexOf('machines') + 1 && after.length === before.length, after.join());
  check('pointer: one PUT with the new order', state.puts.length === 1 && JSON.stringify(state.puts[0]) === JSON.stringify(after), JSON.stringify(state.puts));
  check('pointer: one polite announcement', (await said(page)) === `Quick actions card dropped, position ${after.indexOf('quick') + 1} of ${after.length}`, await said(page));
  const c = await clean(page);
  check('pointer: nothing left behind (attributes, inline style, will-change, animations)', isClean(c), JSON.stringify(c));
  // the moves the user makes count as input-driven shifts (hadRecentInput), which CLS leaves out
  check('pointer: CLS 0 through the drag (every shift input-driven)', (await page.evaluate(() => window.__cls)) === 0, JSON.stringify(await page.evaluate(() => window.__shifts)));
  await shot(page, 'pointer-5-dropped');
  // a press on the handle without moving: no PUT, card clicks elsewhere still work
  await page.click(grip('host'));
  await sleep(300);
  check('pointer: a click on the handle changes nothing and saves nothing', state.puts.length === 1 && JSON.stringify(await order(page)) === JSON.stringify(after));
  await page.click('[data-card="library"] a.fill');
  await page.waitForFunction(() => location.pathname.startsWith('/library'));
  check('pointer: the card itself still clicks through (library link)', true);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- touch and pen: same pointer path
for (const kind of ['touch', 'pen']) {
  if (!want(kind)) continue;
  const { page, errs, state } = await mk(kind === 'touch' ? { hasTouch: true, isMobile: false } : {});
  await home(page);
  const dev = await cdpDev(page, kind);
  const y0 = await page.evaluate(() => scrollY);
  await dragWith(page, dev, 'recent', 'host', { frames: async () => kind === 'touch' && shot(page, 'touch-held') });
  await sleep(1000);
  const o = await order(page);
  check(`${kind}: drag reorders (recent before host)`, o.indexOf('recent') < o.indexOf('host'), o.join());
  check(`${kind}: saved once`, state.puts.length === 1);
  check(`${kind}: the page did not scroll`, (await page.evaluate(() => scrollY)) === y0);
  check(`${kind}: clean afterwards`, isClean(await clean(page)), JSON.stringify(await clean(page)));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- keyboard: Space picks up, arrows move, Enter drops, one announcement
if (want('keyboard')) {
  const { page, errs, state } = await mk();
  await home(page);
  await page.evaluate(() => {
    window.__said = [];
    new MutationObserver(() => window.__said.push(document.querySelector('.sr[role="status"]').textContent)).observe(document.querySelector('.sr[role="status"]'), { childList: true, characterData: true, subtree: true });
  });
  await page.focus(grip('resume'));
  await page.keyboard.press('Space');
  check('keyboard: Space picks up (pressed, lifted)', await page.evaluate(() => document.activeElement.getAttribute('aria-pressed') === 'true' && document.querySelector('[data-card="resume"]').hasAttribute('data-lifted')));
  await sleep(400);
  await shot(page, 'keyboard-1-picked');
  await page.keyboard.press('ArrowRight');
  const a = await anims(page);
  check('keyboard: a step FLIPs the moved card and its neighbour on translate', a.some((x) => x.id === 'resume') && a.every((x) => x.props === 'translate'), JSON.stringify(a));
  await seek(page, 120); await shot(page, 'keyboard-2-step-t120'); await play(page);
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowLeft');
  await sleep(50);
  let o = await order(page);
  check('keyboard: arrows move by one in order (+3 -1 = position 3)', o.indexOf('resume') === 3, o.join());
  check('keyboard: focus stays on the moved handle, still pressed', await page.evaluate(() => document.activeElement?.matches('[data-card="resume"] [data-grip]') && document.activeElement.getAttribute('aria-pressed') === 'true'));
  check('keyboard: nothing announced and nothing saved while moving', (await page.evaluate(() => window.__said.length)) === 0 && state.puts.length === 0);
  await page.keyboard.press('Enter');
  await sleep(900);
  check('keyboard: Enter drops: one PUT, one announcement', state.puts.length === 1 && JSON.stringify(state.puts[0]) === JSON.stringify(o) && JSON.stringify(await page.evaluate(() => window.__said)) === JSON.stringify([`Resume card dropped, position 4 of ${o.length}`]), JSON.stringify(await page.evaluate(() => window.__said)));
  check('keyboard: clean after the drop, focus still on the handle', isClean(await clean(page)) && (await page.evaluate(() => document.activeElement?.matches('[data-card="resume"] [data-grip]'))));
  await shot(page, 'keyboard-3-dropped');
  // ArrowLeft at the first slot is a no-op; Space also drops
  await page.focus(grip('host'));
  await page.keyboard.press('Space'); await page.keyboard.press('ArrowLeft'); await page.keyboard.press('Space');
  await sleep(300);
  check('keyboard: moving past the first slot does nothing; Space drops; unchanged = no PUT', state.puts.length === 1 && (await order(page))[0] === 'host');
  // Tab away while picked up drops it where it is
  await page.keyboard.press('Space'); await page.keyboard.press('ArrowRight'); await page.keyboard.press('Tab');
  await sleep(900);
  o = await order(page);
  check('keyboard: Tab away drops it (saved)', state.puts.length === 2 && o[1] === 'host' && isClean(await clean(page)), JSON.stringify([state.puts, await clean(page)]));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- Escape cancels (keyboard and pointer), nothing saved, back to the order at pick-up
if (want('cancel')) {
  const { page, errs, state } = await mk();
  await home(page);
  const o0 = await order(page);
  await page.focus(grip('quick'));
  await page.keyboard.press('Space'); await page.keyboard.press('ArrowRight'); await page.keyboard.press('ArrowRight');
  await page.keyboard.press('Escape');
  await sleep(900);
  check('cancel: keyboard Esc puts it back, saves and announces nothing', JSON.stringify(await order(page)) === JSON.stringify(o0) && state.puts.length === 0 && (await said(page)) === '' && isClean(await clean(page)));
  const s = await center(page, grip('host'));
  const t = await center(page, '[data-card="library"]');
  await page.mouse.move(s.x, s.y); await page.mouse.down();
  for (let i = 1; i <= 10; i++) (await page.mouse.move(s.x + ((t.x - s.x) * i) / 10, s.y + ((t.y - s.y) * i) / 10), await sleep(16));
  check('cancel: (mouse held over library: moved)', JSON.stringify(await order(page)) !== JSON.stringify(o0));
  await page.keyboard.press('Escape');
  check('cancel: pointer Esc: back to the start order, card glides home on the settle spring', JSON.stringify(await order(page)) === JSON.stringify(o0) && (await anims(page)).some((x) => x.id === 'host' && x.dur === 900));
  await seek(page, 120); await shot(page, 'cancel-pointer-esc-t120'); await play(page);
  await page.mouse.move(t.x + 20, t.y + 10); await page.mouse.up();
  await sleep(1000);
  check('cancel: moves and release after Esc do nothing; nothing saved', JSON.stringify(await order(page)) === JSON.stringify(o0) && state.puts.length === 0 && isClean(await clean(page)), JSON.stringify(await clean(page)));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- persistence: reload keeps the order (stub hostd keeps it), Settings > Reset layout clears it
if (want('persist')) {
  const { page, errs, state } = await mk();
  await home(page);
  await dragWith(page, mouse(page), 'library', 'host');
  await sleep(1000);
  const o = await order(page);
  check('persist: library moved to the front', o[0] === 'library', o.join());
  await page.reload();
  await page.waitForSelector('.bento:not([data-hold]) [data-card]'); await sleep(1300);
  check('persist: same order after a reload', JSON.stringify(await order(page)) === JSON.stringify(o), (await order(page)).join());
  check('persist: layout shift 0 on the reload', (await page.evaluate(() => window.__cls)) === 0, String(await page.evaluate(() => window.__cls)));
  await nav(page, '/settings');
  await page.getByRole('button', { name: 'Reset layout' }).click();
  await page.waitForFunction(() => document.querySelector('.toasts')?.textContent.includes('Home layout reset'));
  check('persist: Reset layout sends one DELETE', state.deletes === 1);
  await shot(page, 'settings-reset');
  await nav(page, '/');
  await page.waitForSelector('.bento:not([data-hold]) [data-card]'); await sleep(200);
  check('persist: Home is back in the default order after the reset (client navigation)', JSON.stringify(await order(page)) === JSON.stringify(DEFAULT), (await order(page)).join());
  await page.reload();
  await page.waitForSelector('.bento:not([data-hold]) [data-card]'); await sleep(200);
  check('persist: and after a reload', JSON.stringify(await order(page)) === JSON.stringify(DEFAULT));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- two quick drops whose PUT replies arrive out of order: the store keeps the latest order
if (want('race')) {
  const { page, errs, state } = await mk();
  state.putDelay = [900, 0]; // the first reply is slow (and stale)
  await home(page);
  await page.focus(grip('host'));
  await page.keyboard.press('Space'); await page.keyboard.press('ArrowRight'); await page.keyboard.press('Enter');
  await page.keyboard.press('Space'); await page.keyboard.press('ArrowRight'); await page.keyboard.press('Enter');
  const o = await order(page);
  await sleep(1300); // both replies are in, the stale one last
  check('race: two PUTs, the second is the newer order', state.puts.length === 2 && JSON.stringify(state.puts[1]) === JSON.stringify(o) && o[2] === 'host', JSON.stringify(state.puts));
  await nav(page, '/library'); await nav(page, '/');
  await page.waitForSelector('.bento:not([data-hold]) [data-card]'); await sleep(200);
  check('race: after the stale reply landed, Home still comes back in the latest order', JSON.stringify(await order(page)) === JSON.stringify(o), (await order(page)).join());
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- ClientRouter: leaving mid-drag leaves a clean DOM, coming back works and keeps the order
if (want('nav')) {
  const { page, errs, state } = await mk();
  await home(page);
  const s = await center(page, grip('quick'));
  const t = await center(page, '[data-card="library"]');
  await page.mouse.move(s.x, s.y); await page.mouse.down();
  for (let i = 1; i <= 8; i++) (await page.mouse.move(s.x + ((t.x - s.x) * i) / 8, s.y + ((t.y - s.y) * i) / 8), await sleep(16));
  await page.evaluate(() => document.querySelector('aside a.nav[href="/library"]').click()); // the router, with the button still held
  await page.waitForFunction(() => location.pathname.startsWith('/library'));
  await sleep(600);
  await page.mouse.up();
  const c = await page.evaluate(() => ({ lifted: document.querySelectorAll('[data-lifted]').length, html: document.documentElement.getAttribute('style') ?? '', body: document.body.getAttribute('style') ?? '', script: document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition) && a.effect?.target?.closest?.('[data-card]')).length }));
  check('nav: destroyed mid-drag: no lifted card, no inline style on html/body, no card animation', !c.lifted && !c.html && !c.body && !c.script, JSON.stringify(c));
  check('nav: nothing saved from the abandoned drag', state.puts.length === 0);
  await nav(page, '/');
  await page.waitForSelector('.bento:not([data-hold]) [data-card]'); await sleep(900);
  check('nav: back on Home: default order, clean', JSON.stringify(await order(page)) === JSON.stringify(DEFAULT) && isClean(await clean(page)), JSON.stringify(await clean(page)));
  await dragWith(page, mouse(page), 'recent', 'host');
  await sleep(1000);
  const o = await order(page);
  check('nav: dragging works again after the router remount', o.indexOf('recent') < o.indexOf('host') && state.puts.length === 1, o.join());
  await nav(page, '/library'); await nav(page, '/');
  await page.waitForSelector('.bento:not([data-hold]) [data-card]'); await sleep(200);
  check('nav: the new order survives client navigation', JSON.stringify(await order(page)) === JSON.stringify(o));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- reduced motion: instant swaps, no lift transition, no drop spring, still works
if (want('reduce')) {
  const { page, errs, state } = await mk({ reducedMotion: 'reduce' });
  await home(page);
  await page.focus(grip('resume'));
  await page.keyboard.press('Space');
  const tp = await page.evaluate(() => getComputedStyle(document.querySelector('[data-card="resume"]')).transitionProperty);
  check('reduce: the lift has no transition', tp === 'border-color' || tp === 'none', tp);
  await page.keyboard.press('ArrowRight');
  check('reduce: a keyboard step swaps at once (no animation)', (await anims(page)).length === 0 && (await order(page))[2] === 'resume');
  await page.keyboard.press('Enter');
  await dragWith(page, mouse(page), 'quick', 'machines', { frames: async () => check('reduce: neighbours do not FLIP mid-drag (only the held follow)', (await anims(page)).every((x) => x.fill === 'forwards'), JSON.stringify(await anims(page))) });
  check('reduce: no drop spring', (await anims(page)).length === 0);
  check('reduce: both saved', state.puts.length === 2 && isClean(await clean(page)), JSON.stringify([state.puts, await clean(page)]));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- /stream: no Home, no handles, no card animation
if (want('stream')) {
  const { page, errs } = await mk();
  await page.goto(BASE + '/stream'); await sleep(1200);
  check('stream: no handle, no data-card, no script animation', await page.evaluate(() => !document.querySelector('[data-grip], [data-card]') && document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition)).length === 0));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- axe at rest and with a card picked up
if (want('axe')) {
  const { page, errs } = await mk();
  await home(page);
  let bad = await axe(page);
  check('axe: Home at rest', bad.length === 0, bad.join(' | '));
  await page.focus(grip('host')); await page.keyboard.press('Space');
  bad = await axe(page);
  check('axe: Home with a card picked up', bad.length === 0, bad.join(' | '));
  await page.keyboard.press('Escape');
  await page.goto(BASE + '/settings'); await sleep(800);
  bad = await axe(page);
  check('axe: Settings with Reset layout', bad.length === 0, bad.join(' | '));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

await browser.close();
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
