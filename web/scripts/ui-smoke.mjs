// UI smoke test: hostd (fixtures + fake bridge + mock Steam API, sentinel key) serving web/dist,
// driven by headless Chrome. Needs 127.0.0.1:8443 free (hostd binds it and checks Host against it; both are fixed in
// code) and a built web/dist. Exit codes: 0 pass, 1 fail, 3 skipped because 8443 is taken (the live hostd service;
// BRISK_SMOKE_ALLOW_SKIP=1 turns a skip into exit 0). The axe rules it applies are also checked without hostd by
// scripts/axe-check.mjs.
//   node scripts/ui-smoke.mjs        (env: BRISK_HOSTD=path to brisk-hostd, BRISK_CHROMIUM=chrome-headless-shell)
// It cannot prove video: a real WebTransport connect fails against the fake bridge, so the Stream
// page is only checked for degrading to its reconnecting/failed state without page errors.
import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import http from 'node:http';
import net from 'node:net';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { axe } from './axe.mjs';

const free = await new Promise((r) => { const s = net.createServer().once('error', () => r(false)).listen(8443, '127.0.0.1', () => s.close(() => r(true))); });
if (!free) {
  const code = process.env.BRISK_SMOKE_ALLOW_SKIP === '1' ? 0 : 3;
  console.log(`SMOKE SKIPPED (exit ${code}): 127.0.0.1:8443 is in use (the live hostd service?). Stop it first, or run scripts/axe-check.mjs for the a11y part.`);
  process.exit(code);
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const FIX = `${root}/crates/brisk-hostd/tests/fixtures`;
const OUT = '/tmp/claude-1000/-home-toby-Projects/94a1bf04-5503-4de5-ba94-79e19e0c32dd/scratchpad/ui-smoke';
const hostd = process.env.BRISK_HOSTD ?? `${root}/target/release/brisk-hostd`;
const exe = process.env.BRISK_CHROMIUM ?? `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;
const KEY = 'SMOKE-SENTINEL-KEY-0123456789';
const TOTAL = 139;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

fs.mkdirSync(OUT, { recursive: true });
const work = fs.mkdtempSync(`${OUT}/work-`);
const w = (p, s, mode) => (fs.mkdirSync(path.dirname(`${work}/${p}`), { recursive: true }), fs.writeFileSync(`${work}/${p}`, s, { mode }));

// Installed: Portal 2 and the quoted Pokemon (from the fixtures) plus Half-Life 2 below; everything else is owned only.
const installed = { 620: 'Portal 2', 1091500: 'Pokemon Legends', 220: 'Half-Life 2' };
for (const [id, name] of Object.entries(installed))
  w(`steam/steamapps/appmanifest_${id}.acf`, `"AppState"\n{\n\t"appid"\t\t"${id}"\n\t"name"\t\t"${name}"\n\t"StateFlags"\t\t"4"\n\t"installdir"\t\t"x"\n\t"LastPlayed"\t\t"1700000000"\n\t"SizeOnDisk"\t\t"1000000000"\n}\n`);
w('steam/steamapps/libraryfolders.vdf', `"libraryfolders"\n{\n\t"0"\n\t{\n\t\t"path"\t\t"${work}/steam"\n\t}\n}\n`);
w('steam/config/loginusers.vdf', '"users"\n{\n\t"76561198000000000"\n\t{\n\t\t"MostRecent"\t"1"\n\t}\n}\n');
w('key', KEY);
const games = Object.entries(installed).map(([id, name]) => ({ appid: +id, name, playtime_forever: 10, rtime_last_played: 1700000000 }));
for (let i = 0; games.length < TOTAL; i++) games.push({ appid: 900000 + i, name: `Filler Game ${i}`, playtime_forever: 0 });
const mock = http.createServer((req, res) => {
  const ok = req.url.includes('/IPlayerService/GetOwnedGames/v1/') && req.url.includes(`key=${KEY}`);
  res.writeHead(ok ? 200 : 403).end(ok ? JSON.stringify({ response: { game_count: TOTAL, games } }) : '');
});
await new Promise((r) => mock.listen(0, '127.0.0.1', r));
w('bin/steam', `#!/bin/sh\necho "$@" >> '${work}/steam.log'\n`, 0o755);
w('bridge.sh', `#!/bin/sh\nFAKE_BRIDGE_LOG='${work}/bridge.log' exec '${FIX}/fake-bridge.sh' "$@"\n`, 0o755);

const results = [];
const check = (name, ok, extra = '') => (results.push(ok), console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${extra ? ` (${extra})` : ''}`));
let child, browser;
const a11y = async (page, name) => { const bad = await axe(page); check(`axe (WCAG A/AA, serious+critical): ${name}`, bad.length === 0, bad.join('; ')); };
const shot = async (page, n) => (await page.screenshot({ path: `${OUT}/${n}.png` }), console.log(`screenshot ${OUT}/${n}.png`));

try {
  child = spawn(hostd, [`${root}/web/dist`], {
    env: { ...process.env, PATH: `${work}/bin:${process.env.PATH}`, BRISK_STEAM_DIR: `${work}/steam`, BRISK_STEAM_KEY_FILE: `${work}/key`,
      BRISK_STEAM_API_BASE: `http://127.0.0.1:${mock.address().port}`, BRISK_BRIDGE_BIN: `${work}/bridge.sh`, BRISK_DATA_DIR: `${work}/data`, BRISK_LISTEN: '127.0.0.1:4433',
      BRISK_SECRET_FILE: `${work}/secret`, BRISK_TLS_DIR: `${work}/tls` },
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  let out = '';
  child.stdout.on('data', (d) => (out += d));
  for (let i = 0; i < 100 && !/page\t\S+/.test(out); i++) await sleep(100);
  const pageUrl = out.match(/page\t(\S+)/)?.[1];
  if (!pageUrl) throw new Error(`hostd printed no page line: ${out}`);
  const secret = new URL(pageUrl).searchParams.get('s');

  browser = await chromium.launch({ executablePath: exe });
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  // Record every canvas context type, to prove /stream creates no WebGL context.
  await ctx.addInitScript(() => {
    const g = HTMLCanvasElement.prototype.getContext;
    window.__ctx = [];
    HTMLCanvasElement.prototype.getContext = function (t, ...a) { window.__ctx.push(t); return g.call(this, t, ...a); };
  });
  const page = await ctx.newPage();
  let errs = [];
  page.on('console', (m) => m.type() === 'error' && errs.push(`console: ${m.text()}`));
  page.on('pageerror', (e) => errs.push(`pageerror: ${e}`));
  const take = () => errs.splice(0);

  await page.goto(pageUrl);
  for (const [name, p] of [['home', '/'], ['library', '/library'], ['machines', '/machines'], ['history', '/history'], ['settings', '/settings']]) {
    if (p !== '/') await page.goto(new URL(p, pageUrl).href);
    await page.waitForLoadState('networkidle');
    await sleep(500);
    check(`${name} loads without console/page errors`, take().length === 0, errs.join('; '));
    if (p === '/library') {
      await page.waitForSelector('button.g');
      const cards = await page.locator('button.g').count();
      check(`library renders ${cards} card nodes for ${TOTAL} games (< 60)`, cards > 0 && cards < 60);
    }
    await a11y(page, name);
    await shot(page, name);
  }
  for (const tab of ['streaming', 'controller', 'about']) {
    await page.goto(new URL(`/settings#${tab}`, pageUrl).href);
    await sleep(500);
    await a11y(page, `settings #${tab}`);
  }
  await page.goto(new URL('/machines', pageUrl).href);
  await page.getByRole('button', { name: 'Restart Sunshine' }).click();
  await page.getByRole('dialog').waitFor();
  await a11y(page, 'restart dialog open');
  await page.keyboard.press('Escape');

  // Command menu
  await page.goto(new URL('/library', pageUrl).href);
  await page.waitForSelector('button.g');
  await page.keyboard.press('Control+k');
  await page.getByPlaceholder('Do something…').fill('portal 2');
  check('Ctrl+K finds a game by name', await page.getByText('Launch Portal 2').first().isVisible());
  await a11y(page, 'command menu open');
  await shot(page, 'command-menu');
  await page.keyboard.press('Escape');

  // Accent persists across reload
  await page.goto(new URL('/settings', pageUrl).href);
  await page.getByRole('radio', { name: 'pink' }).click();
  await page.reload();
  const accent = await page.evaluate(() => document.documentElement.dataset.accent);
  check('accent change persists after reload', accent === 'pink', accent);

  // Secret guard
  const st = await page.evaluate(async () => (await fetch('/api/stream', { method: 'POST', body: '{}' })).status);
  check('POST /api/stream without the secret is 403', st === 403, String(st));
  const stGet = await page.evaluate(async () => (await fetch('/api/stream')).status);
  check('GET /api/stream without the secret is 403', stGet === 403, String(stGet));

  // Launch Portal 2 from the Library (fake bridge: no video, WebTransport will fail)
  await page.goto(new URL('/library', pageUrl).href);
  await page.getByRole('button', { name: /^Play Portal 2/ }).first().click();
  await page.waitForURL(/\/stream/);
  let live = null;
  for (let i = 0; i < 40 && live?.state !== 'live'; i++) {
    live = await page.evaluate(async (sec) => (await fetch('/api/stream', { headers: { 'x-brisk-secret': sec } })).json(), secret);
    await sleep(250);
  }
  check("stream state reaches 'live'", live?.state === 'live', live?.state);
  const bridgeLog = fs.readFileSync(`${work}/bridge.log`, 'utf8');
  check('default codec is H.264 (--codec h264)', live?.codec === 'h264' && bridgeLog.includes('--codec h264'), `${live?.codec}; ${bridgeLog.trim()}`);
  const bad = await page.evaluate(async (sec) => (await fetch('/api/stream', { method: 'POST', headers: { 'x-brisk-secret': sec, 'content-type': 'application/json' }, body: '{"preset":"custom","codec":"av1"}' })).status, secret);
  check('unknown codec is 400', bad === 400, String(bad));
  await sleep(1000);
  const steamLog = fs.existsSync(`${work}/steam.log`) ? fs.readFileSync(`${work}/steam.log`, 'utf8') : '';
  check('steam://rungameid/620 sent after the tap', steamLog.includes('steam://rungameid/620'), steamLog.trim());
  await page.mouse.move(600, 300);
  await page.mouse.move(600, 1);
  await page.waitForSelector('[role=toolbar].on', { timeout: 5000 }).then(() => check('ghost bar appears at the top edge', true), () => check('ghost bar appears at the top edge', false));
  await shot(page, 'stream-ghostbar');
  const degraded = await page.getByText('Lost the connection').waitFor({ timeout: 30000 }).then(() => true, () => false);
  await shot(page, 'stream-degraded');
  if (degraded) await a11y(page, 'stream degraded');
  check('stream degrades to the failed state (no real video to connect to)', degraded);
  check('stream page raised no page errors', take().filter((e) => e.startsWith('pageerror')).length === 0, errs.join('; '));

  // Stream page (fresh load): no smoke canvas, no WebGL context
  await page.goto(new URL('/stream', pageUrl).href);
  await sleep(1000);
  const s = await page.evaluate(() => ({ ctx: window.__ctx, smoke: document.querySelectorAll('canvas[aria-hidden="true"]').length }));
  check('/stream has no Smoke canvas and no WebGL context', s.smoke === 0 && !s.ctx.some((t) => /webgl/.test(t)), JSON.stringify(s));
} catch (e) {
  console.error(`error: ${e.stack ?? e}`);
  results.push(false);
} finally {
  await browser?.close().catch(() => {});
  if (child && child.exitCode === null) {
    child.kill('SIGTERM'); // hostd stops its bridge first
    await Promise.race([new Promise((r) => child.once('exit', r)), sleep(5000)]);
    if (child.exitCode === null) child.kill('SIGKILL');
  }
  mock.close();
  fs.rmSync(work, { recursive: true, force: true });
}
const ok = results.length > 0 && results.every(Boolean);
console.log(ok ? 'SMOKE PASS' : 'SMOKE FAIL');
process.exit(ok ? 0 : 1);
