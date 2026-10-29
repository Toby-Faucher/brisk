// Headless Chromium against a running brisk-bridge: proves WebTransport connect, per-frame
// stream framing, the SPS -> codec string, isConfigSupported, software H.264 decode, and that a
// stalled reader (--stall) does not break decoding; and for audio: the connect click starts an
// AudioContext, Opus datagrams decode in WebCodecs and feed the worklet ring (counters: lost,
// underruns, bufferMs). It cannot prove laptop Chrome: hardware decode, its present timing, the
// real network path, or that the audio is audible (headless has no real output device).
//
//   node scripts/headless.mjs '<page URL printed by brisk-bridge>' [--secs N] [--stall MS] [--input SECS]
//
// --input SECS: click the video once (asks for Pointer Lock; the click itself is not sent), then
// every SECS seconds type a harmless round: Shift down, a, b, Shift up (CDP key events), and
// relative moves of +-5 px that end where they started (page events, see below). Nothing else (no clicks, Enter, Escape, Space,
// Alt, Super or Ctrl): the host shows Steam Big Picture.
//
// Exit 0 when frames arrived at >= 90% of 60 fps (stall time excepted), the codec string matches
// the SPS, and bridge_write - capture looks sane. It decodes in its software H.264 decoder
// (isConfigSupported: no-preference true, prefer-hardware false on bawn).
import { chromium } from 'playwright-core';
import { exe } from './browser.mjs';

const args = process.argv.slice(2);
const url = args[0];
const opt = (k, d) => (args.includes(k) ? Number(args[args.indexOf(k) + 1]) : d);
const secs = opt('--secs', 20);
const stall = opt('--stall', 0);
const inputEvery = opt('--input', 0);
if (!url) {
  console.error('usage: node scripts/headless.mjs URL [--secs N] [--stall MS]');
  process.exit(2);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = await chromium.launch({ executablePath: exe });
let ok = false;
try {
  const page = await browser.newPage();
  page.on('console', (m) => console.log('page:', m.text()));
  await page.goto(url);
  await page.click('#connect'); // a user gesture: the page makes its AudioContext in it
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
  let rounds = 0;
  let locked = null;
  const half = async (ms) => {
    if (!inputEvery) return sleep(ms);
    const end = Date.now() + ms;
    while (Date.now() < end) {
      await safeRound();
      await sleep(Math.min(inputEvery * 1000, Math.max(0, end - Date.now())));
    }
  };
  // A round leaves nothing pressed and the host pointer where it was.
  const safeRound = async () => {
    const box = await page.locator('#video').boundingBox();
    const [x, y] = [box.x + box.width / 2, box.y + box.height / 2];
    if (locked === null) {
      await page.waitForFunction(() => window.briskInput, null, { timeout: 5000 });
      await page.mouse.move(x, y);
      await page.mouse.click(x, y); // Pointer Lock request; not forwarded (not locked yet)
      await sleep(300);
      locked = await page.evaluate(() => document.pointerLockElement?.id === 'video');
      console.log(`pointer lock ${locked ? 'granted' : 'refused: keyboard-only capture (canvas focus)'}`);
    }
    const gap = () => sleep(60);
    await page.keyboard.down('Shift'); await gap();
    await page.keyboard.press('a'); await gap();
    await page.keyboard.press('b'); await gap();
    await page.keyboard.up('Shift'); await gap();
    // Motion: headless Chromium under Pointer Lock turns each CDP move into a jump from and back
    // to its parked cursor (+640,+360 then -640,-360), so the movementX path is driven with page
    // events carrying movementX/Y instead (+5, -5 per axis; they go through the same handler).
    for (const [dx, dy] of [[5, 0], [-5, 0], [0, 5], [0, -5]]) {
      await page.evaluate(([dx, dy]) => document.getElementById('video').dispatchEvent(
        new MouseEvent('mousemove', { movementX: dx, movementY: dy, bubbles: true })), [dx, dy]);
      await sleep(20);
    }
    rounds++;
  };
  await half((secs * 1000) / 2);
  if (stall) {
    console.log(`stalling the stream reader for ${stall} ms`);
    await page.evaluate((ms) => window.brisk.stall(ms), stall);
  }
  await half((secs * 1000) / 2);
  const elapsed = (Date.now() - t0) / 1000;
  const s = await page.evaluate(() => {
    const s = window.brisk.stats;
    const w = (x) => ({ p50: x.pct(50), p99: x.pct(99), min: x.min() });
    const plain = {};
    for (const [k, v] of Object.entries(s)) plain[k] = v && typeof v.pct === 'function' ? w(v) : v;
    plain.ended = window.briskEnded ?? null;
    return plain;
  });
  const a = await page.evaluate(() => (window.briskAudio ? { ...window.briskAudio.stats } : null));
  const inp = await page.evaluate(() => (window.briskInput ? { ...window.briskInput.stats } : null));
  console.log(JSON.stringify({ isConfigSupported: support, elapsed, stats: s, audio: a, input: inp, inputRounds: rounds, pointerLock: locked }, null, 1));
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
  // Sunshine sent audio continuously in every bawn run so far (200 packets/s, silence included).
  const wantAudio = 0.9 * 200 * elapsed;
  checks[`audio decoded >= ${wantAudio.toFixed(0)}`] = !!a && a.decoded >= wantAudio;
  checks['audio decode errors 0'] = !!a && a.decodeErrors === 0;
  checks['AudioContext running'] = !!a && a.contextState === 'running';
  if (inputEvery) {
    // Per round: Shift, a, b down and up = 6 key messages; motion only under Pointer Lock.
    checks[`input keys = 6 x ${rounds} rounds`] = !!inp && inp.keys === 6 * rounds;
    checks['input buttons 0 (no clicks sent)'] = !!inp && inp.buttons === 0;
    if (locked) checks[`input moves = 4 x ${rounds} rounds`] = !!inp && inp.moves === 4 * rounds;
  }
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
