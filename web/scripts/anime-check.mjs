// anime.js motion pass: assertions + seeked screenshots in headless Chrome against `astro preview` (mocked /api).
//   npx astro preview --host 127.0.0.1 --port 4423 &   then   node scripts/anime-check.mjs [section...]
// Sections: graph stream count revert reduce glide chart flip. Screenshots go to $OUT.
import { chromium } from 'playwright-core';
import fs from 'node:fs';
import path from 'node:path';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4423';
const OUT = process.env.OUT ?? '/tmp/anime-shots';
const DIST = new URL('../dist/', import.meta.url).pathname;
const exe = process.env.BRISK_CHROMIUM ?? `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const only = process.argv.slice(2);
const want = (s) => !only.length || only.includes(s);
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x !== '' ? ` (${x})` : ''}`));
const report = {};

// ---- the anime.js chunks = what motion.ts may load (its __vite__mapDeps list), minus Vite's preload helper
const astro = path.join(DIST, '_astro');
const files = fs.readdirSync(astro).filter((f) => f.endsWith('.js'));
const motionChunk = files.find((f) => /^motion\./.test(f) && fs.readFileSync(path.join(astro, f), 'utf8').includes('prefers-reduced-motion: reduce') && fs.readFileSync(path.join(astro, f), 'utf8').includes('__vite__mapDeps'));
const deps = motionChunk ? [...fs.readFileSync(path.join(astro, motionChunk), 'utf8').match(/m\.f=\[([^\]]*)\]/)[1].matchAll(/"_astro\/([^"]+)"/g)].map((m) => m[1]) : [];
const ANIME = new Set(deps.filter((f) => !/^preload-helper/.test(f)));
const isAnime = (url) => ANIME.has(url.split('/').pop());

if (want('graph')) {
  check('build: motion chunk found and it lazy-loads the anime.js chunks', !!motionChunk && ANIME.size >= 3, `${motionChunk}: ${[...ANIME].join(' ')}`);
  // static import closure of the Stream page (islands, renderers, page scripts); dynamic import() is not followed
  const html = fs.readFileSync(path.join(DIST, 'stream/index.html'), 'utf8');
  const entries = [...html.matchAll(/(?:component-url|renderer-url|src)="\/_astro\/([^"]+\.js)"/g)].map((m) => m[1]);
  const seen = new Set();
  const walk = (f) => {
    if (seen.has(f) || !fs.existsSync(path.join(astro, f))) return;
    seen.add(f);
    const src = fs.readFileSync(path.join(astro, f), 'utf8');
    for (const m of src.matchAll(/(?:from|import)\s*["']\.\/([^"']+\.js)["']/g)) walk(m[1]);
  };
  entries.forEach(walk);
  const bad = [...seen].filter((f) => ANIME.has(f) || f === motionChunk);
  check('import graph: /stream static closure contains no anime.js chunk and not motion.ts', bad.length === 0 && seen.size > 3, `${seen.size} chunks; offending: ${bad.join(',')}`);
  const sizes = {};
  for (const f of ANIME) sizes[f] = +(fs.statSync(path.join(astro, f)).size / 1024).toFixed(1);
  report.animeChunks = sizes;
}

const games = Array.from({ length: 139 }, (_, i) => ({ appid: 1000 + i, name: i === 0 ? 'Portal 2' : `Game ${i}`, installed: i < 40, sizeBytes: 1e9, lastPlayed: i < 5 ? 1700000000 - i * 1e5 : null, playtimeMin: 90, cover: '', running: false }));
const sessions = Array.from({ length: 15 }, (_, i) => ({ appid: 1000, name: 'Portal 2', started: 1700000000000 + i * 1e6, ended: 1700000600000 + i * 1e6, host: 'bawn', preset: 'balanced', codec: 'h264', bitrateKbps: 20000, stats: null, spark: [1, 3, 2, 4, 3] }));
const series = Array.from({ length: 120 }, (_, i) => ({ t: 1700000000000 + i * 5000, fps: 58 + Math.sin(i / 6) * 3, mbps: 18 + Math.cos(i / 9) * 4, delayMs: 9 + Math.sin(i / 4) * 2, lost: 0 }));
const mkHost = () => ({ name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null });

const browser = await chromium.launch({ executablePath: exe });
async function mk(opts = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  const state = { host: mkHost(), games };
  await ctx.route('**/api/**', async (r) => {
    const p = new URL(r.request().url()).pathname;
    const json =
      p === '/api/games' ? { games: state.games, ownedAvailable: true }
      : p === '/api/host' ? state.host
      : p === '/api/stream' ? { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null }
      : p === '/api/prefs' ? { prefs: {} }
      : p === '/api/sessions' ? { sessions }
      : p.startsWith('/api/sessions/') ? { session: sessions[0], series }
      : {};
    await r.fulfill({ json });
  });
  await ctx.addInitScript(() => {
    // rAF callbacks per window (the anime.js engine and any leaked loop show up here), concurrent animations, long tasks, CLS
    window.__raf = 0; window.__maxAnims = 0; window.__long = []; window.__cls = 0;
    const raf = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (cb) => (window.__raf++, raf(cb));
    const sample = () => { window.__maxAnims = Math.max(window.__maxAnims, document.getAnimations().length); setTimeout(sample, 16); };
    sample();
    try { new PerformanceObserver((l) => l.getEntries().forEach((e) => window.__long.push(Math.round(e.duration)))).observe({ type: 'longtask', buffered: true }); } catch {}
    try { new PerformanceObserver((l) => { for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value; }).observe({ type: 'layout-shift', buffered: true }); } catch {}
  });
  const page = await ctx.newPage();
  const errs = [];
  const anime = [];
  page.on('console', (m) => m.type() === 'error' && errs.push(m.text()));
  page.on('pageerror', (e) => errs.push(String(e)));
  page.on('request', (r) => isAnime(r.url()) && anime.push({ url: r.url().split('/').pop(), at: page.url() }));
  return { ctx, page, errs, anime, state };
}
const nav = async (page, href) => {
  await page.click(`aside a.nav[href="${href}"]`);
  await page.waitForFunction((h) => location.pathname.replace(/\/$/, '') === (h === '/' ? '' : h), href);
};
const rafIn = async (page, ms) => { const a = await page.evaluate(() => window.__raf); await sleep(ms); return (await page.evaluate(() => window.__raf)) - a; };
const poll = (page) => page.evaluate(() => document.dispatchEvent(new Event('visibilitychange'))); // HostPill refreshes $host on it

// ---- 1. /stream: zero anime.js bytes and animations, on reload and after client navigation from pages that used it
if (want('stream')) {
  const { page, anime, errs } = await mk();
  await page.goto(BASE + '/stream'); await sleep(1500);
  check('stream reload: no anime.js chunk requested', anime.length === 0, JSON.stringify(anime));
  const desc = () => document.getAnimations().map((a) => `${a.animationName ?? 'waapi'}@${a.effect?.target?.tagName?.toLowerCase()}.${String(a.effect?.target?.className?.baseVal ?? a.effect?.target?.className).split(' ')[0]}${a.effect?.target?.closest?.('aside') ? '(aside)' : ''}`);
  const sa = await page.evaluate(desc);
  check('stream reload: no script-made Web Animation (CSS ones only, e.g. the host dot pulse)', sa.every((x) => !x.startsWith('waapi')), sa.join(' '));
  const { page: p2, anime: a2, state } = await mk();
  await p2.goto(BASE + '/'); await sleep(1200);
  state.host = { ...state.host, gpuTempC: 70 }; await poll(p2); await sleep(150); // a tween on Home loads anime.js
  await nav(p2, '/library'); await sleep(800);
  const before = a2.length;
  await nav(p2, '/stream'); await sleep(1500);
  check('client nav home -> library -> stream: anime.js was used before (sanity) and nothing more is requested on /stream', before > 0 && a2.length === before, `${before} before, ${a2.length - before} on stream`);
  const sb = await p2.evaluate(desc);
  check('stream after client nav: no script-made Web Animation (no glide)', sb.every((x) => !x.startsWith('waapi')), sb.join(' '));
  const r = await rafIn(p2, 1000);
  check('stream after client nav: no rAF from anime (engine idle)', r <= 2, `${r} rAF/s`);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close(); await p2.context().close();
}

// ---- 5. number tweens: no animation on first paint, roll on a later poll, end exactly on the formatted value
if (want('count')) {
  const { page, anime, state, errs } = await mk();
  await page.goto(BASE + '/'); await page.waitForFunction(() => document.querySelector('.stats b.mono')?.textContent === '45 °C'); await sleep(800);
  check('home first paint: temps shown, anime.js not even loaded', anime.length === 0, JSON.stringify(anime));
  state.host = { ...state.host, gpuTempC: 63.4, cpuTempC: 52 };
  await page.evaluate(() => {
    window.__samples = [];
    const el = document.querySelector('.stats b.mono'); const ring = document.querySelector('.ring');
    const t0 = performance.now();
    const s = () => { window.__samples.push([Math.round(performance.now() - t0), el.textContent, ring.style.getPropertyValue('--p')]); if (performance.now() - t0 < 1200) requestAnimationFrame(s); };
    requestAnimationFrame(s);
  });
  await poll(page); await sleep(1300);
  const s = await page.evaluate(() => window.__samples);
  const texts = [...new Set(s.map((x) => x[1]))];
  check('home poll: GPU temp rolls 45 -> 63 through intermediate values', texts.length >= 5 && texts[0] === '45 °C' && texts.at(-1) === '63 °C', texts.join(' '));
  check('home poll: never shows the end value before rolling (no flash)', s.findIndex((x) => x[1] === '63 °C') > s.findIndex((x) => x[1] === '50 °C'), '');
  const ps = s.map((x) => +x[2]).filter((v) => !Number.isNaN(v));
  check('home poll: ring --p sweeps and ends on exactly 63.4', ps.some((v) => v > 46 && v < 62) && ps.at(-1) === 63.4, `${ps[0]} .. ${ps.at(-1)}`);
  check('home poll: all temps final', await page.evaluate(() => [...document.querySelectorAll('.stats b.mono')].slice(0, 2).map((b) => b.textContent).join() === '63 °C,52 °C' && [...document.querySelectorAll('.ring b')].map((b) => b.textContent).join() === '63°,52°'));
  check('home poll: tabular-nums on the rolling numbers', await page.evaluate(() => [...document.querySelectorAll('.stats b')].every((b) => getComputedStyle(b).fontVariantNumeric.includes('tabular-nums'))));
  // rapid updates retarget: three polls 120 ms apart end on the last value
  for (const g of [80, 30, 71.6]) { state.host = { ...state.host, gpuTempC: g }; await poll(page); await sleep(120); }
  await sleep(900);
  check('home rapid polls: ends on the last value only', await page.evaluate(() => document.querySelector('.stats b.mono').textContent === '72 °C' && document.querySelector('.ring').style.getPropertyValue('--p') === '71.6'));
  const idle = await rafIn(page, 1000);
  report.homeIdleRaf = idle;
  // Machines: dd temps roll too
  await nav(page, '/machines'); await sleep(900);
  state.host = { ...state.host, gpuTempC: 50 }; await poll(page);
  await sleep(250);
  const mid = await page.evaluate(() => document.querySelectorAll('dd')[2].textContent);
  await sleep(800);
  check('machines poll: GPU dd rolls and ends exact', mid !== '50 °C' && mid !== '72 °C' && (await page.evaluate(() => document.querySelectorAll('dd')[2].textContent)) === '50 °C', `mid ${mid}`);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- 2. revert on destroy: navigate away mid-animation, then nothing keeps ticking
if (want('revert')) {
  const { page, state, errs } = await mk();
  await page.goto(BASE + '/machines'); await sleep(1200);
  state.host = { ...state.host, gpuTempC: 90 }; await poll(page); await sleep(120);
  await nav(page, '/settings'); await sleep(300);
  const r = await rafIn(page, 1000);
  check('machines -> settings mid-tween: 0 rAF after the page settled (tween cancelled with its component)', r <= 1, `${r} rAF/s`);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- 3. reduced motion: nothing animates, end states correct, anime.js never loads
if (want('reduce')) {
  const { page, anime, state, errs } = await mk({ reducedMotion: 'reduce' });
  await page.goto(BASE + '/'); await sleep(1000);
  state.host = { ...state.host, gpuTempC: 63.4 }; await poll(page); await sleep(60);
  check('reduced: temp is final at once', await page.evaluate(() => document.querySelector('.stats b.mono').textContent === '63 °C' && document.querySelector('.ring').style.getPropertyValue('--p') === '63.4'));
  await nav(page, '/library'); await sleep(600);
  await nav(page, '/settings'); await sleep(600);
  check('reduced: client navigation runs no glide (no Web Animation in the sidebar)', (await page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.closest?.('aside')).length)) === 0);
  check('reduced: anime.js never requested', anime.length === 0, JSON.stringify(anime));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

console.log('report', JSON.stringify(report));
await browser.close();
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
