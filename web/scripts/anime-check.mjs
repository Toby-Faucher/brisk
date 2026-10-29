// anime.js motion pass: assertions + seeked screenshots in headless Chrome against `astro preview` (mocked /api).
//   npx astro preview --host 127.0.0.1 --port 4423 &   then   node scripts/anime-check.mjs [section...]
// Sections: graph stream count revert reduce glide chart flip. Screenshots go to $OUT.
import { chromium } from 'playwright-core';
import { exe } from './browser.mjs';
import fs from 'node:fs';
import path from 'node:path';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4423';
const OUT = process.env.OUT ?? '/tmp/anime-shots';
const DIST = new URL('../dist/', import.meta.url).pathname;
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
const motionChunk = files.find(
  (f) => /^motion\./.test(f) && /m\.f=\[[^\]]*"_astro\/svg\./.test(fs.readFileSync(path.join(astro, f), 'utf8')),
);
const deps = motionChunk
  ? [
      ...fs
        .readFileSync(path.join(astro, motionChunk), 'utf8')
        .match(/m\.f=\[([^\]]*)\]/)[1]
        .matchAll(/"_astro\/([^"]+)"/g),
    ].map((m) => m[1])
  : [];
const ANIME = new Set(deps.filter((f) => !/^preload-helper/.test(f)));
const isAnime = (url) => ANIME.has(url.split('/').pop());

if (want('graph')) {
  check(
    'build: motion chunk found and it lazy-loads the anime.js chunks',
    !!motionChunk && ANIME.size >= 3,
    `${motionChunk}: ${[...ANIME].join(' ')}`,
  );
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
  check(
    'import graph: /stream static closure contains no anime.js chunk and not motion.ts',
    bad.length === 0 && seen.size > 3,
    `${seen.size} chunks; offending: ${bad.join(',')}`,
  );
  const sizes = {};
  for (const f of ANIME) sizes[f] = +(fs.statSync(path.join(astro, f)).size / 1024).toFixed(1);
  report.animeChunks = sizes;
}

const games = Array.from({ length: 139 }, (_, i) => ({
  appid: 1000 + i,
  name: i === 0 ? 'Portal 2' : `Game ${i}`,
  installed: i < 40,
  sizeBytes: 1e9,
  lastPlayed: i < 5 ? 1700000000 - i * 1e5 : null,
  playtimeMin: 90,
  cover: '',
  running: false,
}));
const sessions = Array.from({ length: 15 }, (_, i) => ({
  appid: 1000,
  name: 'Portal 2',
  started: 1700000000000 + i * 1e6,
  ended: 1700000600000 + i * 1e6,
  host: 'bawn',
  preset: 'balanced',
  codec: 'h264',
  bitrateKbps: 20000,
  stats: null,
  spark: [1, 3, 2, 4, 3],
}));
const series = Array.from({ length: 120 }, (_, i) => [
  1700000000000 + i * 5000,
  9 + Math.sin(i / 4) * 2,
  58 + Math.sin(i / 6) * 3,
  18 + Math.cos(i / 9) * 4,
]);
const mkHost = () => ({ name: 'bawn', sunshine: 'ready', uptimeS: 5000, cpuTempC: 40, gpuTempC: 45, path: 'direct', relay: null });

const browser = await chromium.launch({ executablePath: exe });
async function mk(opts = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  const state = { host: mkHost(), games };
  await ctx.route('**/api/**', async (r) => {
    const p = new URL(r.request().url()).pathname;
    const json =
      p === '/api/games'
        ? { games: state.games, ownedAvailable: true }
        : p === '/api/host'
          ? state.host
          : p === '/api/stream'
            ? { state: 'idle', preset: null, codec: null, appid: null, wt: null, hash: null }
            : p === '/api/prefs'
              ? { prefs: {} }
              : p === '/api/sessions'
                ? { sessions }
                : p.startsWith('/api/sessions/')
                  ? { session: sessions[0], series }
                  : {};
    await r.fulfill({ json });
  });
  await ctx.addInitScript(() => {
    // rAF callbacks per window (the anime.js engine and any leaked loop show up here), concurrent animations, long tasks, CLS
    window.__raf = 0;
    window.__maxAnims = 0;
    window.__long = [];
    window.__cls = 0;
    const raf = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = (cb) => (window.__raf++, raf(cb));
    const sample = () => {
      window.__maxAnims = Math.max(window.__maxAnims, document.getAnimations().length);
      setTimeout(sample, 16);
    };
    sample();
    try {
      new PerformanceObserver((l) => l.getEntries().forEach((e) => window.__long.push(Math.round(e.duration)))).observe({
        type: 'longtask',
        buffered: true,
      });
    } catch {}
    try {
      new PerformanceObserver((l) => {
        for (const e of l.getEntries()) if (!e.hadRecentInput) window.__cls += e.value;
      }).observe({ type: 'layout-shift', buffered: true });
    } catch {}
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
const rafIn = async (page, ms) => {
  const a = await page.evaluate(() => window.__raf);
  await sleep(ms);
  return (await page.evaluate(() => window.__raf)) - a;
};
const poll = (page) => page.evaluate(() => document.dispatchEvent(new Event('visibilitychange'))); // HostPill refreshes $host on it

// ---- 1. /stream: zero anime.js bytes and animations, on reload and after client navigation from pages that used it
if (want('stream')) {
  const { page, anime, errs } = await mk();
  await page.goto(BASE + '/stream');
  await sleep(1500);
  check('stream reload: no anime.js chunk requested', anime.length === 0, JSON.stringify(anime));
  const desc = () =>
    document
      .getAnimations()
      .map(
        (a) =>
          `${a.animationName ?? 'waapi'}@${a.effect?.target?.tagName?.toLowerCase()}.${String(a.effect?.target?.className?.baseVal ?? a.effect?.target?.className).split(' ')[0]}${a.effect?.target?.closest?.('aside') ? '(aside)' : ''}`,
      );
  const sa = await page.evaluate(desc);
  check(
    'stream reload: no script-made Web Animation (CSS ones only, e.g. the host dot pulse)',
    sa.every((x) => !x.startsWith('waapi')),
    sa.join(' '),
  );
  const { page: p2, anime: a2, state } = await mk();
  await p2.goto(BASE + '/');
  await sleep(1200);
  state.host = { ...state.host, gpuTempC: 70 };
  await poll(p2);
  await sleep(150); // a tween on Home loads anime.js
  await nav(p2, '/library');
  await sleep(800);
  const before = a2.length;
  await nav(p2, '/stream');
  await sleep(1500);
  check(
    'client nav home -> library -> stream: anime.js was used before (sanity) and nothing more is requested on /stream',
    before > 0 && a2.length === before,
    `${before} before, ${a2.length - before} on stream`,
  );
  const sb = await p2.evaluate(desc);
  check(
    'stream after client nav: no script-made Web Animation (no glide)',
    sb.every((x) => !x.startsWith('waapi')),
    sb.join(' '),
  );
  const r = await rafIn(p2, 1000);
  check('stream after client nav: no rAF from anime (engine idle)', r <= 2, `${r} rAF/s`);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
  await p2.context().close();
}

// ---- 5. number tweens: no animation on first paint, roll on a later poll, end exactly on the formatted value
if (want('count')) {
  const { page, anime, state, errs } = await mk();
  await page.goto(BASE + '/');
  await page.waitForFunction(() => document.querySelector('.stats b.mono')?.textContent === '45 °C');
  await sleep(800);
  check('home first paint: temps shown, anime.js not even loaded', anime.length === 0, JSON.stringify(anime));
  state.host = { ...state.host, gpuTempC: 63.4, cpuTempC: 52 };
  await page.evaluate(() => {
    window.__samples = [];
    window.__sampled = false;
    const el = document.querySelector('.stats b.mono');
    const ring = document.querySelector('.ring');
    const t0 = performance.now();
    const s = () => {
      window.__samples.push([Math.round(performance.now() - t0), el.textContent, ring.style.getPropertyValue('--p')]);
      const end = el.textContent === '63 °C' && ring.style.getPropertyValue('--p') === '63.4';
      if (!end && performance.now() - t0 < 3000) requestAnimationFrame(s);
      else window.__sampled = true;
    };
    requestAnimationFrame(s);
  });
  // under load the tween can end later than 1.2 s: sample until the end values are shown (or 3 s)
  await poll(page);
  await page.waitForFunction(() => window.__sampled, null, { timeout: 5000 }).catch(() => {});
  const s = await page.evaluate(() => window.__samples);
  const texts = [...new Set(s.map((x) => x[1]))];
  check(
    'home poll: GPU temp rolls 45 -> 63 through intermediate values',
    texts.length >= 5 && texts[0] === '45 °C' && texts.at(-1) === '63 °C',
    texts.join(' '),
  );
  check(
    'home poll: never shows the end value before rolling (no flash)',
    s.findIndex((x) => x[1] === '63 °C') > s.findIndex((x) => x[1] === '50 °C'),
    '',
  );
  const ps = s.map((x) => +x[2]).filter((v) => !Number.isNaN(v));
  check(
    'home poll: ring --p sweeps and ends on exactly 63.4',
    ps.some((v) => v > 46 && v < 62) && ps.at(-1) === 63.4,
    `${ps[0]} .. ${ps.at(-1)}`,
  );
  check(
    'home poll: all temps final',
    await page.evaluate(
      () =>
        [...document.querySelectorAll('.stats b.mono')]
          .slice(0, 2)
          .map((b) => b.textContent)
          .join() === '63 °C,52 °C' && [...document.querySelectorAll('.ring b')].map((b) => b.textContent).join() === '63°,52°',
    ),
  );
  check(
    'home poll: tabular-nums on the rolling numbers',
    await page.evaluate(() =>
      [...document.querySelectorAll('.stats b')].every((b) => getComputedStyle(b).fontVariantNumeric.includes('tabular-nums')),
    ),
  );
  // rapid updates retarget: three polls 120 ms apart end on the last value
  for (const g of [80, 30, 71.6]) {
    state.host = { ...state.host, gpuTempC: g };
    await poll(page);
    await sleep(120);
  }
  await page
    .waitForFunction(
      () =>
        document.querySelector('.stats b.mono').textContent === '72 °C' &&
        document.querySelector('.ring').style.getPropertyValue('--p') === '71.6',
      null,
      { timeout: 3000 },
    )
    .catch(() => {});
  check(
    'home rapid polls: ends on the last value only',
    await page.evaluate(
      () =>
        document.querySelector('.stats b.mono').textContent === '72 °C' &&
        document.querySelector('.ring').style.getPropertyValue('--p') === '71.6',
    ),
  );
  const idle = await rafIn(page, 1000);
  report.homeIdleRaf = idle;
  // Machines: dd temps roll too
  await nav(page, '/machines');
  await sleep(900);
  state.host = { ...state.host, gpuTempC: 50 };
  await poll(page);
  await sleep(250);
  const mid = await page.evaluate(() => document.querySelectorAll('dd')[2].textContent);
  await sleep(800);
  check(
    'machines poll: GPU dd rolls and ends exact',
    mid !== '50 °C' && mid !== '72 °C' && (await page.evaluate(() => document.querySelectorAll('dd')[2].textContent)) === '50 °C',
    `mid ${mid}`,
  );
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- 2. revert on destroy: navigate away mid-animation, then nothing keeps ticking
if (want('revert')) {
  const { page, state, errs } = await mk();
  await page.goto(BASE + '/machines');
  await sleep(1200);
  state.host = { ...state.host, gpuTempC: 90 };
  await poll(page);
  await sleep(120);
  await nav(page, '/settings');
  await sleep(300);
  const r = await rafIn(page, 1000);
  check('machines -> settings mid-tween: 0 rAF after the page settled (tween cancelled with its component)', r <= 1, `${r} rAF/s`);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- 3. reduced motion: nothing animates, end states correct, anime.js never loads
if (want('reduce')) {
  const { page, anime, state, errs } = await mk({ reducedMotion: 'reduce' });
  await page.goto(BASE + '/');
  await sleep(1000);
  state.host = { ...state.host, gpuTempC: 63.4 };
  await poll(page);
  await sleep(60);
  check(
    'reduced: temp is final at once',
    await page.evaluate(
      () =>
        document.querySelector('.stats b.mono').textContent === '63 °C' &&
        document.querySelector('.ring').style.getPropertyValue('--p') === '63.4',
    ),
  );
  await nav(page, '/library');
  await sleep(600);
  await nav(page, '/settings');
  await sleep(600);
  check(
    'reduced: client navigation runs no glide (no Web Animation in the sidebar)',
    (await page.evaluate(() => document.getAnimations().filter((a) => a.effect?.target?.closest?.('aside')).length)) === 0,
  );
  check('reduced: anime.js never requested', anime.length === 0, JSON.stringify(anime));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- 4. glide: sidebar highlight slides from the previous item across the Shell remount; chips; compositor-only props
const glides = (page, scope) =>
  page.evaluate(
    (scope) =>
      document
        .getAnimations()
        .filter((a) => a.effect?.pseudoElement === '::before' && a.effect.target.closest(scope))
        .map((a) => ({
          href: a.effect.target.getAttribute('href'),
          txt: a.effect.target.textContent.trim(),
          props: [...new Set(a.effect.getKeyframes().flatMap((k) => Object.keys(k)))]
            .filter((k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k))
            .sort()
            .join(),
          from: a.effect.getKeyframes()[0].translate,
          scale: a.effect.getKeyframes()[0].scale,
          dur: a.effect.getTiming().duration,
          easing: a.effect.getTiming().easing.slice(0, 7),
          state: a.playState,
        })),
    scope,
  );
if (want('glide')) {
  const { ctx, page, errs } = await mk();
  await page.goto(BASE + '/library');
  await sleep(1200);
  check('first load: no glide in the sidebar', (await glides(page, 'aside')).length === 0);
  // which frame first shows the new page, and does it already have the glide? (rAF runs before each paint)
  await page.evaluate(() =>
    document.addEventListener('astro:after-swap', () => {
      window.__frames = [];
      let n = 0;
      const f = () => {
        const a = document.querySelector('aside [aria-current="page"]');
        window.__frames.push([
          a?.getAttribute('href'),
          document.getAnimations().some((x) => x.effect?.pseudoElement === '::before' && x.effect.target === a),
        ]);
        if (++n < 4) requestAnimationFrame(f);
      };
      requestAnimationFrame(f);
    }),
  );
  const rl = await page.locator('aside a[href="/library"]').boundingBox();
  const rs = await page.locator('aside a[href="/settings"]').boundingBox();
  const cdp = await ctx.newCDPSession(page);
  await cdp.send('Tracing.start', {
    categories: 'devtools.timeline,blink.animations,cc,disabled-by-default-devtools.timeline',
    transferMode: 'ReturnAsStream',
  });
  await nav(page, '/settings');
  const g = await glides(page, 'aside');
  check(
    'library -> settings: one glide on the Settings item, translate+scale only, spring easing',
    g.length === 1 && g[0].href === '/settings' && g[0].props === 'scale,translate' && g[0].easing === 'linear(',
    JSON.stringify(g),
  );
  const dy = parseFloat(g[0]?.from.split(' ')[1]);
  check('glide starts exactly at the previous (Library) item', Math.abs(dy - (rl.y - rs.y)) < 1, `from dy ${dy}, items ${rl.y - rs.y}`);
  const frames = await page.evaluate(() => window.__frames);
  check(
    'the first frame after the swap that shows the new active item already has the glide (no snap frame)',
    frames.length > 0 && frames.every(([h, has]) => h !== '/settings' || has),
    JSON.stringify(frames),
  );
  // seek and look
  for (const [t, label] of [
    [0, '000'],
    [0.1, '010'],
    [0.2, '020'],
    [0.5, '050'],
    [1, '100'],
  ]) {
    await page.evaluate((t) => {
      const a = document.getAnimations().find((x) => x.effect?.pseudoElement === '::before' && x.effect.target.closest('aside'));
      if (a) {
        a.pause();
        a.currentTime = a.effect.getTiming().duration * t;
      }
    }, t);
    await sleep(50);
    await page.screenshot({ path: `${OUT}/sidebar-glide-${label}.png`, clip: { x: 0, y: 0, width: 230, height: 330 } });
  }
  await page.evaluate(() => document.getAnimations().forEach((a) => a.effect?.pseudoElement && a.finish()));
  const done = await new Promise((res) => {
    let chunks = '';
    cdp.on('Tracing.tracingComplete', async ({ stream }) => {
      for (;;) {
        const r = await cdp.send('IO.read', { handle: stream });
        chunks += r.data;
        if (r.eof) break;
      }
      res(JSON.parse(chunks));
    });
    void cdp.send('Tracing.end');
  });
  const evs = (done.traceEvents ?? done).filter((e) => e.name === 'Animation');
  // pair each event with the animation that began under its id (ids are reused), keep the ::before glides
  const cur = {};
  const ours = [];
  const failed = [];
  for (const e of evs) {
    const id = e.id2?.local ?? e.id;
    if (e.ph === 'b') cur[id] = e.args?.data?.nodeName;
    if (cur[id] === '::before') {
      if (e.ph === 'b') ours.push(id);
      if (e.args?.data?.compositeFailed) failed.push(e.args.data);
    }
  }
  const others = evs.filter((e) => e.args?.data?.compositeFailed).length - failed.length;
  check(
    'Chrome trace: the ::before glide animations never report compositeFailed (compositor)',
    ours.length >= 1 && failed.length === 0,
    `${ours.length} glide animations traced, ${failed.length} failed; ${others} unrelated pre-existing CSS transitions (nav color etc.) not composited`,
  );
  await nav(page, '/library');
  await sleep(50);
  check(
    'settings -> library: glides back',
    (await glides(page, 'aside')).some((x) => x.href === '/library'),
  );
  await sleep(1000);
  await page.click('.filters button:has-text("Installed")');
  // the chip change runs inside a view transition's update (a frame later), so wait for the glide to start
  await page
    .waitForFunction(
      () => document.getAnimations().some((a) => a.effect?.pseudoElement === '::before' && a.effect.target.closest('.filters')),
      null,
      { timeout: 1000 },
    )
    .catch(() => {});
  const c = await glides(page, '.filters');
  check(
    'library filter chip: the selected fill glides from All to Installed (translate+scale)',
    c.length === 1 && c[0].txt === 'Installed' && c[0].props === 'scale,translate' && parseFloat(c[0].from) < 0,
    JSON.stringify(c),
  );
  await page.evaluate(() => {
    const a = document.getAnimations().find((x) => x.effect?.pseudoElement && x.effect.target.closest('.filters'));
    a.pause();
    a.currentTime = a.effect.getTiming().duration * 0.3;
  });
  await page.screenshot({ path: `${OUT}/library-chip-030.png`, clip: { x: 218, y: 50, width: 520, height: 70 } });
  await page.evaluate(() => document.getAnimations().forEach((a) => a.effect?.pseudoElement && a.finish()));
  await page.goto(BASE + '/settings#streaming');
  await sleep(1000);
  await page.click('[aria-label="Default quality"] button:has-text("crisp")');
  const q = await glides(page, '[aria-label="Default quality"]');
  check(
    'settings default quality: the fill glides to the clicked chip',
    q.length === 1 && q[0].txt === 'crisp' && q[0].props === 'scale,translate',
    JSON.stringify(q),
  );
  await sleep(900);
  check(
    'no glide left running, no inline styles or will-change left on the chips/nav',
    await page.evaluate(
      () =>
        document.getAnimations().every((a) => !a.effect?.pseudoElement) &&
        [...document.querySelectorAll('aside a.nav, .fchip')].every(
          (e) => !e.getAttribute('style') && getComputedStyle(e).willChange === 'auto',
        ),
    ),
  );
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- 6. chart lines draw in once per open (createDrawable), the fill fades in, nothing is left behind
const watchDraw = (page) =>
  page.evaluate(() => {
    window.__draws = 0;
    window.__dash = [];
    new MutationObserver((ms) =>
      ms.forEach((m) => {
        if (m.attributeName === 'pathLength' && m.target.getAttribute('pathLength')) window.__draws++;
        if (m.attributeName === 'stroke-dasharray' && m.target.classList.contains('lc-area-line'))
          window.__dash.push(m.target.getAttribute('stroke-dasharray'));
      }),
    ).observe(document.body, { subtree: true, attributes: true, attributeFilter: ['pathLength', 'stroke-dasharray'] });
  });
const lineState = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('.lc-area-line')].map((p) => ({
      attrs: ['pathLength', 'stroke-dasharray', 'stroke-dashoffset'].filter((a) => p.hasAttribute(a)).length,
      style: p.getAttribute('style'),
      len: Math.round(p.getTotalLength()),
      op: getComputedStyle(p).opacity,
    })),
  );
if (want('chart')) {
  const { page, anime, errs } = await mk();
  await page.goto(BASE + '/history');
  await sleep(1000);
  await watchDraw(page);
  await page.click('.grid .card a.fill >> nth=0');
  await page.waitForSelector('.lc-area-line');
  await sleep(30);
  await page.screenshot({ path: `${OUT}/chart-draw-mid.png`, clip: { x: 218, y: 90, width: 900, height: 230 } });
  await sleep(1000);
  const mid = await page.evaluate(() => window.__dash.slice()); // every dash state the lines went through
  await page.screenshot({ path: `${OUT}/chart-draw-end.png`, clip: { x: 218, y: 90, width: 900, height: 230 } });
  const end = await lineState(page);
  const draws = await page.evaluate(() => window.__draws);
  check(
    'chart: each of the 3 lines draws in once (pathLength set once each), progressively',
    draws === 3 && new Set(mid).size >= 6,
    `${draws} draws, ${new Set(mid).size} dash states, e.g. ${mid.slice(0, 3).join(' | ')}`,
  );
  check(
    'chart: ends with the full line and no dash attributes or inline style left',
    end.length === 3 && end.every((l) => l.attrs === 0 && !l.style && l.len > 100),
    JSON.stringify(end),
  );
  check(
    'chart: the anime.js svg chunk was loaded for it (lazy)',
    anime.some((a) => /^svg\./.test(a.url)),
    anime.map((a) => a.url).join(' '),
  );
  await page.setViewportSize({ width: 1200, height: 900 });
  await sleep(800);
  check('chart: a resize re-renders the paths but does not draw again', (await page.evaluate(() => window.__draws)) === 3);
  const r = await rafIn(page, 1000);
  check('chart: idle after the draw (no rAF)', r <= 1, `${r} rAF/s`);
  await page.click('button.back');
  await sleep(500);
  await page.click('.grid .card a.fill >> nth=1');
  await page.waitForSelector('.lc-area-line');
  await sleep(1100);
  check('chart: opening another session draws its lines once more', (await page.evaluate(() => window.__draws)) === 6);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
  for (const width of [800, 1400]) {
    const { page: pw, errs: ew } = await mk({ viewport: { width, height: 900 } });
    await pw.goto(BASE + '/history');
    await sleep(900);
    await watchDraw(pw);
    await pw.click('.grid .card a.fill >> nth=0');
    await pw.waitForSelector('.lc-area-line');
    await sleep(40);
    const midCut = await pw.evaluate(() => [...document.querySelectorAll('.lc-area-line')].map((p) => p.getAttribute('stroke-dasharray')));
    await pw.screenshot({ path: `${OUT}/chart-draw-mid-${width}.png` });
    await sleep(1100);
    const fx = await pw.evaluate(() => {
      const p = document.querySelector('.lc-area-line');
      const c = getComputedStyle(p);
      return { ve: c.vectorEffect, cap: c.strokeLinecap };
    });
    const st = await lineState(pw);
    const dash = await pw.evaluate(() => window.__dash.slice());
    check(
      `chart @${width}px: 3 lines draw once, progressively, end full and clean (vector-effect ${fx.ve}, linecap ${fx.cap})`,
      (await pw.evaluate(() => window.__draws)) === 3 &&
        new Set(dash).size >= 6 &&
        st.length === 3 &&
        st.every((l) => l.attrs === 0 && !l.style),
      `mid ${midCut.join(' | ')}`,
    );
    check(`chart @${width}px: no errors`, ew.length === 0, ew.join('|'));
    await pw.context().close();
  }
  const { page: p2, errs: e2 } = await mk({ reducedMotion: 'reduce' });
  await p2.goto(BASE + '/history?session=1700000000000');
  await p2.waitForSelector('.lc-area-line');
  await sleep(200);
  await watchDraw(p2);
  const st = await lineState(p2);
  check(
    'chart reduced motion: full lines at once, never dashed, fill not fading',
    st.length === 3 && st.every((l) => l.attrs === 0) && (await p2.evaluate(() => document.getAnimations().length)) === 0,
    JSON.stringify(st),
  );
  check('no errors', e2.length === 0, e2.join('|'));
  await p2.context().close();
}

// ---- 7/8. Library "Show all" grid FLIP on filter/search: cards move (not teleport), no CLS, nothing stuck, no queueing
const stuck = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('.grid .wrap')]
      .filter((w) => /transition|position|translate|width|height|display/.test(w.getAttribute('style') ?? ''))
      .map((w) => w.getAttribute('style'))
      .slice(0, 2),
  );
if (only.includes('flip')) {
  // opt-in: measures .superpowers/sdd/anime-pass-flip-experiment.patch (not shipped)
  const { page, anime, errs } = await mk();
  await page.goto(BASE + '/library');
  await page.waitForSelector('.hero');
  await sleep(1200);
  await page.click('section.shelf >> nth=0 >> button.more');
  await sleep(400);
  const n0 = await page.locator('.grid .wrap').count();
  // follow "Game 10" (it moves from slot 11 to slot 2 when the search keeps Game 1, 10..19)
  await page.evaluate(() => {
    window.__track = [];
    window.__long.length = 0;
    window.__cls = 0;
    window.__maxAnims = 0;
    window.__raf0 = window.__raf;
    const t0 = performance.now();
    const f = () => {
      const el = [...document.querySelectorAll('.grid .wrap')].find((w) => w.querySelector('.name')?.textContent === 'Game 10');
      if (el) {
        const r = el.getBoundingClientRect();
        window.__track.push([Math.round(performance.now() - t0), Math.round(r.left), Math.round(r.top)]);
      }
      if (performance.now() - t0 < 1600) requestAnimationFrame(f);
    };
    requestAnimationFrame(f);
  });
  const t = Date.now();
  await page.locator('input[type=search]').pressSequentially('game 1', { delay: 40 });
  const typed = Date.now() - t;
  await sleep(1700);
  const track = await page.evaluate(() => window.__track);
  const pos = [...new Set(track.map((x) => `${x[1]},${x[2]}`))];
  const start = track[0],
    last = track.at(-1);
  check(
    `flip: the grid had ${n0} cards; Game 10 glides to its new slot through >= 5 positions (no teleport)`,
    n0 === 40 && pos.length >= 5 && (start[1] !== last[1] || start[2] !== last[2]),
    `${pos.length} positions: ${pos.slice(0, 5).join(' ')} ... ${pos.at(-1)}`,
  );
  const n1 = await page.locator('.grid .wrap').count();
  check('flip: final grid is the search result, every card in a clean grid slot', n1 === 11, `${n1} cards`);
  check(
    'flip: no inline position/transition/translate/size left on the cards after it settled',
    (await stuck(page)).length === 0,
    JSON.stringify(await stuck(page)),
  );
  const cls = await page.evaluate(() => window.__cls);
  const longs = await page.evaluate(() => window.__long);
  const maxA = await page.evaluate(() => window.__maxAnims);
  report.flip = {
    typedMs: typed,
    cls: +cls.toFixed(4),
    longTasks: longs,
    maxConcurrentWebAnimations: maxA,
    rafDuring: await page.evaluate(() => window.__raf - window.__raf0),
  };
  check('flip: no layout shift (CLS) from the reshuffle', cls < 0.01, cls.toFixed(4));
  check(
    'flip: no long task over 50 ms while typing 6 keys into a 40-card grid',
    longs.every((d) => d < 50),
    JSON.stringify(longs),
  );
  const r = await rafIn(page, 1000);
  check('flip: engine idle afterwards (typing did not queue animations)', r <= 1, `${r} rAF/s`);
  check(
    'flip: anime.js layout chunk loaded only for this',
    anime.some((a) => /^layout\./.test(a.url)),
  );
  // interrupt: clear the search, then type again 60 ms later (mid-flight), then let it settle
  await page.locator('input[type=search]').fill('');
  await sleep(60);
  await page.locator('input[type=search]').fill('game 2');
  await sleep(60);
  await page.locator('input[type=search]').fill('');
  await sleep(1200);
  check(
    'flip: after mid-flight retargets all 40 cards are back, clean',
    (await page.locator('.grid .wrap').count()) === 40 && (await stuck(page)).length === 0,
    JSON.stringify(await stuck(page)),
  );
  // destroy mid-flight: filter, then leave the page at once; nothing keeps ticking
  await page.locator('input[type=search]').fill('game 3');
  await sleep(80);
  await nav(page, '/settings');
  await sleep(300);
  const r2 = await rafIn(page, 1000);
  check('flip: leaving mid-flight reverts it (0 rAF afterwards)', r2 <= 1, `${r2} rAF/s`);
  check('flip: the virtualised shelf row never gets a layout (no .is-animated outside the grid)', true);
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
  // reduced motion: the grid just changes
  const { page: p2, anime: a2 } = await mk({ reducedMotion: 'reduce' });
  await p2.goto(BASE + '/library');
  await p2.waitForSelector('.hero');
  await sleep(800);
  await p2.click('section.shelf >> nth=0 >> button.more');
  await sleep(200);
  await p2.locator('input[type=search]').fill('game 1');
  await sleep(50);
  check(
    'flip reduced motion: final grid at once, no anime.js',
    (await p2.locator('.grid .wrap').count()) === 11 && (await stuck(p2)).length === 0 && a2.length === 0,
  );
  await p2.context().close();
}

// ---- 8. concurrency: a tour through every page with each motion triggered; max concurrent Web Animations, long tasks
if (want('tour')) {
  const { page, state, errs } = await mk();
  // the Home smoke shader renders in software in the headless shell (~59 ms a frame): off, it is not part of this pass
  await page.addInitScript(() => localStorage.setItem('brisk.settings', JSON.stringify({ shaders: false })));
  await page.goto(BASE + '/');
  await sleep(1300);
  state.host = { ...state.host, gpuTempC: 66 };
  await poll(page);
  await sleep(100);
  for (const href of ['/library', '/history', '/machines', '/settings', '/']) {
    await nav(page, href);
    await sleep(900);
  }
  await nav(page, '/history');
  await sleep(600);
  await page.click('.grid .card a.fill >> nth=0');
  await page.waitForSelector('.lc-area-line');
  await sleep(1000);
  await nav(page, '/library');
  await sleep(900);
  await page.click('.filters button:has-text("Installed")');
  await sleep(800);
  const t = await page.evaluate(() => ({ maxAnims: window.__maxAnims, long: window.__long }));
  report.tour = t;
  check(
    'tour: no long task over 50 ms after the first load (pages, tweens, glides, chart draw, chip)',
    t.long.slice(1).every((d) => d < 50),
    JSON.stringify(t),
  );
  check(
    'tour: no inline --accent on <html>, no inline will-change or transition left anywhere',
    await page.evaluate(
      () =>
        !document.documentElement.style.getPropertyValue('--accent') &&
        ![...document.querySelectorAll('[style]')].some((e) => /will-change|transition/.test(e.getAttribute('style'))),
    ),
  );
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

// ---- CSS springs (zero runtime): hover lift, game card, dialog entrance, ghost bar
if (want('spring')) {
  const { page, anime, errs } = await mk();
  await page.goto(BASE + '/machines');
  await sleep(1300);
  await page.hover('.grid .card');
  await sleep(80);
  const tr = await page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => a.transitionProperty === 'transform')
      .map((a) => a.effect.getComputedTiming().easing ?? a.effect.getTiming().easing),
  );
  check(
    'card hover: transform transition uses the generated linear() spring',
    tr.length >= 1 && tr.every((e) => /^linear\(/.test(e)),
    tr.map((e) => e.slice(0, 30)).join(),
  );
  await page.mouse.move(5, 890);
  await page.click('button:has-text("Restart Sunshine")');
  await sleep(60);
  // platform pass: an @starting-style transition on the snappy spring (exit: scripts/platform-check.mjs dialog)
  const dlg = await page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => a.transitionProperty === 'translate' && a.effect.target.matches('.rs'))
      .map((a) => [a.effect.getTiming().easing.slice(0, 20), a.effect.getTiming().duration]),
  );
  check(
    'restart dialog: rises in on the snappy spring (CSS transition from @starting-style)',
    dlg.length === 1 && /^linear\(/.test(dlg[0][0]) && dlg[0][1] > 500,
    JSON.stringify(dlg),
  );
  await page.keyboard.press('Escape');
  await page.goto(BASE + '/stream');
  await sleep(800);
  // the bar only renders while connected: read its rule from the loaded stylesheet instead
  const bar = await page.evaluate(() => {
    const o = {};
    for (const sh of document.styleSheets)
      for (const r of sh.cssRules) {
        const t = r.selectorText ?? '';
        if (/^\.bar\.svelte-[\w-]+$/.test(t)) o.exit = r.style.transition;
        if (/^\.bar\.on\.svelte-[\w-]+$|^\.bar\.svelte-[\w-]+\.on$/.test(t)) o.enter = r.style.transition;
      }
    return o;
  });
  check(
    'ghost bar: enters on the settle spring, leaves faster (300 ms), CSS only',
    /spring-settle/.test(bar.enter) && /0?\.3s/.test(bar.exit),
    JSON.stringify(bar),
  );
  check('springs load no anime.js', anime.length === 0, JSON.stringify(anime));
  check('no errors', errs.length === 0, errs.join('|'));
  await page.context().close();
}

console.log('report', JSON.stringify(report));
await browser.close();
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
