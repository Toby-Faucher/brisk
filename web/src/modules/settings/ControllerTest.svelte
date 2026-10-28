<script lang="ts">
  import { onMount } from 'svelte';
  import { padName, type PadLike } from '../../lib/gamepad';
  import Card from '../../ui/Card.svelte';
  import Dropdown from '../../ui/Dropdown.svelte';
  import { countUp, popIn } from '../../ui/motion';
  import PadDiagram from './PadDiagram.svelte';
  import {
    DEFAULT_DEADZONE, DriftCheck, PollRate, RangeTest, Trail, mappingRows, padChoices, padView, type DriftResult, type PadView, type StickDrift,
  } from './controllertest';

  const ROWS = mappingRows();
  const DRIFT_MS = 3000;
  const RUMBLE_MS = 400;
  const noPad = { index: -1, id: '', connected: false, mapping: '', buttons: [], axes: [] } as PadLike;

  let root: HTMLElement;
  let dz = $state(DEFAULT_DEADZONE * 100);
  let choices = $state<ReturnType<typeof padChoices>>([]);
  let sel = $state(-1);
  let view = $state<PadView>(padView(noPad, DEFAULT_DEADZONE));
  let hz = $state(0);
  let polling = $state(false);
  let status = $state(''); // polite line: connect / disconnect / drift result only
  let trailL = $state('');
  let trailR = $state('');
  let rangeOn = $state(false);
  let rangeL = $state('');
  let rangeR = $state('');
  let rumbleOk = $state(false);
  let drift = $state<{ phase: 'idle' | 'running' | 'moved' | 'done'; pct: number; res?: Extract<DriftResult, { state: 'done' }> }>({ phase: 'idle', pct: 0 });

  const rate = new PollRate();
  const tl = new Trail(), tr = new Trail();
  const rl = new RangeTest(32), rr = new RangeTest(32);
  let check: DriftCheck | null = null;
  let seen = '';
  let raf = 0;
  let inView = true;
  let io: IntersectionObserver | undefined;

  const pts = (p: [number, number][]) => p.map(([x, y]) => `${x.toFixed(3)},${y.toFixed(3)}`).join(' ');
  const trailPts = (t: Trail) => pts(t.points.map((p) => [p.x, p.y]));
  const pads = () => (navigator.getGamepads?.() ?? []) as (PadLike | null)[];
  const cur = () => pads().find((g) => g?.connected && g.index === sel) ?? null;
  const pct = (v: number, d = 0) => (v * 100).toFixed(d);
  const sgn = (v: number) => (v >= 0 ? '+' : '') + v.toFixed(2);
  const fmt1 = (v: number) => `${v.toFixed(1)}%`;
  const fmtPct = (v: number) => `${Math.round(v)}%`;

  function frame(now: number) {
    raf = requestAnimationFrame(frame);
    rate.tick(now);
    hz = Math.round(rate.hz);
    const all = pads();
    const list = padChoices(all);
    const key = JSON.stringify(list);
    if (key !== seen) {
      seen = key;
      choices = list;
    }
    if (!list.some((c) => c.index === sel)) sel = list[0]?.index ?? -1;
    const g = all.find((p) => p?.connected && p.index === sel);
    if (!g) {
      view = padView(noPad, dz / 100);
      rumbleOk = false;
      return;
    }
    rumbleOk = !!g.vibrationActuator;
    const v = (view = padView(g, dz / 100));
    tl.add(v.left.x, v.left.y, now);
    tr.add(v.right.x, v.right.y, now);
    trailL = trailPts(tl);
    trailR = trailPts(tr);
    if (rangeOn) {
      rl.record(v.left.x, v.left.y);
      rr.record(v.right.x, v.right.y);
      rangeL = pts(rl.polygon());
      rangeR = pts(rr.polygon());
    }
    if (check) {
      const r = check.push(v, now);
      driftT0 ||= now;
      if (r.state === 'running') drift = { phase: 'running', pct: Math.min(99, Math.round(((now - driftT0) / DRIFT_MS) * 100)) };
      else {
        check = null;
        if (r.state === 'moved') (drift = { phase: 'moved', pct: 0 }), (status = 'You moved it, try again.');
        else {
          drift = { phase: 'done', pct: 100, res: r };
          status = `Drift check done: left ${r.left.verdict}, right ${r.right.verdict}.`;
        }
      }
    }
  }
  let driftT0 = 0;

  const active = () => inView && document.hasFocus() && document.visibilityState === 'visible';
  function sync() {
    const want = active();
    polling = want;
    if (want && !raf) raf = requestAnimationFrame(frame);
    if (!want && raf) {
      cancelAnimationFrame(raf);
      raf = 0;
      rate.hz = 0;
      hz = 0;
      check = null; // a check cannot span a pause
      if (drift.phase === 'running') drift = { phase: 'idle', pct: 0 };
      stopRumble();
    }
  }

  function startDrift() {
    check = new DriftCheck(DRIFT_MS);
    driftT0 = 0;
    drift = { phase: 'running', pct: 0 };
    status = '';
  }
  function resetRange() {
    rl.reset();
    rr.reset();
    rangeL = rangeR = '';
  }

  async function rumble(strong: number, weak: number) {
    try {
      await cur()?.vibrationActuator?.playEffect('dual-rumble', { duration: RUMBLE_MS, startDelay: 0, strongMagnitude: strong, weakMagnitude: weak });
    } catch {
      // the pad refused (unplugged or busy): nothing to show
    }
  }
  function stopRumble() {
    for (const g of pads()) g?.vibrationActuator?.reset().catch(() => {});
  }

  onMount(() => {
    const ac = new AbortController();
    // biome-ignore lint/suspicious/noExplicitAny: one helper for listeners of several event types
    const on = (t: EventTarget, type: string, f: (e: any) => void) => t.addEventListener(type, f, { signal: ac.signal });
    on(window, 'focus', sync);
    on(window, 'blur', sync);
    on(document, 'visibilitychange', sync);
    on(window, 'gamepadconnected', (e: { gamepad: PadLike }) => (status = `Controller connected: ${padName(e.gamepad.id)}.`));
    on(window, 'gamepaddisconnected', (e: { gamepad: PadLike }) => (status = `Controller disconnected: ${padName(e.gamepad.id)}.`));
    io = new IntersectionObserver((es) => ((inView = es[es.length - 1].isIntersecting), sync()));
    io.observe(root);
    sync();
    return () => {
      ac.abort();
      io?.disconnect();
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
      stopRumble();
    };
  });

  const driftSticks = $derived(drift.res ? ([['Left stick', drift.res.left], ['Right stick', drift.res.right]] as [string, StickDrift][]) : []);
</script>

<section bind:this={root} aria-label="Controller test" class="ct">
  <Card cols={2} class="ctcard">
    <h4>Controller</h4>
    <p class="note first">Live view of your pad, to spot stick drift and see what each button sends. Runs in this page only: nothing is sent to the host, and it stops when you leave. To change what a game's buttons do, open an emulator game in the Library.</p>

    <div class="sr" role="status">{status}</div>
    {#if status}<p class="msg" aria-hidden="true">{status}</p>{/if}

    <!-- 1. status -->
    <div class="block">
      {#if choices.length === 0}
        <p class="hint">Press a button on your controller (Chrome only shows it after a press)</p>
      {:else}
        <div class="opt"><span class="lab">Pad</span>
          <Dropdown value={String(sel)} options={choices.map((c): [string, string] => [String(c.index), `${c.index}: ${padName(c.id)}`])} label="Controller" onchange={(v) => (sel = Number(v))} />
        </div>
        {#each choices.filter((c) => c.index === sel) as c (c.index)}
          <p class="mono small">{c.id}<br />{c.detail}</p>
        {/each}
      {/if}
      <p class="small"><span class="lab">Poll rate</span> <span class="mono">{polling ? `${hz} Hz` : 'paused'}</span>
        {#if !polling}<span class="lab"> (the page needs focus)</span>{/if}</p>
    </div>

    <!-- 2. diagram + sticks -->
    <div class="live">
      <PadDiagram {view} dz={dz / 100} trails={[trailL, trailR]} />

      <div class="sticks">
        {#each [['Left stick', view.left, tl, trailL, rangeL], ['Right stick', view.right, tr, trailR, rangeR]] as [name, s, , trail, gate] (name)}
          {@const st = s as typeof view.left}
          <div class="stick">
            <svg viewBox="-1.15 -1.15 2.3 2.3" aria-hidden="true" focusable="false">
              <circle class="ring" r="1" />
              <line class="cross" x1="-1" x2="1" y1="0" y2="0" /><line class="cross" y1="-1" y2="1" x1="0" x2="0" />
              <polygon class="gate" points={gate as string} />
              <circle class="dzr" r={dz / 100} />
              <polyline class="trail" points={trail as string} />
              <circle class="dot" class:out={!st.inDeadzone} cx={st.x} cy={st.y} r="0.07" />
            </svg>
            <div class="mono nums">
              <b>{name}</b>
              <span>x {sgn(st.x)} ({st.pctX}%)</span>
              <span>y {sgn(st.y)} ({st.pctY}%)</span>
              <span>{st.pctMag}% {st.inDeadzone ? 'inside' : 'outside'} deadzone</span>
            </div>
          </div>
        {/each}
      </div>
    </div>
    <p class="small mono">LT {pct(view.lt)}% · RT {pct(view.rt)}% <span class="lab">(analog, 0 to 100%)</span></p>
    <label class="opt"><span class="lab">Deadzone ring (display only)</span>
      <span class="sl"><input type="range" min="0" max="25" step="1" bind:value={dz} aria-label="Deadzone ring, percent" /><span class="mono">{dz}%</span></span>
    </label>

    <!-- 3. range -->
    <div class="block">
      <h4>Range test</h4>
      <p class="note">Rotate both sticks around their full edge; the grey outline is the furthest each direction reached. A worn or sticky stick shows as a dented circle.</p>
      <div class="row">
        <button class="btn" aria-pressed={rangeOn} onclick={() => (rangeOn = !rangeOn)}>{rangeOn ? 'Stop' : 'Rotate both sticks'}</button>
        <button class="btn ghost" onclick={resetRange}>Reset</button>
      </div>
    </div>

    <!-- 4. drift -->
    <div class="block">
      <h4>Drift check</h4>
      <p class="note">Leave the sticks and triggers untouched. Samples for about 3 seconds.</p>
      <div class="row">
        <button class="btn" onclick={startDrift} disabled={!polling || sel < 0 || drift.phase === 'running'}>{drift.phase === 'running' ? `Checking ${drift.pct}%` : 'Check drift'}</button>
        {#if drift.phase === 'running'}<span class="lab">Do not touch the controller.</span>{/if}
        {#if drift.phase === 'moved'}<span class="bad">You moved it, try again.</span>{/if}
      </div>
      {#if drift.phase === 'done' && drift.res}
        <table class="tbl mono">
          <thead><tr><th>Stick</th><th>Mean x, y</th><th>Offset</th><th>Noise p-p</th><th>Verdict</th><th>Min deadzone</th></tr></thead>
          <tbody>
            <!-- the result only: the chips pop in one after another and the numbers count up; the live readings above are raw -->
            {#each driftSticks as [n, d], i (n)}
              <tr><td>{n}</td><td>{d.meanX.toFixed(1)}%, {d.meanY.toFixed(1)}%</td><td {@attach countUp(d.offset, fmt1)}>{fmt1(d.offset)}</td><td {@attach countUp(d.noise, fmt1)}>{fmt1(d.noise)}</td><td><span class="vchip {d.verdict}" {@attach popIn(i * 60)}>{d.verdict}</span></td><td {@attach countUp(d.suggested, fmtPct)}>{fmtPct(d.suggested)}</td></tr>
            {/each}
          </tbody>
        </table>
        <p class="small">Triggers at rest: LT {pct(drift.res.lt.rest, 1)}%, RT {pct(drift.res.rt.rest, 1)}% {drift.res.lt.rest === 0 && drift.res.rt.rest === 0 ? '(should be 0: OK)' : '(should be 0)'}</p>
        <p class="note">OK under 2%, Minor 2 to 6%, Drift over 6% (mean offset from centre).</p>
      {/if}
    </div>

    <!-- 5. mapping -->
    <div class="block">
      <h4>Mapping</h4>
      <table class="tbl mono">
        <caption class="sr">Standard-mapping buttons and axes, what Brisk sends, and the live value</caption>
        <thead><tr><th>Index</th><th>Control</th><th>Brisk sends</th><th>Live</th></tr></thead>
        <tbody>
          {#each ROWS as r (r.kind + r.index)}
            {@const live = r.kind === 'button' ? view.buttons[r.index] : null}
            {@const ax = r.kind === 'axis' ? [view.left.x, view.left.y, view.right.x, view.right.y][r.index] : 0}
            <tr class:hit={live?.pressed || (r.kind === 'axis' && Math.abs(ax) > dz / 100)}>
              <td>{r.kind === 'button' ? 'B' : 'A'}{r.index}</td>
              <td>{r.label}</td>
              <td>{r.sends}</td>
              <td>{r.kind === 'button' ? (live!.pressed ? 'pressed' : '') + (r.index === 6 || r.index === 7 ? ` ${pct(live!.value)}%` : '') : sgn(ax)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <!-- 6. rumble -->
    <div class="block">
      <h4>Rumble test</h4>
      <div class="row">
        <button class="btn ghost" disabled={!rumbleOk} onclick={() => rumble(0, 1)}>Weak</button>
        <button class="btn ghost" disabled={!rumbleOk} onclick={() => rumble(1, 0)}>Strong</button>
        <button class="btn ghost" disabled={!rumbleOk} onclick={() => rumble(1, 1)}>Both</button>
      </div>
      {#if !rumbleOk}<p class="note">{sel < 0 ? 'No controller yet.' : 'This controller does not report rumble support to the browser.'}</p>{/if}
    </div>
  </Card>
</section>

<style>
  .ct { margin-top: 14px; }
  .ct :global(.ctcard) { max-width: 880px; }
  .ct :global(.ctcard:hover) { transform: none; }
  .block { margin-top: 18px; padding-top: 14px; border-top: 1px solid var(--line); }
  .note { margin: 8px 0 0; font-size: 12px; color: var(--muted); }
  .note.first { margin-top: 10px; }
  .hint { margin: 0; color: var(--warn); font-size: 13px; }
  .msg { margin: 10px 0 0; font-size: 12.5px; color: var(--accent); }
  .small { margin: 8px 0 0; font-size: 12.5px; overflow-wrap: anywhere; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .opt { display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-top: 12px; flex-wrap: wrap; }
  .opt :global(.dd) { max-width: 100%; overflow: hidden; }
  .sl { display: flex; align-items: center; gap: 10px; }
  .sl input { accent-color: var(--accent); width: 160px; }
  .sl span { min-width: 3ch; text-align: right; }
  .row { display: flex; gap: 10px; margin-top: 12px; flex-wrap: wrap; align-items: center; }
  .btn[aria-pressed='true'] { background: var(--warn); color: #201700; }
  .bad { color: var(--bad); font-size: 13px; }
  .live { display: grid; grid-template-columns: minmax(260px, 1.3fr) minmax(260px, 1fr); gap: 18px; margin-top: 16px; align-items: center; }
  @media (max-width: 640px) { .live { grid-template-columns: 1fr; } }
  .sticks { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .stick { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .stick svg { width: 100%; max-width: 150px; height: auto; align-self: center; }
  .ring { fill: #0b0c0f; stroke: #343a48; stroke-width: .02; }
  .cross { stroke: #2a2f3a; stroke-width: .015; }
  .dzr { fill: color-mix(in srgb, var(--warn) 10%, transparent); stroke: var(--warn); stroke-width: .015; stroke-dasharray: .05 .04; }
  .gate { fill: color-mix(in srgb, #fff 6%, transparent); stroke: var(--muted); stroke-width: .02; }
  .trail { fill: none; stroke: var(--accent); stroke-width: .025; opacity: .55; stroke-linejoin: round; }
  .dot { fill: var(--muted); }
  .dot.out { fill: var(--accent); }
  .nums { display: flex; flex-direction: column; font-size: 11.5px; line-height: 1.5; }
  .nums b { font: 600 12px var(--font); color: #c4cad6; }
  .tbl { width: 100%; border-collapse: collapse; font-size: 12px; margin-top: 10px; }
  .tbl th { text-align: left; color: var(--muted); font-weight: 500; padding: 4px 8px 4px 0; border-bottom: 1px solid var(--line); }
  .tbl td { padding: 3px 8px 3px 0; border-bottom: 1px solid color-mix(in srgb, var(--line) 50%, transparent); }
  .tbl tr.hit td { background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--fg); }
  .vchip { display: inline-block; padding: 1px 9px; border-radius: 999px; background: color-mix(in srgb, currentColor 14%, transparent); }
  .OK { color: var(--ok); }
  .Minor { color: var(--warn); }
  .Drift { color: var(--bad); }
  .block:has(.tbl) { overflow-x: auto; }
</style>
