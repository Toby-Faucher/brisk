<script lang="ts">
  import { onMount } from 'svelte';
  import { padName, type PadLike } from '../../lib/gamepad';
  import Card from '../../ui/Card.svelte';
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
  const cur = () => pads().find((g) => g && g.connected && g.index === sel) ?? null;
  const pct = (v: number, d = 0) => (v * 100).toFixed(d);
  const sgn = (v: number) => (v >= 0 ? '+' : '') + v.toFixed(2);

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
    const g = all.find((p) => p && p.connected && p.index === sel);
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

  // Diagram geometry (viewBox units).
  const FACE = [[3, 'Y', 268, 76], [0, 'A', 268, 120], [2, 'X', 246, 98], [1, 'B', 290, 98]] as const;
  const SMALL = [[8, 'Back', 160, 98, 8], [9, 'Start', 200, 98, 8], [16, 'Home', 180, 74, 10]] as const;
  const DPAD = [[12, 135, 132, '↑'], [13, 135, 164, '↓'], [14, 119, 148, '←'], [15, 151, 148, '→']] as const;
  const pr = (i: number) => view.buttons[i].pressed;
  const driftSticks = $derived(drift.res ? ([['Left stick', drift.res.left], ['Right stick', drift.res.right]] as [string, StickDrift][]) : []);
</script>

<section bind:this={root} aria-label="Controller test" class="ct">
  <Card cols={2} class="ctcard">
    <h4>Controller</h4>
    <p class="note first">Live view of your pad, to spot stick drift and see what each button sends. Runs in this page only: nothing is sent to the host, and it stops when you leave.</p>

    <div class="sr" role="status">{status}</div>
    {#if status}<p class="msg" aria-hidden="true">{status}</p>{/if}

    <!-- 1. status -->
    <div class="block">
      {#if choices.length === 0}
        <p class="hint">Press a button on your controller (Chrome only shows it after a press)</p>
      {:else}
        <label class="opt"><span class="lab">Pad</span>
          <select value={sel} onchange={(e) => (sel = Number(e.currentTarget.value))} aria-label="Controller">
            {#each choices as c (c.index)}<option value={c.index}>{c.index}: {padName(c.id)}</option>{/each}
          </select>
        </label>
        {#each choices.filter((c) => c.index === sel) as c (c.index)}
          <p class="mono small">{c.id}<br />{c.detail}</p>
        {/each}
      {/if}
      <p class="small"><span class="lab">Poll rate</span> <span class="mono">{polling ? `${hz} Hz` : 'paused'}</span>
        {#if !polling}<span class="lab"> (the page needs focus)</span>{/if}</p>
    </div>

    <!-- 2. diagram + sticks -->
    <div class="live">
      <svg class="pad" viewBox="-6 0 372 236" aria-hidden="true" focusable="false">
        <path class="body" d="M64 46 H296 Q336 46 346 120 L356 196 Q360 226 332 226 Q310 226 298 200 L284 178 H76 L62 200 Q50 226 28 226 Q0 226 4 196 L14 120 Q24 46 64 46Z" />
        {#each [['LT', view.lt, 62], ['RT', view.rt, 228]] as [n, v, x] (n)}
          <rect class="trk" x={x as number} y="4" width="70" height="16" rx="5" />
          <rect class="fill" x={x as number} y="4" width={(v as number) * 70} height="16" rx="5" />
          <text class="t" x={(x as number) + 35} y="15.5" text-anchor="middle">{n} {pct(v as number)}%</text>
        {/each}
        {#each [['LB', 4, 62], ['RB', 5, 228]] as [n, i, x] (n)}
          <rect class="key" class:on={pr(i as number)} x={x as number} y="24" width="70" height="16" rx="8" />
          <text class="t" class:on={pr(i as number)} x={(x as number) + 35} y="35.5" text-anchor="middle">{n}</text>
        {/each}
        <circle class="well" cx="92" cy="98" r="26" />
        <circle class="cap" class:on={pr(10)} cx={92 + view.left.x * 12} cy={98 + view.left.y * 12} r="17" />
        <text class="t" class:on={pr(10)} x={92 + view.left.x * 12} y={101 + view.left.y * 12} text-anchor="middle">L3</text>
        <circle class="well" cx="222" cy="152" r="26" />
        <circle class="cap" class:on={pr(11)} cx={222 + view.right.x * 12} cy={152 + view.right.y * 12} r="17" />
        <text class="t" class:on={pr(11)} x={222 + view.right.x * 12} y={155 + view.right.y * 12} text-anchor="middle">R3</text>
        {#each DPAD as [i, x, y, a] (i)}
          <rect class="key" class:on={pr(i)} x={x - 8} y={y - 8} width="16" height="16" rx="4" />
          <text class="t" class:on={pr(i)} {x} y={y + 3.5} text-anchor="middle">{a}</text>
        {/each}
        {#each FACE as [i, n, x, y] (i)}
          <circle class="key" class:on={pr(i)} cx={x} cy={y} r="11" />
          <text class="t" class:on={pr(i)} {x} y={y + 3.5} text-anchor="middle">{n}</text>
        {/each}
        {#each SMALL as [i, n, x, y, r] (i)}
          <circle class="key" class:on={pr(i)} cx={x} cy={y} {r} />
          <text class="t s" class:on={pr(i)} {x} y={y + (i === 16 ? 3 : 18)} text-anchor="middle">{n}</text>
        {/each}
      </svg>

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
            {#each driftSticks as [n, d] (n)}
              <tr><td>{n}</td><td>{d.meanX.toFixed(1)}%, {d.meanY.toFixed(1)}%</td><td>{d.offset.toFixed(1)}%</td><td>{d.noise.toFixed(1)}%</td><td class={d.verdict}>{d.verdict}</td><td>{d.suggested}%</td></tr>
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
  .opt select { background: var(--panel2); color: var(--fg); border: 1px solid var(--line); border-radius: 9px; padding: 5px 8px; font: inherit; font-size: 13px; max-width: 100%; }
  .sl { display: flex; align-items: center; gap: 10px; }
  .sl input { accent-color: var(--accent); width: 160px; }
  .sl span { min-width: 3ch; text-align: right; }
  .row { display: flex; gap: 10px; margin-top: 12px; flex-wrap: wrap; align-items: center; }
  .btn[aria-pressed='true'] { background: var(--warn); color: #201700; }
  .bad { color: var(--bad); font-size: 13px; }
  .live { display: grid; grid-template-columns: minmax(260px, 1.3fr) minmax(260px, 1fr); gap: 18px; margin-top: 16px; align-items: center; }
  @media (max-width: 640px) { .live { grid-template-columns: 1fr; } }
  .pad { width: 100%; height: auto; }
  .body { fill: var(--panel2); stroke: var(--line); stroke-width: 1.5; }
  .trk, .well { fill: #0b0c0f; stroke: var(--line); }
  .fill { fill: var(--accent); opacity: .85; }
  .key { fill: #0b0c0f; stroke: #343a48; stroke-width: 1.2; }
  .key.on { fill: var(--accent); stroke: var(--accent); }
  .cap { fill: #1d212b; stroke: #343a48; stroke-width: 1.2; }
  .cap.on { fill: var(--accent); }
  .t { font: 600 9px var(--font); fill: var(--muted); pointer-events: none; }
  .t.on { fill: var(--accent-fg); }
  .trk ~ .t { fill: var(--fg); }
  .t.s { font-size: 8px; }
  .t.s.on { fill: var(--fg); }
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
  .OK { color: var(--ok); }
  .Minor { color: var(--warn); }
  .Drift { color: var(--bad); }
  .block:has(.tbl) { overflow-x: auto; }
</style>
