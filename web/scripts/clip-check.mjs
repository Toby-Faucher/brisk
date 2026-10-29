// Headless Chrome checks for screenshot and replay clips (no stream needed):
//  1. screenshot(): a desynchronized 2D canvas, frames drawn from VideoFrames as video.ts does;
//     the PNG is the stream's native size and shows the latest frame.
//  2. a Replay ring at Crisp size (30 s at ~40 Mbps from the H.264 fixture padded with filler NAL
//     units, plus 30 s of Opus): snapshot() and clipInWorker() while a 60 fps canvas loop runs;
//     reports long tasks (> 50 ms), the longest main-thread gap and the peak RSS of all Chrome
//     processes over the baseline (worker + Blob); the MP4 plays in a <video>.
//  4. hotkeys: input.ts attach() and reserveHotkeys() on a real page with real (CDP) key events:
//     Ctrl+Alt+S / Ctrl+Alt+R reach our handler and never the input sender; plain S does.
//  3. the fixture clip muxed in the worker is written to $OUT/chrome-h264.mp4 (ffprobe it), and
//     MP4s given as arguments are loaded in a <video> (duration, size, error).
//
//   OUT=/some/dir node scripts/clip-check.mjs [file.mp4 ...]
import { chromium } from 'playwright-core';
import { exe } from './browser.mjs';
import { createServer } from 'vite';
import { mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const web = resolve(import.meta.dirname, '..');
const out = process.env.OUT ?? mkdtempSync(join(tmpdir(), 'brisk-clip-check-'));
const root = mkdtempSync(join(tmpdir(), 'brisk-clip-root-'));
const fs = (p) => `/@fs${join(web, p)}`;
writeFileSync(join(root, 'index.html'), `<!doctype html><body><script type="module">
import { Replay } from '${fs('src/modules/stream/replay.ts')}';
import { clipInWorker, reserveHotkeys, screenshot } from '${fs('src/modules/stream/capture.ts')}';
import { attach } from '${fs('src/lib/input.ts')}';
Object.assign(window, { Replay, clipInWorker, reserveHotkeys, screenshot, attach, ready: true });
</script></body>`);
const server = await createServer({ root, configFile: false, logLevel: 'error', server: { port: 0, fs: { allow: [web, root] } } });
await server.listen();
const url = server.resolvedUrls.local[0];
const browser = await chromium.launch({ executablePath: exe, args: ['--autoplay-policy=no-user-gesture-required'] });
let fail = 0;
const check = (ok, what) => (console.log(`${ok ? 'PASS' : 'FAIL'} ${what}`), ok || fail++);
try {
  const page = await browser.newPage();
  page.on('pageerror', (e) => console.log('pageerror:', e.message));
  await page.goto(url);
  await page.waitForFunction(() => window.ready);

  // 1. screenshot of the latest frame
  const shot = await page.evaluate(async () => {
    const c = document.createElement('canvas');
    c.width = 1920;
    c.height = 1080;
    c.style.width = '320px';
    document.body.append(c);
    const ctx = c.getContext('2d', { alpha: false, desynchronized: true });
    const src = new OffscreenCanvas(1920, 1080);
    const sctx = src.getContext('2d');
    const colors = ['#ff0000', '#00ff00', '#0000ff', '#ffff00'];
    const results = [];
    for (let round = 0; round < 3; round++) {
      for (const [i, col] of colors.entries()) {
        sctx.fillStyle = col;
        sctx.fillRect(0, 0, 1920, 1080);
        const f = new VideoFrame(src, { timestamp: (round * 4 + i) * 16667 });
        ctx.drawImage(f, 0, 0);
        f.close();
        if (i < colors.length - 1) await new Promise((r) => setTimeout(r, 5));
      }
      const { png, copied: cp } = screenshot(c); // right after the last draw, no frame wait
      const blob = await png;
      const copied = await cp;
      const bmp = await createImageBitmap(blob);
      const o = new OffscreenCanvas(1, 1).getContext('2d');
      o.drawImage(bmp, -960, -540);
      results.push({ w: bmp.width, h: bmp.height, px: [...o.getImageData(0, 0, 1, 1).data], type: blob.type, copied });
    }
    return results;
  });
  for (const r of shot) check(r.w === 1920 && r.h === 1080 && r.px.join() === '255,255,0,255' && r.type === 'image/png', `screenshot native 1920x1080 PNG shows the latest frame (got ${r.w}x${r.h} ${r.px}, clipboard copied=${r.copied})`);

  // 2 + 3. replay ring at Crisp size, clip while a 60 fps draw loop runs
  const h264 = [...readFileSync(join(web, 'src/modules/stream/fixtures/h264.h264'))];
  const ogg = [...readFileSync(join(web, 'src/modules/stream/fixtures/opus.ogg'))];
  const mem = { base: 0, peak: 0 };
  const rss = () => {
    let kb = 0;
    for (const pid of readdirSync('/proc').filter((d) => /^\d+$/.test(d))) {
      try {
        if (!readFileSync(`/proc/${pid}/cmdline`, 'latin1').includes('chrome-headless-shell')) continue;
        kb += Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${pid}/status`, 'latin1'))?.[1] ?? 0);
      } catch {}
    }
    return kb;
  };
  let memTimer;
  await page.exposeFunction('memMark', () => {
    mem.base = mem.peak = rss();
    memTimer = setInterval(() => (mem.peak = Math.max(mem.peak, rss())), 20);
  });
  const res = await page.evaluate(async ({ h264, ogg }) => {
    const es = Uint8Array.from(h264);
    const aud = [];
    for (let i = 0; i + 4 < es.length; i++) if (es[i] === 0 && es[i + 1] === 0 && es[i + 2] === 0 && es[i + 3] === 1 && (es[i + 4] & 0x1f) === 9) aud.push(i);
    const aus = aud.slice(0, 180).map((s, k) => es.slice(s, aud[k + 1]));
    const b = Uint8Array.from(ogg);
    const opus = [];
    let cur = [];
    for (let o = 0; o < b.length; ) {
      const n = b[o + 26];
      let p = o + 27 + n;
      for (let s = 0; s < n; s++) {
        const len = b[o + 27 + s];
        cur.push(...b.subarray(p, p + len));
        p += len;
        if (len < 255) opus.push(Uint8Array.from(cur)), (cur = []);
      }
      o = p;
    }
    opus.splice(0, 2);
    const isKey = (au) => { for (let i = 0; i + 4 < au.length; i++) if (au[i] === 0 && au[i + 1] === 0 && au[i + 2] === 1 && (au[i + 3] & 0x1f) === 5) return true; return false; };
    const keys = aus.map(isKey);

    const fill = (au, size) => {
      const pad = Math.max(0, size - au.length - 6);
      const x = new Uint8Array(au.length + pad + 6);
      x.set(au);
      x.set([0, 0, 0, 1, 0x0c], au.length); // filler data NAL
      x.fill(0xff, au.length + 5, au.length + 5 + pad);
      x[x.length - 1] = 0x80;
      return x;
    };
    const SECS = 30;
    const FRAMES = SECS * 60;
    const PER = Math.round((40e6 / 8 / 60) * 0.98); // ~40 Mbps
    const make = (frames, size) => {
      const t0 = performance.now() - SECS * 1000 - 50;
      const r = new window.Replay(SECS, 160 << 20, () => t0 + SECS * 1000 + 50);
      for (let i = 0; i < frames; i++) {
        const at = t0 + i * (1000 / 60);
        for (let k = Math.floor(((i - 1) * 1000) / 60 / 5) + 1; k <= Math.floor((i * 1000) / 60 / 5); k++) {
          if (k >= 0) r.audio({ seq: k & 0xffff, ts48: 96000 + k * 240, opus: opus[k % opus.length] }, at);
        }
        const au = aus[i % 180];
        r.video(size ? fill(au, keys[i % 180] ? size * 4 : size) : au, keys[i % 180], BigInt(Math.round((at - 3) * 1e6)), at);
      }
      return r;
    };

    // a draw loop like the stream: every rAF, plus a probe of main-thread gaps
    const c = document.querySelector('canvas');
    const ctx = c.getContext('2d', { alpha: false, desynchronized: true });
    let drawing = true;
    let maxGap = 0;
    let last = performance.now();
    const loop = () => {
      const now = performance.now();
      maxGap = Math.max(maxGap, now - last);
      last = now;
      ctx.fillStyle = `hsl(${now % 360} 80% 50%)`;
      ctx.fillRect(0, 0, 64, 64);
      if (drawing) requestAnimationFrame(loop);
    };
    const longs = [];
    new PerformanceObserver((l) => longs.push(...l.getEntries().map((e) => Math.round(e.duration)))).observe({ type: 'longtask' });

    const big = make(FRAMES, PER);
    const ringMB = big.bytes / 1e6;
    await new Promise((r) => setTimeout(r, 300));
    longs.length = 0;
    requestAnimationFrame(loop);
    await new Promise((r) => setTimeout(r, 200));
    maxGap = 0;
    const s0 = performance.now();
    const snap = big.snapshot();
    const selectMs = performance.now() - s0;
    const m0 = performance.now();
    window.memMark?.('start');
    const { blob, seconds } = await window.clipInWorker(snap, 320, 180);
    const muxMs = performance.now() - m0;
    await new Promise((r) => setTimeout(r, 100));
    drawing = false;
    const v = document.createElement('video');
    v.muted = true;
    v.src = URL.createObjectURL(blob);
    const meta = await new Promise((res) => {
      v.onloadedmetadata = () => res({ duration: v.duration, w: v.videoWidth, h: v.videoHeight });
      v.onerror = () => res({ error: v.error?.message || String(v.error?.code) });
    });

    // the plain fixture, muxed in the worker, returned for ffprobe
    const small = make(180, 0).snapshot();
    const sb = new Uint8Array(await (await window.clipInWorker(small, 320, 180)).blob.arrayBuffer());
    return { ringMB, frames: snap.v.length, audio: snap.a.length, clipMB: blob.size / 1e6, seconds, selectMs, muxMs, longs, maxGap, meta, small: [...sb], heapMB: performance.memory?.usedJSHeapSize / 1e6 };
  }, { h264, ogg });
  clearInterval(memTimer);
  console.log(`ring ${res.ringMB.toFixed(1)} MB (${res.frames} frames + ${res.audio} Opus packets); clip ${res.clipMB.toFixed(1)} MB, ${res.seconds.toFixed(2)} s; snapshot() ${res.selectMs.toFixed(1)} ms; worker select+mux ${res.muxMs.toFixed(0)} ms; long tasks ${JSON.stringify(res.longs)}; longest rAF gap ${res.maxGap.toFixed(1)} ms`);
  console.log(`memory: Chrome RSS baseline ${(mem.base / 1024).toFixed(0)} MB, peak ${(mem.peak / 1024).toFixed(0)} MB (+${((mem.peak - mem.base) / 1024).toFixed(0)} MB while clipping); page JS heap after ${res.heapMB?.toFixed(0)} MB`);
  check(res.ringMB <= 160 * 1.048576 + 1 && res.ringMB > 140, `ring at Crisp size stays under 160 MiB (${res.ringMB.toFixed(1)} MB)`);
  check(res.longs.every((d) => d <= 50), 'no main-thread long task over 50 ms while clipping');
  check(res.selectMs < 5, `snapshot() on the main thread under 5 ms (${res.selectMs.toFixed(2)} ms)`);
  check(res.meta.duration >= 30 && !res.meta.error, `Crisp-size clip plays in <video>: ${JSON.stringify(res.meta)}`);
  writeFileSync(join(out, 'chrome-h264.mp4'), Uint8Array.from(res.small));
  console.log(`wrote ${join(out, 'chrome-h264.mp4')}`);

  // 4. hotkeys vs input.ts
  await page.evaluate(() => {
    const c = document.createElement('canvas');
    c.id = 'kb';
    c.tabIndex = 0;
    document.body.append(c);
    window.sent = [];
    window.ours = [];
    const s = new Proxy({}, { get: (_, k) => (...a) => window.sent.push([k, ...a]) });
    window.attach(s, { canvas: c, doc: document, win: window }, new Promise(() => {}));
    window.reserveHotkeys(window, () => true, (k) => window.ours.push(k));
    c.focus();
  });
  for (const k of ['KeyS', 'KeyR']) {
    await page.keyboard.down('Control');
    await page.keyboard.down('Alt');
    await page.keyboard.press(k);
    await page.keyboard.up('Alt');
    await page.keyboard.up('Control');
  }
  await page.keyboard.press('KeyS');
  const kb = await page.evaluate(() => ({ sent: window.sent.filter((x) => x[0] === 'key').map((x) => [x[1], x[2]]), ours: window.ours }));
  const vkS = 0x53, vkR = 0x52;
  const sCount = kb.sent.filter(([vk]) => vk === vkS).length;
  check(JSON.stringify(kb.ours) === '["shot","clip"]', `Ctrl+Alt+S / Ctrl+Alt+R reach the handler (${JSON.stringify(kb.ours)})`);
  check(sCount === 2 && !kb.sent.some(([vk]) => vk === vkR), `the game never sees them: only the plain S down/up went to input.ts (keys sent ${JSON.stringify(kb.sent)})`);

  for (const f of process.argv.slice(2)) {
    const bytes = [...readFileSync(f)];
    const r = await page.evaluate(async (bytes) => {
      const v = document.createElement('video');
      v.muted = true;
      v.src = URL.createObjectURL(new Blob([Uint8Array.from(bytes)], { type: 'video/mp4' }));
      document.body.append(v);
      const meta = await new Promise((res) => {
        v.onloadedmetadata = () => res({ duration: v.duration, w: v.videoWidth, h: v.videoHeight });
        v.onerror = () => res({ error: v.error?.message || String(v.error?.code) });
        setTimeout(() => res({ error: 'timeout' }), 5000);
      });
      if (meta.error) return meta;
      await v.play().catch((e) => (meta.playError = String(e)));
      await new Promise((r) => setTimeout(r, 1500));
      return { ...meta, playedTo: v.currentTime, error: v.error?.message ?? null, frames: v.getVideoPlaybackQuality?.().totalVideoFrames };
    }, bytes);
    check(r.duration > 0 && !r.error && !r.playError, `${f} in <video>: ${JSON.stringify(r)}`);
  }
} finally {
  await browser.close();
  await server.close();
}
process.exit(fail ? 1 : 0);
