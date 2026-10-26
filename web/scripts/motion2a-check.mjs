// Motion pass 2A: status-dot ring, play/spinner morph, split-text hero title, controller result chips, connection path.
// Assertions + seeked screenshots in headless Chrome against `astro preview` (mocked /api, own port), killed by PID:
//   npx astro preview --host 127.0.0.1 --port 4480 &   then   node scripts/motion2a-check.mjs [section...]
// Sections: graph stream ring morph split chips path reduce fail. Screenshots go to $OUT.
import { chromium } from 'playwright-core';
import fs from 'node:fs';
import path from 'node:path';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4480';
const OUT = process.env.OUT ?? '/tmp/claude-1000/-home-toby-Projects/94a1bf04-5503-4de5-ba94-79e19e0c32dd/scratchpad/motion2a';
const DIST = new URL('../dist/', import.meta.url).pathname;
const exe = process.env.BRISK_CHROMIUM ?? `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const only = process.argv.slice(2);
const want = (s) => !only.length || only.includes(s);
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x !== '' ? ` (${x})` : ''}`));

// ---- anime.js chunks = what motion.ts may load (its __vite__mapDeps list), minus Vite's preload helper
const astro = path.join(DIST, '_astro');
const files = fs.readdirSync(astro).filter((f) => f.endsWith('.js'));
const motionChunk = files.find((f) => /^motion\./.test(f) && /m\.f=\[[^\]]*"_astro\/text\./.test(fs.readFileSync(path.join(astro, f), 'utf8')));
const deps = motionChunk ? [...fs.readFileSync(path.join(astro, motionChunk), 'utf8').match(/m\.f=\[([^\]]*)\]/)[1].matchAll(/"_astro\/([^"]+)"/g)].map((m) => m[1]) : [];
const ANIME = new Set(deps.filter((f) => !/^preload-helper/.test(f)));
const isAnime = (url) => ANIME.has(url.split('/').pop());
const kb = (f) => +(fs.statSync(path.join(astro, f)).size / 1024).toFixed(2);

const games = Array.from({ length: 60 }, (_, i) => ({ appid: 1000 + i, name: i === 0 ? 'Portal 2' : `Game ${i}`, installed: i < 40, sizeBytes: 1e9, lastPlayed: i < 5 ? 1700000000 - i * 1e5 : null, playtimeMin: 90, cover: '', running: false }));
const mkHost = () => ({ name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null });

const browser = await chromium.launch({ executablePath: exe });
async function mk({ block = false, ...opts } = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  const state = { host: mkHost(), games, hold: null, startStatus: 200 };
  await ctx.route('**/api/**', async (r) => {
    const req = r.request();
    const p = new URL(req.url()).pathname;
    if (p === '/api/stream' && req.method() === 'POST') {
      await state.hold; // a start in flight until the test releases it
      return r.fulfill({ status: state.startStatus, json: state.startStatus === 200 ? { state: 'starting' } : { error: 'nope' } });
    }
    const json =
      p === '/api/games' ? { games: state.games, ownedAvailable: true }
      : p === '/api/host' ? state.host
      : p === '/api/stream' ? { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null }
      : p === '/api/prefs' ? { prefs: {} }
      : p === '/api/sessions' ? { sessions: [] }
      : {};
    await r.fulfill({ json });
  });
  if (block) await ctx.route((u) => isAnime(u.href), (r) => r.abort());
  await ctx.addInitScript(() => {
    window.__raf = 0;
    const raf = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (cb) => (window.__raf++, raf(cb));
  });
  const page = await ctx.newPage();
  const errs = [];
  const anime = [];
  page.on('console', (m) => m.type() === 'error' && !/Failed to load resource|ERR_FAILED/.test(m.text()) && errs.push(m.text()));
  page.on('pageerror', (e) => errs.push(String(e)));
  page.on('request', (r) => isAnime(r.url()) && anime.push(r.url().split('/').pop()));
  return { ctx, page, errs, anime, state };
}
const nav = async (page, href) => {
  await page.click(`aside a.nav[href="${href}"]`);
  await page.waitForFunction((h) => location.pathname.replace(/\/$/, '') === (h === '/' ? '' : h), href);
};
const rafIn = async (page, ms) => { const a = await page.evaluate(() => window.__raf); await sleep(ms); return (await page.evaluate(() => window.__raf)) - a; };
const poll = (page) => page.evaluate(() => document.dispatchEvent(new Event('visibilitychange'))); // HostPill refreshes $host on it
const shot = (page, name, clip) => page.screenshot({ path: path.join(OUT, `${name}.png`), clip });
const rings = (page) => page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.classList?.contains('ring')).map((a) => ({ where: a.effect.target.closest('aside') ? 'pill' : 'card', props: [...new Set(a.effect.getKeyframes().flatMap((k) => Object.keys(k)))].filter((k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k)).sort().join(), dur: a.effect.getTiming().duration, iter: a.effect.getTiming().iterations, ease: a.effect.getTiming().easing })));

// ---- 0. build: the import graph of the Stream page and chunk sizes
if (want('graph')) {
  check('build: motion chunk found and it lazy-loads the anime.js chunks (incl. text)', !!motionChunk && ANIME.size >= 4, `${motionChunk}: ${[...ANIME].join(' ')}`);
  const html = fs.readFileSync(path.join(DIST, 'stream/index.html'), 'utf8');
  const entries = [...html.matchAll(/(?:component-url|renderer-url|src)="\/_astro\/([^"]+\.js)"/g)].map((m) => m[1]);
  const seen = new Set();
  const walk = (f) => {
    if (seen.has(f) || !fs.existsSync(path.join(astro, f))) return;
    seen.add(f);
    for (const m of fs.readFileSync(path.join(astro, f), 'utf8').matchAll(/(?:from|import)\s*["']\.\/([^"']+\.js)["']/g)) walk(m[1]);
  };
  entries.forEach(walk);
  const bad = [...seen].filter((f) => ANIME.has(f) || f === motionChunk);
  check('import graph: /stream static closure contains no anime.js chunk and not motion.ts', bad.length === 0 && seen.size > 3, `${seen.size} chunks; offending: ${bad.join(',')}`);
  const stream = files.find((f) => /^Stream\./.test(f));
  console.log(`info Stream chunk ${stream}: ${fs.statSync(path.join(astro, stream)).size} bytes; closure ${[...seen].reduce((n, f) => n + fs.statSync(path.join(astro, f)).size, 0)} bytes in ${seen.size} files`);
  for (const f of ANIME) console.log(`info lazy chunk ${f}: ${kb(f)} kB`);
}

// ---- 1. /stream: no anime.js, no ring, no script-made animation, even when the host state changes there
if (want('stream')) {
  const { page, anime, state, errs } = await mk();
  await page.goto(BASE + '/stream'); await sleep(1200);
  state.host = { ...state.host, sunshine: 'down' }; await poll(page); await sleep(300);
  check('stream: host state change makes no ring', (await rings(page)).length === 0);
  check('stream: no script-made Web Animation', (await page.evaluate(() => document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition)).length)) === 0);
  check('stream: no anime.js chunk requested', anime.length === 0, anime.join());
  check('stream: no rAF from anime (engine idle)', (await rafIn(page, 1000)) <= 2);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- #6 status dot ring
if (want('ring')) {
  const { page, anime, state, errs } = await mk();
  await page.goto(BASE + '/machines'); await page.waitForSelector('.grid .dotw'); await sleep(900);
  check('ring: none on first paint', (await rings(page)).length === 0);
  check('ring: aria-hidden sibling, resting opacity 0', await page.evaluate(() => [...document.querySelectorAll('.ring')].length === 2 && [...document.querySelectorAll('.ring')].every((r) => r.getAttribute('aria-hidden') === 'true' && getComputedStyle(r).opacity === '0')));
  state.host = { ...state.host, sunshine: 'busy' }; await poll(page); await sleep(120);
  const r1 = await rings(page);
  check('ring: ready -> busy gives one ring on the pill and one on the card', r1.length === 2 && r1.some((r) => r.where === 'pill') && r1.some((r) => r.where === 'card'), JSON.stringify(r1));
  check('ring: scale + opacity only, 700 ms, no loop, --ease', r1.every((r) => r.props === 'opacity,scale' && r.dur === 700 && r.iter === 1 && /cubic-bezier/.test(r.ease)), JSON.stringify(r1[0]));
  await page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.classList?.contains('ring')).forEach((a) => { a.pause(); a.currentTime = 280; }));
  await shot(page, 'ring-mid-card', { x: 250, y: 60, width: 330, height: 60 });
  await shot(page, 'ring-mid-pill', { x: 0, y: 820, width: 240, height: 80 });
  await page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.classList?.contains('ring')).forEach((a) => a.play()));
  await sleep(900);
  check('ring: gone after 700 ms, resting opacity 0 again, no inline style left', (await rings(page)).length === 0 && await page.evaluate(() => [...document.querySelectorAll('.ring')].every((r) => getComputedStyle(r).opacity === '0' && !r.getAttribute('style'))));
  await poll(page); await sleep(150);
  check('ring: same state polled again makes no ring', (await rings(page)).length === 0);
  state.host = { ...state.host, sunshine: 'down' }; await poll(page); await sleep(100);
  check('ring: busy -> down rings again', (await rings(page)).length === 2);
  await sleep(900);
  check('ring: no anime.js needed (native animate)', anime.length === 0, anime.join());
  // sidebar pill on another page, and the load-time null -> value is not a change
  await nav(page, '/library'); await sleep(900);
  check('ring: no ring when the Shell remounts on navigation', (await rings(page)).length === 0);
  const raf = await rafIn(page, 1000);
  check('ring: nothing ticks after the animation', raf <= 2, `${raf} rAF/s`);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
  const q = await mk({ reducedMotion: 'reduce' });
  await q.page.goto(BASE + '/machines'); await q.page.waitForSelector('.grid .dotw'); await sleep(700);
  q.state.host = { ...q.state.host, sunshine: 'busy' }; await poll(q.page); await sleep(150);
  check('ring: reduced motion makes none; the dot state still changes', (await rings(q.page)).length === 0 && await q.page.evaluate(() => document.querySelector('.grid .dot').classList.contains('busy')));
  await q.page.context().close();
}

// ---- #5 play -> spinner swap (native Web Animations; triangle -> ring does not morph cleanly, see motion.ts swapIcon)
const RING = 'M12 4A8 8 0 1 1 4 12h3A5 5 0 1 0 12 7z';
const TRI = 'M7 4.5v15l13-7.5z';
const heroPath = (page) => page.evaluate(() => document.querySelector('.hero .btn svg path').getAttribute('d'));
const svgAnims = (page) => page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.closest?.('.hero .btn') && !(a instanceof CSSAnimation) && !(a instanceof CSSTransition)).map((a) => ({ props: [...new Set(a.effect.getKeyframes().flatMap((k) => Object.keys(k)))].filter((k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k)).sort().join(), dur: a.effect.getTiming().duration, inf: a.effect.getTiming().iterations === Infinity, ease: a.effect.getTiming().easing })));
if (want('morph')) {
  const { page, anime, state, errs } = await mk();
  await page.goto(BASE + '/library'); await page.waitForSelector('.hero .btn svg path'); await sleep(900);
  check('swap: first paint is the plain play triangle, nothing animating, no chunk', (await heroPath(page)) === TRI && (await svgAnims(page)).length === 0 && anime.length === 0);
  const name0 = await page.evaluate(() => document.querySelector('.hero .btn').textContent.trim());
  let release; state.hold = new Promise((r) => (release = r));
  await page.click('.hero .btn');
  await sleep(40);
  const a1 = await svgAnims(page);
  check('swap: busy turns the triangle out first (opacity + rotate, 90 ms, d still the triangle)', a1.length === 1 && a1[0].props === 'opacity,rotate' && a1[0].dur === 90 && (await heroPath(page)) === TRI, JSON.stringify(a1));
  await page.waitForFunction((r) => document.querySelector('.hero .btn svg path').getAttribute('d') === r, RING, { timeout: 2000 });
  await sleep(250);
  const a2 = await svgAnims(page);
  check('swap: ring in place, one native linear infinite rotate (compositor) is the only animation left', a2.length === 1 && a2[0].props === 'rotate' && a2[0].inf && a2[0].ease === 'linear', JSON.stringify(a2));
  check('swap: text says Starting…, icon still aria-hidden, one path, no inline style or will-change left', await page.evaluate(() => { const b = document.querySelector('.hero .btn'), s = b.querySelector('svg'); return b.textContent.trim() === 'Starting…' && s.getAttribute('aria-hidden') === 'true' && s.querySelectorAll('path').length === 1 && !s.getAttribute('style') && getComputedStyle(s).willChange === 'auto'; }));
  check('swap: no library', anime.length === 0, anime.join());
  state.startStatus = 500; release(); // the start fails: back to the triangle
  await page.waitForFunction((t) => document.querySelector('.hero .btn svg path').getAttribute('d') === t, TRI, { timeout: 3000 });
  await sleep(250);
  check('swap: failed start -> triangle again, no animation left, label restored', (await svgAnims(page)).length === 0 && (await page.evaluate(() => document.querySelector('.hero .btn').textContent.trim())) === name0);
  check('swap: engine idle afterwards', (await rafIn(page, 1000)) <= 2);
  state.startStatus = 200; state.hold = new Promise((r) => (release = r));
  await page.click('.hero .btn'); await page.waitForFunction((r) => document.querySelector('.hero .btn svg path')?.getAttribute('d') === r, RING, { timeout: 3000 });
  await nav(page, '/settings'); await sleep(400);
  const left = await page.evaluate(() => document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition) && !a.effect?.target?.closest?.('aside')).map((a) => a.effect?.target?.outerHTML?.slice(0, 80)));
  check('swap: navigating away mid-spin leaves no script-made animation and no rAF', left.length === 0 && (await rafIn(page, 800)) <= 2, left.join('|'));
  state.startStatus = 500; release();
  await nav(page, '/'); await page.waitForSelector('.btn svg path'); await sleep(600);
  check('swap: Home Resume button has the same icon', await page.evaluate(() => [...document.querySelectorAll('button.btn svg path')].some((p) => p.getAttribute('d') === 'M7 4.5v15l13-7.5z')));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();

  // frames: everything at one tenth speed (CDP), so the 180 ms swap takes 1.8 s
  const f = await mk({ deviceScaleFactor: 6 });
  await f.page.goto(BASE + '/library'); await f.page.waitForSelector('.hero .btn svg path'); await sleep(1000);
  const box = await f.page.locator('.hero .btn svg').boundingBox();
  const clip = { x: box.x - 10, y: box.y - 10, width: box.width + 20, height: box.height + 20 };
  await shot(f.page, 'swap-0-triangle', clip);
  const cdp = await f.ctx.newCDPSession(f.page);
  await cdp.send('Animation.enable'); await cdp.send('Animation.setPlaybackRate', { playbackRate: 0.1 });
  f.state.hold = new Promise(() => {});
  await f.page.click('.hero .btn');
  await sleep(450); await shot(f.page, 'swap-1-out-mid', clip);
  await sleep(550); await shot(f.page, 'swap-2-in-mid', clip);
  await sleep(900); await shot(f.page, 'swap-3-spinning', clip);
  await f.ctx.close();
}

// ---- #11 split-text Library hero title
const wordAnims = (page) => page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.closest?.('.split-fx')).map((a) => ({ props: [...new Set(a.effect.getKeyframes().flatMap((k) => Object.keys(k)))].filter((k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k)).sort().join(), dur: a.effect.getTiming().duration, delay: a.effect.getTiming().delay })));
if (want('split')) {
  const base = await mk({ reducedMotion: 'reduce' });
  await base.page.goto(BASE + '/library'); await base.page.waitForSelector('.hero h3'); await sleep(800);
  const plain = await base.page.evaluate(() => document.querySelector('.hero h3').outerHTML.replace(/ class="[^"]*"/g, ''));
  const plainBox = await base.page.evaluate(() => { const r = document.querySelector('.hero h3').getBoundingClientRect(); return [r.width, r.height]; });
  check('split: reduced motion => plain text, no split copy, no anime.js', !plain.includes('split-fx') && base.anime.length === 0 && (await wordAnims(base.page)).length === 0, plain);
  await base.ctx.close();

  const { page, anime, state, errs } = await mk({ deviceScaleFactor: 2 });
  await page.addInitScript(() => { window.__cls = 0; try { new PerformanceObserver((l) => l.getEntries().forEach((e) => !e.hadRecentInput && e.sources?.some((s) => s.node?.closest?.('.hero h3, .hero')) && (window.__cls += e.value))).observe({ type: 'layout-shift', buffered: true }); } catch {} });
  await page.goto(BASE + '/library'); await page.waitForSelector('.hero h3 .split-fx', { timeout: 4000 });
  await sleep(100);
  const wa = await wordAnims(page);
  check('split: one translate animation per word (2), 500 ms, 45 ms apart', wa.length === 2 && wa.every((a) => a.props === 'translate' && a.dur === 500) && wa[1].delay - wa[0].delay === 45, JSON.stringify(wa));
  const mid = await page.evaluate(() => {
    const h = document.querySelector('.hero h3'), fx = h.querySelector('.split-fx'), real = h.firstElementChild;
    const r = h.getBoundingClientRect();
    return { box: [r.width, r.height], fxHidden: fx.getAttribute('aria-hidden'), realOp: getComputedStyle(real).opacity, realVis: getComputedStyle(real).visibility, words: fx.querySelectorAll('[data-word]').length, clip: [...fx.children].every((c) => getComputedStyle(c).overflow.startsWith('clip')), text: h.textContent };
  });
  check('split: words are clipped by wrappers, copy is aria-hidden, the real text only fades (opacity, not visibility)', mid.words === 2 && mid.clip && mid.fxHidden === 'true' && mid.realOp === '0' && mid.realVis === 'visible', JSON.stringify(mid));
  const align = await page.evaluate(() => {
    const h = document.querySelector('.hero h3'), real = h.firstElementChild, rg = document.createRange();
    rg.selectNodeContents(real);
    const t = rg.getBoundingClientRect();
    const w = [...h.querySelectorAll('.split-fx [data-word]')].map((x) => x.parentElement.getBoundingClientRect());
    return { dx: Math.abs(w[0].left - t.left), dr: Math.abs(w.at(-1).right - t.right), dh: Math.abs(w[0].bottom - t.bottom) };
  });
  check('split: the split copy lines up with the real text (it would otherwise jump when it ends)', align.dx < 1.5 && align.dr < 1.5, JSON.stringify(align));
  check('split: heading keeps its accessible name', (await page.getByRole('heading', { name: 'Portal 2', exact: true }).count()) === 1);
  check('split: no layout shift (heading box unchanged vs plain)', Math.abs(mid.box[0] - plainBox[0]) < 0.5 && Math.abs(mid.box[1] - plainBox[1]) < 0.5, `${mid.box} vs ${plainBox}`);
  await page.waitForFunction(() => !document.querySelector('.hero h3 .split-fx'), null, { timeout: 3000 });
  const after = await page.evaluate(() => document.querySelector('.hero h3').outerHTML.replace(/ class="[^"]*"/g, ''));
  check('split: when it ends the heading is exactly the plain markup again (revert)', after === plain, after);
  check('split: no CLS from the title, no animation left, engine idle', (await page.evaluate(() => window.__cls)) < 0.001 && (await wordAnims(page)).length === 0 && (await rafIn(page, 800)) <= 2);
  check('split: only the lazy text chunks were fetched (no animation/svg chunk)', anime.length > 0 && !anime.some((f) => /^(animation|svg|easings|scope)\./.test(f)), anime.join());
  // once per page mount: data refresh and a search that clears the hero do not replay it
  state.games = games.map((g) => (g.appid === 1000 ? { ...g, playtimeMin: 120 } : g));
  await page.evaluate(() => { window.dispatchEvent(new Event('focus')); document.dispatchEvent(new Event('visibilitychange')); });
  await sleep(600);
  await page.fill('input[type=search]', 'Game 3'); await sleep(300);
  await page.fill('input[type=search]', ''); await page.waitForSelector('.hero h3'); await sleep(300);
  check('split: not again after a data refresh or a search that removes and re-mounts the hero', (await page.evaluate(() => document.querySelectorAll('.split-fx').length)) === 0 && (await wordAnims(page)).length === 0);
  check('no errors', errs.length === 0, errs.join('|'));
  // leaving mid-animation cleans up
  await page.goto(BASE + '/library'); await page.waitForSelector('.hero h3 .split-fx');
  await nav(page, '/settings'); await sleep(300);
  const left = await page.evaluate(() => document.getAnimations().filter((a) => !(a instanceof CSSAnimation) && !(a instanceof CSSTransition) && !a.effect?.target?.closest?.('aside')).length);
  check('split: leaving mid-animation leaves nothing running', left === 0 && (await rafIn(page, 600)) <= 2, `${left}`);
  await page.context().close();

  // pixel check: the copy at the end of its animation must be the real text, to the pixel
  const g = await mk({ deviceScaleFactor: 2 });
  await g.page.goto(BASE + '/library'); await g.page.waitForSelector('.hero h3 .split-fx');
  await g.page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.closest?.('.split-fx')).forEach((a) => a.pause()));
  await sleep(900);
  await g.page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.closest?.('.split-fx')).forEach((a) => (a.currentTime = 700)));
  const hb = await g.page.locator('.hero h3').boundingBox();
  const hclip = { x: hb.x - 4, y: hb.y - 4, width: hb.width + 8, height: hb.height + 8 };
  const A = await g.page.screenshot({ clip: hclip });
  await g.page.evaluate(() => { const h = document.querySelector('.hero h3'); h.querySelector('.split-fx').remove(); h.firstElementChild.style.opacity = ''; });
  const B = await g.page.screenshot({ clip: hclip });
  const diff = await g.page.evaluate(async ([a, b]) => {
    const px = async (s) => { const bm = await createImageBitmap(await (await fetch(`data:image/png;base64,${s}`)).blob()); const c = new OffscreenCanvas(bm.width, bm.height), x = c.getContext('2d'); x.drawImage(bm, 0, 0); return x.getImageData(0, 0, bm.width, bm.height).data; };
    const [p, q] = [await px(a), await px(b)];
    let bad = 0, max = 0;
    for (let i = 0; i < p.length; i += 4) { const d = Math.max(Math.abs(p[i] - q[i]), Math.abs(p[i + 1] - q[i + 1]), Math.abs(p[i + 2] - q[i + 2])); max = Math.max(max, d); if (d > 24) bad++; }
    return { bad, max, total: p.length / 4 };
  }, [A.toString('base64'), B.toString('base64')]);
  check('split: copy at the end of its animation is the real text (under 0.2 % of pixels differ, glyph edges only)', diff.bad < 70, JSON.stringify(diff));
  if (!A.equals(B)) { fs.writeFileSync(path.join(OUT, 'split-diff-copy.png'), A); fs.writeFileSync(path.join(OUT, 'split-diff-real.png'), B); }
  await g.ctx.close();

  // frames at one tenth speed
  const f = await mk({ deviceScaleFactor: 3 });
  await f.page.goto(BASE + '/library'); await f.page.waitForSelector('.hero h3'); await sleep(2000);
  await f.page.reload(); await f.page.waitForSelector('.hero h3');
  const cdp = await f.ctx.newCDPSession(f.page);
  await cdp.send('Animation.enable'); await cdp.send('Animation.setPlaybackRate', { playbackRate: 0.1 });
  await f.page.waitForSelector('.hero h3 .split-fx', { timeout: 4000 }).catch(() => {});
  const b = await f.page.locator('.hero .txt').boundingBox();
  const clip = { x: b.x - 6, y: b.y - 6, width: Math.min(b.width, 520) + 12, height: b.height + 12 };
  await shot(f.page, 'split-1', clip);
  await sleep(1500); await shot(f.page, 'split-2', clip);
  await sleep(1800); await shot(f.page, 'split-3', clip);
  await sleep(3500); await shot(f.page, 'split-end', clip);
  await f.ctx.close();
}

await browser.close();
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
