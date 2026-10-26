<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { navigate } from 'astro:transitions/client';
  import { api, type StreamInfo, type StreamReq } from '../../state/api';
  import { host } from '../../state/host';
  import { settings } from '../../state/settings';
  import { games, watchGames } from '../../state/games';
  import { forgetPrefs, prefs, saver, savedBody, startGame, watchPrefs } from '../../state/prefs';
  import { onCommand } from '../../state/commands';
  import { toast } from '../../shell/Toasts.svelte';
  import { startVideo } from '../../lib/video';
  import { startAudio } from '../../lib/audio';
  import { startInput } from '../../lib/input';
  import workletUrl from '../../lib/audio-worklet.ts?worker&url';
  import { StreamSession, pollLive } from './session';
  import { holdEsc, type Hold } from './ghostbar';
  import { ClipSync, browserClipboard, uploadQueue, type Upload } from './clipboard';
  import { download, fileName, hotkey, muxInWorker, screenshot } from './capture';
  import GhostBar from './GhostBar.svelte';
  import StatsCard from './StatsCard.svelte';

  const hexBytes = (h: string) => Uint8Array.from(h.match(/../g) ?? [], (b) => parseInt(b, 16));
  const session = new StreamSession({
    api,
    open: (url, hash) => new WebTransport(url, { serverCertificateHashes: [{ algorithm: 'sha-256', value: hexBytes(hash) }] }),
    audioContext: () => new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' }),
    startVideo: (t, c, onDecode) => startVideo(t as WebTransport, c, onDecode),
    startAudio: (t, ctx, out, onPacket) => startAudio(t as WebTransport, ctx as AudioContext, workletUrl, out as unknown as AudioNode, onPacket),
    replaySeconds: () => get(settings).replaySeconds,
    startInput: (t, c, onpads) => startInput(t as WebTransport, c, onpads),
    canvas: () => {
      const c = document.createElement('canvas');
      c.id = 'video';
      return c;
    },
    notify: (text, error) => toast(text, { error }),
  });
  const { state: phase, stats, needsSound, canvasEl, pads } = session;

  let clipDenied = $state(false);
  const clip = new ClipSync({
    local: browserClipboard(),
    api: { get: api.getClipboard, post: api.postClipboard },
    focused: () => document.hasFocus() && document.visibilityState === 'visible',
    enabled: () => get(settings).clipboardSync && get(phase) === 'live',
    notify: (text, error) => toast(text, { error }),
    onstatus: (s) => (clipDenied = s === 'denied'),
  });
  let upload = $state<{ u: Upload; queued: number } | null>(null);
  let dropping = $state(false);
  const uploads = uploadQueue(api.uploadFile, {
    progress: (u, queued) => (upload = u && { u, queued }),
    done: (_, path) => toast(`Saved ${path}`, { ms: 6000 }),
    error: (name, msg) => toast(`Couldn't send ${name}: ${msg}`, { error: true }),
  });
  const pct = (u: Upload) => (u.total ? Math.floor((u.loaded / u.total) * 100) : 100);

  // 'boot' = asking hostd; 'idle' = nothing streaming; 'unsupported' = no WebTransport/WebCodecs.
  let page = $state<'boot' | 'idle' | 'unsupported' | 'session'>('boot');
  let statsOn = $state(false);
  let preset = $state<StreamInfo['preset']>(null);
  let codec = $state<StreamInfo['codec']>(null);
  let appid = $state<number | null>(null); // the live game, null for the Desktop
  let captured = $state(false);
  let starting = $state(false);
  let hold = $state<Hold>({ progress: 0, fired: false });
  let kbLocked = false;
  let destroyed = false;
  let stage: HTMLElement;

  async function boot() {
    page = 'boot';
    if (!('WebTransport' in window) || !('VideoDecoder' in window)) return void (page = 'unsupported');
    let info: StreamInfo | null;
    try {
      info = await pollLive(api, () => !destroyed);
    } catch (e) {
      toast(`Couldn't reach the host: ${(e as Error).message}`, { error: true });
      return void (page = 'idle');
    }
    if (!info) return; // left the page while waiting
    if (info.state !== 'live') return void (page = 'idle');
    appid = info.appid;
    preset = info.preset;
    codec = info.codec;
    page = 'session';
    await session.connect(info);
  }

  async function start() {
    starting = true;
    try {
      const info = await startGame(undefined); // the Desktop: no saved quality
      if (destroyed) return; // left the page during the POST: the session is closed anyway
      appid = info.appid;
      preset = info.preset;
      codec = info.codec;
      page = 'session';
      await session.connect(info);
    } catch (e) {
      toast(`Couldn't start the stream: ${(e as Error).message}`, { error: true });
    } finally {
      starting = false;
    }
  }

  async function quality(r: StreamReq) {
    toast(`Switching to ${r.preset === 'custom' ? 'custom quality' : r.preset}…`);
    const liveCodec = codec;
    await session.changeQuality(r);
    preset = session.info?.preset ?? r.preset;
    codec = session.info?.codec ?? codec;
    // Remembered per game, automatically; the Desktop (no appid) and a failed switch save nothing.
    if (appid !== null && get(phase) !== 'failed') saver.save(appid, savedBody(r, liveCodec));
  }

  const gameName = $derived($games?.games.find((g) => g.appid === appid)?.name ?? 'this game');
  const savedFor = $derived(appid !== null && $prefs[appid] ? gameName : null);

  const gameOrNull = () => (appid === null ? null : ($games?.games.find((g) => g.appid === appid)?.name ?? `app-${appid}`));

  async function shot() {
    const c = $canvasEl;
    if (!c || !c.width) return toast('Nothing on screen yet');
    const name = fileName(gameOrNull(), 'png');
    try {
      const { blob, copied } = await screenshot(c);
      download(blob, name);
      toast(`Saved ${name}${copied ? ' · copied' : ''}`);
    } catch (e) {
      toast(`Couldn't take the screenshot: ${(e as Error).message}`, { error: true });
    }
  }

  let saving = false;
  async function saveReplay() {
    if (!$settings.replaySeconds) return toast('Replay buffer is off: turn it on in Settings');
    if (saving) return toast('Already saving a clip…');
    const c = $canvasEl;
    const clip = session.clip();
    if (!clip || !c?.width) return toast('Nothing to save yet');
    saving = true;
    const name = fileName(gameOrNull(), 'mp4');
    try {
      const blob = await muxInWorker(clip, c.width, c.height);
      download(blob, name);
      toast(`Saved ${name} · ${Math.round(clip.seconds)} s · ${(blob.size / 1e6).toFixed(1)} MB`, { ms: 5000 });
    } catch (e) {
      toast(`Couldn't save the clip: ${(e as Error).message}`, { error: true });
    } finally {
      saving = false;
    }
  }

  async function forget() {
    if (appid === null) return;
    try {
      await forgetPrefs(appid);
      toast(`Forgot the saved quality for ${gameName}`);
    } catch (e) {
      toast(`Couldn't forget it: ${(e as Error).message}`, { error: true });
    }
  }

  async function disconnect() {
    session.disconnect();
    try {
      await api.stopStream();
    } catch {}
    toast('Disconnected');
    await navigate('/library');
  }

  function capture() {
    const c = $canvasEl;
    if (!c) return;
    if (captured) {
      document.exitPointerLock();
      c.blur();
    } else c.click(); // input.ts: the click focuses the canvas and asks for Pointer Lock
  }

  async function fullscreen() {
    if (document.fullscreenElement) return void document.exitFullscreen();
    await document.documentElement.requestFullscreen().catch(() => {});
    // Keyboard Lock: Esc goes to the page (hold Esc releases), not to Chrome.
    const kb = (navigator as Navigator & { keyboard?: { lock(keys?: string[]): Promise<void> } }).keyboard;
    let why = kb ? '' : 'this browser has no Keyboard Lock';
    kbLocked = !!kb && (await kb.lock(['Escape']).then(() => true, (e: Error) => ((why = e?.message || e?.name || 'refused'), false)));
    if (!document.fullscreenElement) why ||= 'fullscreen did not start (use this button, not F11)';
    toast(kbLocked ? 'Fullscreen: hold Esc to get your mouse back' : `Esc lock is off (${why}): a tap on Esc will free the mouse`, { error: !kbLocked });
  }

  const clipboardSync = $derived($settings.clipboardSync);
  $effect(() => {
    if ($phase !== 'live' || !clipboardSync) return;
    clip.start();
    return () => clip.stop();
  });

  // Mount the session's canvas (a fresh one per connection) into the stage.
  $effect(() => {
    const c = $canvasEl;
    if (c && stage && c.parentElement !== stage) stage.replaceChildren(c);
  });

  onMount(() => {
    const esc = holdEsc();
    let raf = 0;
    const tick = () => {
      hold = esc.tick(performance.now());
      if (hold.fired) {
        document.exitPointerLock(); // input.ts releases keys on the lock change
        $canvasEl?.blur();
        hold = esc.release(performance.now());
        return;
      }
      raf = requestAnimationFrame(tick); // only while Esc is held
    };
    const down = (e: KeyboardEvent) => {
      if (e.code !== 'Escape' || !kbLocked || !document.pointerLockElement) return;
      const first = !e.repeat && hold.progress === 0;
      hold = esc.press(performance.now());
      if (first) raf = requestAnimationFrame(tick);
    };
    const up = (e: KeyboardEvent) => {
      if (e.code !== 'Escape') return;
      cancelAnimationFrame(raf);
      hold = esc.release(performance.now());
    };
    // Reserved hotkeys, in the capture phase so input.ts (window, bubble phase) never sees them.
    const swallowed = new Set<string>();
    const hot = (e: KeyboardEvent) => {
      if (page !== 'session') return;
      const k = e.type === 'keydown' ? hotkey(e) : null;
      if (!k && !(e.type === 'keyup' && swallowed.delete(e.code))) return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (!k) return;
      swallowed.add(e.code);
      if (!e.repeat) void (k === 'shot' ? shot() : saveReplay());
    };
    window.addEventListener('keydown', hot, true);
    window.addEventListener('keyup', hot, true);
    let hinted = false;
    const cap = () => {
      captured = !!document.pointerLockElement || document.activeElement?.id === 'video';
      // Outside page fullscreen Chrome frees the mouse on a plain Esc tap and nothing can stop it.
      if (document.pointerLockElement && !document.fullscreenElement && !hinted) {
        hinted = true;
        toast('Esc frees the mouse here. Use Fullscreen in the top bar and you hold Esc instead.');
      }
    };
    const fs = () => {
      if (!document.fullscreenElement) kbLocked = false;
    };
    const clipFocus = () => void clip.focus();
    // Drag and drop of files from the laptop (needs a free pointer: not while it is locked).
    const hasFiles = (e: DragEvent) => !!e.dataTransfer?.types.includes('Files');
    // dragenter/dragleave fire per element crossed: count them, the overlay shows while > 0.
    let depth = 0;
    const enter = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      dropping = ++depth > 0;
    };
    const over = (e: DragEvent) => {
      if (hasFiles(e)) e.preventDefault(); // else Chrome opens the file
    };
    const leave = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      depth = Math.max(0, depth - 1);
      dropping = depth > 0;
    };
    const drop = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth = 0;
      dropping = false;
      uploads.add(Array.from(e.dataTransfer!.files));
    };
    const drag: [string, (e: DragEvent) => void][] = [['dragenter', enter], ['dragover', over], ['dragleave', leave], ['drop', drop]];
    for (const [k, f] of drag) window.addEventListener(k, f as EventListener);
    window.addEventListener('focus', clipFocus);
    document.addEventListener('visibilitychange', clipFocus);
    document.addEventListener('pointerlockchange', clipFocus);
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    document.addEventListener('pointerlockchange', cap);
    document.addEventListener('focusin', cap);
    const out = () => setTimeout(cap);
    document.addEventListener('focusout', out);
    document.addEventListener('fullscreenchange', fs);
    const off = onCommand((c) => {
      if (c.type === 'stats') statsOn = !statsOn;
      else if (page === 'session' && c.preset !== preset) void quality({ preset: c.preset });
    });
    void boot();
    const unwatchPrefs = watchPrefs();
    const unwatchGames = watchGames(); // the game's name for "Saved for …"
    return () => {
      unwatchPrefs();
      unwatchGames();
      // Leaving /stream (the island unmounts on navigation): release keys, close everything.
      destroyed = true;
      saver.flush(); // a change made in the last second still counts
      session.disconnect();
      clip.stop();
      uploads.cancel();
      for (const [k, f] of drag) window.removeEventListener(k, f as EventListener);
      window.removeEventListener('focus', clipFocus);
      document.removeEventListener('visibilitychange', clipFocus);
      document.removeEventListener('pointerlockchange', clipFocus);
      off();
      cancelAnimationFrame(raf);
      window.removeEventListener('keydown', hot, true);
      window.removeEventListener('keyup', hot, true);
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
      document.removeEventListener('pointerlockchange', cap);
      document.removeEventListener('focusin', cap);
      document.removeEventListener('focusout', out);
      document.removeEventListener('fullscreenchange', fs);
      if (document.pointerLockElement) document.exitPointerLock();
      if (document.fullscreenElement) void document.exitFullscreen().catch(() => {});
    };
  });
</script>

<div class="stream" data-state={page === 'session' ? $phase : page}>
  <div class="stage" bind:this={stage}></div>

  {#if page === 'session'}
    <GhostBar
      host={$host?.name ?? 'host'}
      rttMs={$stats.rttMs}
      {preset}
      {codec}
      {captured}
      {clipDenied}
      {upload}
      pads={$pads}
      onfiles={(f) => uploads.add(f)}
      bind:statsOn
      onvolume={(v) => session.setVolume(v)}
      oncapture={capture}
      onquality={quality}
      {savedFor}
      onforget={forget}
      onfullscreen={fullscreen}
      ondisconnect={disconnect}
      replaySeconds={$settings.replaySeconds}
      onshot={shot}
      onreplay={saveReplay}
    />
    {#if statsOn}<StatsCard stats={$stats} path={$host?.path === 'relay' ? 'Relay' : $host?.path === 'direct' ? 'Direct' : null} />{/if}
    {#if $phase === 'connecting'}
      <div class="center glass pill"><span class="spin"></span> Connecting to {$host?.name ?? 'the host'}…</div>
    {:else if $phase === 'failed'}
      <div class="center glass card">
        <h3>Lost the connection</h3>
        <p class="muted">The stream stopped answering. It may still be running on the host.</p>
        <div class="row"><button class="btn" onclick={boot}>Reconnect</button><a class="btn ghost" href="/library">Library</a></div>
      </div>
    {/if}
    {#if $needsSound}
      <button class="sound glass" onclick={() => session.resumeSound()}><span class="ic-s"></span> Tap to start sound</button>
    {/if}
    {#if hold.progress > 0}
      <div class="esc glass" role="status">
        <div class="ring" style="--p:{hold.progress * 100}"><b>{((1 - hold.progress) * 0.8).toFixed(1)}</b></div>
        <div><b>Hold Esc</b> to give your mouse back<div class="muted small">Release in {((1 - hold.progress) * 0.8).toFixed(1)} s</div></div>
      </div>
    {/if}
  {:else if page === 'idle'}
    <div class="center glass card">
      <h3>Nothing is streaming</h3>
      <p class="muted">Start the desktop of {$host?.name ?? 'your host'}, or pick a game in the Library.</p>
      <div class="row">
        <button class="btn" disabled={starting} onclick={start}>{starting ? 'Starting…' : 'Start'}</button>
        <a class="btn ghost" href="/library">Library</a>
      </div>
    </div>
  {:else if page === 'unsupported'}
    <div class="center glass card">
      <h3>This browser can't stream</h3>
      <p class="muted">Brisk needs WebTransport and WebCodecs: use Chrome on http://localhost.</p>
    </div>
  {/if}
  {#if dropping}
    <div class="drop" aria-hidden="true"><div class="glass card"><h3>Drop to send to {$host?.name ?? 'bawn'}</h3><p class="muted">Saved in ~/Downloads/brisk</p></div></div>
  {/if}
  {#if upload}
    <div class="up glass mono" role="status">Sending {upload.u.name} · {pct(upload.u)}%{upload.queued ? ` · ${upload.queued} more` : ''}</div>
  {/if}
</div>

<style>
  /* Covers the shell (sidebar included): the stream is the whole window. */
  .stream { position: fixed; inset: 0; z-index: 30; background: #000; }
  .stage { position: absolute; inset: 0; display: grid; }
  .stage :global(canvas) { width: 100%; height: 100%; object-fit: contain; outline: none; display: block; }
  .center { position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%); }
  .pill { display: flex; gap: 10px; align-items: center; padding: 10px 18px; border-radius: 999px; font-weight: 500; }
  .card { padding: 22px 24px; width: min(420px, calc(100vw - 32px)); display: grid; gap: 10px; border-radius: var(--r-card); }
  .card p { margin: 0; }
  .row { display: flex; gap: 10px; margin-top: 6px; }
  .spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid color-mix(in srgb, #fff 25%, transparent); border-top-color: var(--accent); animation: sp .8s linear infinite; }
  @keyframes sp { to { transform: rotate(360deg); } }
  .sound { position: absolute; left: 50%; bottom: 70px; transform: translateX(-50%); padding: 10px 18px; border-radius: 999px; font-weight: 600; cursor: pointer; display: flex; gap: 8px; align-items: center; }
  .ic-s { width: 8px; height: 8px; border-radius: 50%; background: var(--warn); }
  .esc { position: absolute; left: 50%; top: 40%; transform: translate(-50%, -50%); padding: 12px 18px; border-radius: 18px; display: flex; gap: 12px; align-items: center; z-index: 36; }
  .ring { width: 34px; height: 34px; border-radius: 50%; background: conic-gradient(var(--accent) calc(var(--p, 0) * 1%), color-mix(in srgb, #fff 14%, transparent) 0); display: grid; place-items: center; }
  .ring b { width: 26px; height: 26px; border-radius: 50%; background: #12151b; display: grid; place-items: center; font-size: 10.5px; font-family: var(--mono); }
  .small { font-size: 12px; }
  .drop { position: absolute; inset: 12px; z-index: 40; border: 2px dashed var(--accent); border-radius: 22px; display: grid; place-items: center; background: #0008; pointer-events: none; }
  .drop .card { text-align: center; }
  .up { position: absolute; right: 16px; bottom: 16px; padding: 8px 14px; border-radius: 999px; font-size: 12px; z-index: 36; max-width: 60vw; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
