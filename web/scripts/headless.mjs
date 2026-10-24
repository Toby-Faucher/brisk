// Headless Chromium against a running brisk-bridge: proves WebTransport connect, per-frame
// stream framing, the SPS -> codec string, isConfigSupported, software H.264 decode, and that a
// stalled reader (--stall) does not break decoding. It cannot prove laptop Chrome: hardware
// decode, its present timing, the real network path.
//
//   node scripts/headless.mjs '<page URL printed by brisk-bridge>' [--secs N] [--stall MS]
//
// Exit 0 when frames arrived at >= 90% of 60 fps (stall time excepted), the codec string matches
// the SPS, and bridge_write - capture looks sane. It decodes in its software H.264 decoder
// (isConfigSupported: no-preference true, prefer-hardware false on bawn).
import { chromium } from 'playwright-core';

const args = process.argv.slice(2);
const url = args[0];
const opt = (k, d) => (args.includes(k) ? Number(args[args.indexOf(k) + 1]) : d);
const secs = opt('--secs', 20);
const stall = opt('--stall', 0);
if (!url) {
  console.error('usage: node scripts/headless.mjs URL [--secs N] [--stall MS]');
  process.exit(2);
}
const exe =
  process.env.BRISK_CHROMIUM ??
  `${process.env.HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await chromium.launch({ executablePath: exe });
let ok = false;
try {
  const page = await browser.newPage();
  page.on('console', (m) => console.log('page:', m.text()));
  await page.goto(url);
  await page.waitForFunction(() => window.brisk || window.briskEnded, null, { timeout: 15000 });
  const ended = await page.evaluate(() => window.briskEnded);
  if (ended) throw new Error(`page: ${ended}`);
  const support = await page.evaluate(async () => {
    const r = {};
    for (const hardwareAcceleration of ['prefer-hardware', 'no-preference', 'prefer-software']) {
      r[hardwareAcceleration] = (
        await VideoDecoder.isConfigSupported({ codec: 'avc1.640c2a', optimizeForLatency: true, hardwareAcceleration })
      ).supported;
    }
    return r;
  });
  const t0 = Date.now();
  await sleep((secs * 1000) / 2);
  if (stall) {
    console.log(`stalling the stream reader for ${stall} ms`);
    await page.evaluate((ms) => window.brisk.stall(ms), stall);
  }
  await sleep((secs * 1000) / 2);
  const elapsed = (Date.now() - t0) / 1000;
  const s = await page.evaluate(() => {
    const s = window.brisk.stats;
    const w = (x) => ({ p50: x.pct(50), p99: x.pct(99), min: x.min() });
    const plain = {};
    for (const [k, v] of Object.entries(s)) plain[k] = v && typeof v.pct === 'function' ? w(v) : v;
    plain.ended = window.briskEnded ?? null;
    return plain;
  });
  console.log(JSON.stringify({ isConfigSupported: support, elapsed, stats: s }, null, 1));
  // Frames are counted from the page's start, so the window is a bit longer than `elapsed`.
  const want = 0.9 * 60 * (elapsed - stall / 1000);
  const checks = {
    [`frames received >= ${want.toFixed(0)}`]: s.framesReceived >= want,
    'codec avc1.640c2a': s.codec === 'avc1.640c2a',
    'bridge_write - capture p50 in (0, 50) ms': s.bridgeCaptureMs.p50 > 0 && s.bridgeCaptureMs.p50 < 50,
    'session still open': s.ended === null,
    // Also across a stall: Chrome acknowledges and buffers the streams the page is not reading,
    // so the bridge sees no back-pressure for a few seconds (docs/m2-notes.md).
    'decode errors 0': s.decodeErrors === 0,
  };
  // After a stall the burst backs the decoder up (queue > 2), so it waits for an IDR by design.
  if (s.supported && !stall) checks['decoded >= 95% of received'] = s.framesDecoded >= 0.95 * s.framesReceived;
  for (const [k, v] of Object.entries(checks)) console.log(`${v ? 'ok  ' : 'FAIL'} ${k}`);
  ok = Object.values(checks).every(Boolean);
} catch (e) {
  console.error(`error: ${e}`);
} finally {
  await browser.close();
}
process.exit(ok ? 0 : 1);
