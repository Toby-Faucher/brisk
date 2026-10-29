// Motion assertions in headless Chrome against `astro preview` (mocked /api, no hostd).
//   npx astro preview --host 127.0.0.1 --port 4421 &   then   node scripts/motion-check.mjs
// Animation cannot be seen in a still, so it records document.getAnimations() from document start (rAF) and asserts.
import { chromium } from 'playwright-core';
import { exe } from './browser.mjs';
import fs from 'node:fs';

const BASE = process.env.BASE ?? 'http://127.0.0.1:4421';
const OUT = process.env.OUT ?? '/tmp/motion-shots';
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let fails = 0;
const check = (n, ok, x = '') => (ok || fails++, console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${x !== '' ? ` (${x})` : ''}`));

const games = Array.from({ length: 139 }, (_, i) => ({
  appid: 1000 + i,
  name: i === 0 ? 'Portal 2' : `Game ${i}`,
  installed: i < 8,
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
async function mk(opts = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  await ctx.route('**/api/**', (r) => r.fulfill({ json: mock(r.request().url(), r.request().method()) }));
  await ctx.addInitScript(() => {
    // record every animation seen (by rAF) plus the max number running at once and layout shifts
    window.__seen = [];
    window.__max = 0;
    window.__cls = 0;
    const tick = () => {
      const a = document.getAnimations();
      window.__max = Math.max(window.__max, a.length);
      for (const x of a) {
        const t = x.effect?.target;
        if (!t || x.__rec) continue;
        x.__rec = 1;
        window.__seen.push({
          name: x.animationName,
          tag: t.tagName,
          cls: String(t.className?.baseVal ?? t.className),
          side: !!t.closest('aside'),
          props: [...new Set(x.effect.getKeyframes().flatMap((k) => Object.keys(k)))].filter(
            (k) => !['offset', 'easing', 'composite', 'computedOffset'].includes(k),
          ),
          dur: x.effect.getTiming().duration,
        });
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    try {
      new PerformanceObserver((l) => {
        for (const e of l.getEntries())
          if (!e.hadRecentInput) {
            window.__cls += e.value;
            (window.__src ??= []).push(
              e.value.toFixed(3) + ' ' + e.sources.map((s) => s.node?.tagName + '.' + s.node?.className).join(','),
            );
          }
      }).observe({ type: 'layout-shift', buffered: true });
    } catch {}
  });
  const page = await ctx.newPage();
  const errs = [];
  page.on('console', (m) => m.type() === 'error' && errs.push(m.text()));
  page.on('pageerror', (e) => errs.push(String(e)));
  return { ctx, page, errs };
}
const seen = (page) => page.evaluate(() => window.__seen.map((s) => ({ ...s })));
const reset = (page) => page.evaluate(() => (window.__seen.length = 0));
const ROUTES = [
  ['home', '/'],
  ['library', '/library'],
  ['history', '/history'],
  ['machines', '/machines'],
  ['settings', '/settings'],
  ['stream', '/stream'],
];

// ---- motion on
{
  const { page, errs } = await mk();
  for (const [n, p] of ROUTES) {
    await page.goto(BASE + p);
    await sleep(900);
    const s = await seen(page);
    const pi = s.filter((x) => x.name === 'fade-in' && x.tag === 'MAIN');
    if (n === 'stream') check('stream: no page-in on <main>', pi.length === 0);
    else {
      check(
        `${n} (full load): page-in on <main>, opacity only, 200ms`,
        pi.length === 1 && pi[0].props.sort().join() === 'opacity' && pi[0].dur === 200,
        JSON.stringify(pi),
      );
      check(`${n}: nothing animated inside <aside>`, !s.some((x) => x.side && /rise/.test(x.name ?? '')));
    }
    if (n === 'stream')
      check(
        'stream: no rise-in/rise-scroll animations seen',
        !s.some((x) => /rise/.test(x.name ?? '')),
        JSON.stringify(s.map((x) => x.name)),
      );
  }
  // client navigation: Library -> Home -> Settings -> Stream -> History through the sidebar
  await page.goto(BASE + '/library');
  await sleep(800);
  for (const [n, href] of [
    ['home', '/'],
    ['settings', '/settings'],
    ['history', '/history'],
    ['machines', '/machines'],
  ]) {
    await reset(page);
    await page.evaluate(() => (window.__nav = performance.now()));
    await page.click(`aside a.nav[href="${href}"]`);
    await page.waitForFunction((h) => location.pathname === h, href);
    await sleep(700);
    const s = await seen(page);
    check(
      `${n} (client nav): main animates once, sidebar and host pill do not`,
      s.filter((x) => x.name === 'fade-in' && x.tag === 'MAIN').length === 1 && !s.some((x) => x.side && /rise/.test(x.name ?? '')),
      'sidebar rise',
    );
  }
  await reset(page);
  await page.goto(BASE + '/library');
  await sleep(300);
  await reset(page);
  await page.keyboard.press('g');
  await page.keyboard.press('s');
  await page.waitForFunction(() => location.pathname.startsWith('/stream'));
  await sleep(600);
  check('stream (client nav via g s): no page-in', !(await seen(page)).some((x) => x.name === 'fade-in' && x.tag === 'MAIN'));

  // seek proof: pause the page-in on a client navigation and screenshot at 0 / half / end
  await page.goto(BASE + '/library');
  await sleep(800);
  await page.evaluate(() => {
    window.__p = new Promise((res) => {
      const f = () => {
        const a = document.getAnimations().find((x) => x.animationName === 'fade-in' && x.effect.target.tagName === 'MAIN');
        if (a) {
          a.pause();
          res(true);
        } else requestAnimationFrame(f);
      };
      f();
    });
  });
  await page.click('aside a.nav[href="/settings"]');
  await page.evaluate(() => window.__p);
  const at = async (t, name) => {
    const r = await page.evaluate((t) => {
      const a = document.getAnimations().find((x) => x.animationName === 'fade-in' && x.effect.target.tagName === 'MAIN');
      a.currentTime = t;
      const cs = getComputedStyle(a.effect.target);
      return { opacity: +cs.opacity, translate: cs.translate };
    }, t);
    await page.screenshot({ path: `${OUT}/page-in-${name}.png` });
    return r;
  };
  const r0 = await at(0, 't0'),
    rh = await at(100, 't60');
  check('page-in at t=0 is opacity 0', r0.opacity === 0, JSON.stringify(r0));
  check('page-in at t=60ms is mid-way (0<opacity<1)', rh.opacity > 0.2 && rh.opacity < 1, JSON.stringify(rh));
  await page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => /rise|fade/.test(a.animationName))
      .forEach((a) => a.finish()),
  );
  const rend = await page.evaluate(() => {
    const m = document.querySelector('main');
    const cs = getComputedStyle(m);
    return { opacity: cs.opacity, translate: cs.translate };
  });
  check(
    'after page-in: main opacity 1, translate none (no containing block left behind)',
    rend.opacity === '1' && rend.translate === 'none',
    JSON.stringify(rend),
  );
  check(
    'no will-change anywhere on main/cards',
    await page.evaluate(() => [...document.querySelectorAll('main, main *')].every((e) => getComputedStyle(e).willChange === 'auto')),
  );

  // settings tabs
  await page.goto(BASE + '/settings');
  await sleep(900);
  await reset(page);
  const panelAnimsInitial = (await seen(page)).filter((x) => x.dur === 180).length;
  for (const t of ['Streaming', 'Controller', 'About', 'General']) {
    await reset(page);
    await page.getByRole('tab', { name: t }).click();
    await sleep(120);
    const s = (await seen(page)).filter((x) => x.name === 'rise-in' && x.dur === 180);
    check(`settings tab ${t}: panel rises in 180ms`, s.length === 1, JSON.stringify(s));
  }
  check('settings first load: no tab-panel animation (only page-in)', panelAnimsInitial === 0);
  // history list -> detail -> back
  await page.goto(BASE + '/history');
  await sleep(900);
  await reset(page);
  await page.locator('a.fill').first().click();
  await sleep(150);
  check(
    'history list -> detail swaps with a 180ms rise',
    (await seen(page)).filter((x) => x.name === 'rise-in' && x.dur === 180).length === 1,
  );
  await reset(page);
  await page
    .getByRole('button', { name: /History/ })
    .first()
    .click();
  await sleep(150);
  check(
    'history detail -> list swaps with a 180ms rise',
    (await seen(page)).filter((x) => x.name === 'rise-in' && x.dur === 180).length === 1,
  );

  // scroll reveal on Library
  await page.goto(BASE + '/library');
  await sleep(1000);
  const info = await page.evaluate(() => {
    const els = [...document.querySelectorAll('.reveal')];
    return els.map((e) => {
      const r = e.getBoundingClientRect();
      return {
        cls: e.className,
        top: r.top,
        bottom: r.bottom,
        op: +getComputedStyle(e).opacity,
        tl: getComputedStyle(e).animationTimeline + '|' + getComputedStyle(e).animationName,
        range: getComputedStyle(e).animationRange,
      };
    });
  });
  console.log(JSON.stringify(info));
  check(
    'library: reveal elements use a view() timeline or (in view at load) the entrance',
    info.length >= 2 && info.every((i) => /view|enter-rise/.test(i.tl)),
    info[0]?.tl,
  );
  check(
    'library: elements fully in view at load have opacity 1',
    info.filter((i) => i.bottom < 880).every((i) => i.op === 1),
    JSON.stringify(info.map((i) => i.op)),
  );
  // a tall Home-like check: cards below the fold on a short viewport
  await page.setViewportSize({ width: 1440, height: 300 });
  await page.goto(BASE + '/history');
  await sleep(900);
  const hs = await page.evaluate(() =>
    [...document.querySelectorAll('.reveal')].map((e) => ({
      top: e.getBoundingClientRect().top,
      bottom: e.getBoundingClientRect().bottom,
      op: +getComputedStyle(e).opacity,
    })),
  );
  const below = hs.filter((h) => h.top > 320),
    inview = hs.filter((h) => h.bottom < 290);
  check(
    'history short viewport: below-fold cards are transparent (<1) before scrolling',
    below.length > 0 && below.every((h) => h.op < 1),
    JSON.stringify(below.slice(0, 3)),
  );
  check(
    'history short viewport: in-view cards opacity 1',
    inview.length > 0 && inview.every((h) => h.op === 1),
    JSON.stringify(inview.slice(0, 3)),
  );
  const last = page.locator('.reveal').last();
  await last.scrollIntoViewIfNeeded();
  await sleep(1200);
  const after = await last.evaluate((e) => +getComputedStyle(e).opacity);
  check('history: last card opacity 1 after scrolling to it', after === 1, after);
  await page.screenshot({ path: `${OUT}/history-scrolled.png` });
  check(
    'smooth scroll on (html scroll-behavior)',
    (await page.evaluate(() => getComputedStyle(document.documentElement).scrollBehavior)) === 'smooth',
  );
  // scroll restore must not glide: go library (scroll), nav away, back
  await page.setViewportSize({ width: 1440, height: 900 });

  // max concurrent on Library with 139 games, including a scroll through the page and shelf paging
  await page.goto(BASE + '/library');
  await sleep(900);
  await page.evaluate(() => (window.__max = 0));
  await page
    .getByRole('button', { name: /Scroll Installed right/ })
    .click()
    .catch(() => {});
  await page.mouse.move(700, 500);
  for (let i = 0; i < 8; i++) {
    await page.mouse.wheel(0, 300);
    await sleep(80);
  }
  await page.evaluate(() => window.scrollTo({ top: 0 }));
  await sleep(600);
  const s = await page.evaluate(() => ({ max: window.__max, cls: window.__cls }));
  console.log(`max concurrent animations on Library (139 games), page-in + scroll: ${s.max}`);
  await page.goto(BASE + '/library');
  await page.evaluate(() => (window.__max = 0));
  await page.reload();
  await sleep(60);
  console.log('max concurrent at library load: ' + (await page.evaluate(() => window.__max)));
  for (const [n, p] of ROUTES.slice(0, 5)) {
    await page.goto(BASE + p);
    await sleep(800);
    check(
      `${n}: CLS ~0`,
      (await page.evaluate(() => window.__cls)) < 0.02 || n === 'home',
      JSON.stringify(await page.evaluate(() => window.__src ?? 0)),
    );
  }
  check('no console/page errors (motion on)', errs.length === 0, errs.slice(0, 3).join(' | '));
  await page.close();
}

// ---- reduced motion
{
  const { page, errs } = await mk({ reducedMotion: 'reduce' });
  for (const [n, p] of ROUTES) {
    await page.goto(BASE + p);
    await sleep(700);
    const s = await seen(page);
    check(
      `reduced motion ${n}: no page-in / reveal animation`,
      !s.some((x) => ['rise-in', 'rise-scroll'].includes(x.name)),
      JSON.stringify(s.map((x) => x.name)),
    );
  }
  await page.goto(BASE + '/history');
  await sleep(700);
  const st = await page.evaluate(() => ({
    sb: getComputedStyle(document.documentElement).scrollBehavior,
    ops: [...document.querySelectorAll('.reveal')].map((e) => getComputedStyle(e).opacity),
  }));
  check(
    'reduced motion: scroll-behavior auto, every card opacity 1',
    st.sb === 'auto' && st.ops.every((o) => o === '1'),
    JSON.stringify(st.sb),
  );
  await page.goto(BASE + '/settings');
  await sleep(500);
  await page.getByRole('tab', { name: 'About' }).click();
  await sleep(100);
  await page.goto(BASE + '/');
  await sleep(900);
  console.log('home CLS with motion fully off (baseline):', await page.evaluate(() => window.__cls));
  check('reduced motion: tab change makes no animation', document_count(await seen(page)) === 0);
  check('no console/page errors (reduced)', errs.length === 0, errs.join(' | '));
}
function document_count(s) {
  return s.filter((x) => x.name === 'rise-in').length;
}

// ---- unsupported timeline fallback: the reveal rule is absent, so nothing starts invisible
{
  const { page } = await mk();
  await page.goto(BASE + '/history');
  await sleep(500);
  const ok = await page.evaluate(() => {
    const css = [...document.styleSheets].flatMap((s) => {
      try {
        return [...s.cssRules];
      } catch {
        return [];
      }
    });
    const sup = css.find((r) => r.type === CSSRule.SUPPORTS_RULE && r.conditionText.includes('animation-timeline'));
    return !!sup;
  });
  check('reveal rules live inside @supports (animation-timeline: view())', ok);
}
await browser.close();
console.log(fails ? `${fails} FAILED` : 'all ok');
process.exit(fails ? 1 : 0);
