<script lang="ts" module>
  // The falling-blocks sim lives at module level so it keeps its stack across page changes (the Layer remounts).
  type Cell = [number, number];
  type Piece = { t: number; cells: Cell[]; col: number; row: number; acc: number; speed: number; slam: boolean };
  const CELL = 36;
  const COLORS = ['#00e5ff', '#ffd400', '#c24dff', '#3ee06b', '#ff4a5a', '#4d7bff', '#ff9a1f'];
  const BASE: Cell[][] = [
    [[0, 0], [1, 0], [2, 0], [3, 0]], // I
    [[0, 0], [1, 0], [0, 1], [1, 1]], // O
    [[0, 0], [1, 0], [2, 0], [1, 1]], // T
    [[1, 0], [2, 0], [0, 1], [1, 1]], // S
    [[0, 0], [1, 0], [1, 1], [2, 1]], // Z
    [[0, 0], [0, 1], [1, 1], [2, 1]], // J
    [[2, 0], [0, 1], [1, 1], [2, 1]], // L
  ];
  const norm = (c: Cell[]): Cell[] => {
    const mx = Math.min(...c.map((p) => p[0]));
    const my = Math.min(...c.map((p) => p[1]));
    return c.map(([x, y]) => [x - mx, y - my]);
  };
  const rot = (c: Cell[]): Cell[] => norm(c.map(([x, y]) => [-y, x]));
  const rand = () => Math.floor(Math.random() * 7);
  const MSG = ['Line clear', 'Two lines', 'Three lines', 'Four lines. Nice.'];

  const sim = {
    cols: 0,
    rows: 0,
    grid: [] as number[][],
    fly: [] as Piece[],
    clears: [] as { rows: number[]; t: number }[],
    queue: [rand(), rand()],
    spawn: 0,
    lines: 0,
    score: 0,
    msg: '',
    msgT: 0,
    hook: null as null | (() => void),
  };
  let lastPath: string | null = null;

  const fits = (cells: Cell[], col: number, row: number, g: number[][]) =>
    cells.every(([x, y]) => {
      const c = col + x;
      const r = row + y;
      return c >= 0 && c < sim.cols && r < sim.rows && (r < 0 || g[r][c] === 0);
    });
  const drop = (cells: Cell[], col: number, g: number[][]) => {
    let row = -5;
    while (fits(cells, col, row + 1, g)) row++;
    return row;
  };
  const put = (g: number[][], cells: Cell[], col: number, row: number, v: number) => {
    for (const [x, y] of cells) if (row + y >= 0) g[row + y][col + x] = v;
  };
  const stackTop = () => {
    for (let r = 0; r < sim.rows; r++) if (sim.grid[r].some(Boolean)) return sim.rows - r;
    return 0;
  };

  // Greedy bot: lowest, flattest, fewest holes. A little noise keeps the stack lively.
  function score(g: number[][]) {
    const { rows, cols } = sim;
    let agg = 0;
    let holes = 0;
    let bump = 0;
    let prev = -1;
    let lines = 0;
    for (let c = 0; c < cols; c++) {
      let h = 0;
      let seen = false;
      for (let r = 0; r < rows; r++) {
        if (g[r][c]) {
          if (!seen) {
            seen = true;
            h = rows - r;
          }
        } else if (seen) holes++;
      }
      agg += h;
      if (prev >= 0) bump += Math.abs(h - prev);
      prev = h;
    }
    for (let r = 0; r < rows; r++) if (g[r].every(Boolean)) lines++;
    return agg * 0.5 + holes * 4 + bump * 0.25 - lines * 8 + Math.random() * 1.5;
  }
  function choose(t: number) {
    const g = sim.grid.map((r) => r.slice());
    for (const p of sim.fly) put(g, p.cells, p.col, drop(p.cells, p.col, g), p.t + 1);
    let cells = BASE[t];
    let best = { s: 1e9, cells, col: 0 };
    for (let k = 0; k < 4; k++, cells = rot(cells)) {
      const w = Math.max(...cells.map((p) => p[0])) + 1;
      for (let col = 0; col + w <= sim.cols; col++) {
        const row = drop(cells, col, g);
        if (row + Math.min(...cells.map((p) => p[1])) < 0) continue;
        put(g, cells, col, row, 9);
        const s = score(g);
        put(g, cells, col, row, 0);
        if (s < best.s) best = { s, cells, col };
      }
    }
    return best;
  }
  function spawn(slam = false) {
    const t = sim.queue.shift() as number;
    sim.queue.push(rand());
    const b = choose(t);
    sim.fly.push({ t, cells: b.cells, col: b.col, row: -4, acc: 0, speed: slam ? 70 : 3.2, slam });
    sim.hook?.();
  }
  function reset() {
    sim.grid = Array.from({ length: sim.rows }, () => Array(sim.cols).fill(0));
    sim.fly = [];
    sim.clears = [];
  }
  function lock(p: Piece) {
    if (p.row + Math.min(...p.cells.map((c) => c[1])) < 0) return reset();
    put(sim.grid, p.cells, p.col, p.row, p.t + 1);
    sim.score += p.slam ? 40 : 10;
    const full = sim.grid.flatMap((r, i) => (r.every(Boolean) && !sim.clears.some((c) => c.rows.includes(i)) ? [i] : []));
    if (full.length) sim.clears.push({ rows: full, t: 0 });
    // too tall to stay out of the way: sweep the bottom rows as if the player cleared them
    else if (stackTop() > 6 && !sim.clears.length) sim.clears.push({ rows: [1, 2, 3, 4].map((n) => sim.rows - n), t: 0 });
    sim.hook?.();
  }
  function step(dt: number) {
    sim.spawn -= dt;
    if (sim.spawn <= 0 && sim.fly.length < 8) {
      spawn();
      sim.spawn = 0.8;
    }
    for (const p of sim.fly.slice()) {
      p.acc += dt * p.speed;
      while (p.acc >= 1) {
        if (fits(p.cells, p.col, p.row + 1, sim.grid)) {
          p.row++;
          p.acc -= 1;
        } else {
          p.acc = 0;
          sim.fly.splice(sim.fly.indexOf(p), 1);
          lock(p);
          break;
        }
      }
    }
    for (const c of sim.clears.slice()) {
      c.t += dt;
      if (c.t < 0.5) continue;
      sim.clears.splice(sim.clears.indexOf(c), 1);
      const rows = c.rows.filter((r) => r >= 0 && r < sim.rows);
      sim.grid = [...rows.map(() => Array(sim.cols).fill(0)), ...sim.grid.filter((_, i) => !rows.includes(i))];
      // other pending clears index old rows: move the ones above this one down
      for (const o of sim.clears) o.rows = o.rows.map((r) => r + rows.filter((x) => x > r).length);
      sim.lines += rows.length;
      sim.score += [0, 100, 300, 500, 800][Math.min(rows.length, 4)] * (1 + Math.floor(sim.lines / 5));
      sim.msg = MSG[Math.min(rows.length, 4) - 1] ?? '';
      sim.msgT = 2.6;
      sim.hook?.();
    }
    if (sim.msgT > 0 && (sim.msgT -= dt) <= 0) {
      sim.msg = '';
      sim.hook?.();
    }
  }
  function setup(w: number, h: number) {
    const cols = Math.max(8, Math.floor(w / CELL));
    const rows = Math.ceil(h / CELL);
    if (cols === sim.cols && rows === sim.rows) return;
    sim.cols = cols;
    sim.rows = rows;
    reset();
    for (let i = 0; i < 1400; i++) step(0.05); // start with a stack already standing
  }
</script>

<script lang="ts">
  let { path }: { path: string } = $props();
  const stream = $derived(/^\/stream/.test(path));
  let canvas: HTMLCanvasElement | undefined = $state();
  let hud = $state({ lines: 0, score: 0, level: 1, next: 0, msg: '' });
  const sync = () => (hud = { lines: sim.lines, score: sim.score, level: 1 + Math.floor(sim.lines / 5), next: sim.queue[0], msg: sim.msg });

  function cell(ctx: CanvasRenderingContext2D, x: number, y: number, color: string) {
    const s = CELL;
    ctx.fillStyle = color;
    ctx.fillRect(x + 1, y + 1, s - 2, s - 2);
    ctx.fillStyle = 'rgb(255 255 255 / .5)';
    ctx.fillRect(x + 1, y + 1, s - 2, 4);
    ctx.fillRect(x + 1, y + 1, 4, s - 2);
    ctx.fillStyle = 'rgb(0 0 0 / .38)';
    ctx.fillRect(x + 1, y + s - 5, s - 2, 4);
    ctx.fillRect(x + s - 5, y + 1, 4, s - 2);
    ctx.fillStyle = 'rgb(255 255 255 / .28)';
    ctx.fillRect(x + 9, y + 9, 9, 9);
  }

  $effect(() => {
    const cv = canvas;
    if (stream || !cv) return;
    const ctx = cv.getContext('2d') as CanvasRenderingContext2D;
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
    let raf = 0;
    let last = 0;
    let ox = 0;
    let oy = 0;
    const draw = (now: number) => {
      ctx.clearRect(0, 0, innerWidth, innerHeight);
      ctx.strokeStyle = 'rgb(120 140 255 / .16)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let c = 0; c <= sim.cols; c++) {
        ctx.moveTo(ox + c * CELL + 0.5, 0);
        ctx.lineTo(ox + c * CELL + 0.5, innerHeight);
      }
      for (let r = 0; r <= sim.rows; r++) {
        ctx.moveTo(ox, oy + r * CELL + 0.5);
        ctx.lineTo(ox + sim.cols * CELL, oy + r * CELL + 0.5);
      }
      ctx.stroke();
      const flashing = new Set(sim.clears.flatMap((c) => c.rows));
      const on = Math.floor(now / 70) % 2 === 0;
      for (let r = 0; r < sim.rows; r++)
        for (let c = 0; c < sim.cols; c++) {
          const v = sim.grid[r][c];
          if (v) cell(ctx, ox + c * CELL, oy + r * CELL, flashing.has(r) && on ? '#ffffff' : COLORS[v - 1]);
        }
      for (const p of sim.fly) {
        const ghost = drop(p.cells, p.col, sim.grid);
        ctx.setLineDash([5, 5]);
        ctx.strokeStyle = 'rgb(255 255 255 / .35)';
        for (const [x, y] of p.cells) ctx.strokeRect(ox + (p.col + x) * CELL + 3, oy + (ghost + y) * CELL + 3, CELL - 6, CELL - 6);
        ctx.setLineDash([]);
        const off = fits(p.cells, p.col, p.row + 1, sim.grid) ? p.acc : 0;
        for (const [x, y] of p.cells) cell(ctx, ox + (p.col + x) * CELL, oy + (p.row + y + off) * CELL, COLORS[p.t]);
      }
    };
    const size = () => {
      const w = innerWidth;
      const h = innerHeight;
      const d = devicePixelRatio || 1;
      cv.width = w * d;
      cv.height = h * d;
      ctx.setTransform(d, 0, 0, d, 0, 0);
      setup(w, h);
      ox = (w - sim.cols * CELL) / 2;
      oy = h - sim.rows * CELL;
      sim.hook = sync;
      sync();
      draw(performance.now());
    };
    const tick = (now: number) => {
      raf = requestAnimationFrame(tick);
      const dt = Math.min(0.1, (now - last) / 1000);
      last = now;
      step(dt);
      draw(now);
    };
    size();
    // each page change is a hard drop
    if (lastPath !== null && lastPath !== path && !reduced) {
      sim.score += 25;
      sim.msg = 'Hard drop';
      sim.msgT = 2;
      spawn(true);
    }
    lastPath = path;
    if (!reduced)
      raf = requestAnimationFrame((n) => {
        last = n;
        raf = requestAnimationFrame(tick);
      });
    addEventListener('resize', size);
    return () => {
      cancelAnimationFrame(raf);
      removeEventListener('resize', size);
      sim.hook = null;
    };
  });
</script>

{#if !stream}
  <div class="tt-well" aria-hidden="true"><canvas bind:this={canvas}></canvas></div>
  <div class="tt-hud" aria-hidden="true">
    <div class="tt-box">
      <b>Next</b>
      <div class="tt-next">
        {#each BASE[hud.next] as [x, y]}<i class="c{hud.next}" style="left:{x * 18}px;top:{y * 18}px"></i>{/each}
      </div>
    </div>
    <div class="tt-box"><b>Level</b><span>{hud.level}</span></div>
    <div class="tt-box"><b>Lines</b><span>{hud.lines}</span></div>
    <div class="tt-box"><b>Score</b><span>{String(hud.score).padStart(6, '0')}</span></div>
    <p class="tt-msg" class:on={hud.msg}>{hud.msg}</p>
  </div>
{/if}

<style>
  .tt-well { position: fixed; inset: 0; z-index: -1; pointer-events: none; overflow: hidden; background: radial-gradient(120% 90% at 50% 0%, #10142e 0%, #080a18 70%, #05060f 100%); }
  canvas { width: 100%; height: 100%; display: block; opacity: 0.24; }
  .tt-hud { position: fixed; z-index: 5; top: 18px; right: 18px; width: 150px; display: none; flex-direction: column; gap: 10px; pointer-events: none; font-family: 'Audiowide', system-ui, sans-serif; }
  .tt-box { position: relative; padding: 22px 12px 10px; border-radius: 10px; background: #0b0e22; border: 2px solid #3a4486; box-shadow: 0 0 0 2px #05060f, inset 0 0 18px rgb(80 100 255 / 0.18); }
  .tt-box b { position: absolute; top: 5px; left: 12px; font-weight: 400; font-size: 11px; letter-spacing: 0.18em; text-transform: uppercase; color: #aeb7f5; }
  .tt-box span { display: block; font-size: 22px; color: #fff; letter-spacing: 0.06em; text-align: right; font-variant-numeric: tabular-nums; }
  .tt-next { position: relative; height: 40px; margin: 4px 0 0 24px; }
  .tt-next i { position: absolute; width: 18px; height: 18px; background-image: linear-gradient(145deg, rgb(255 255 255 / 0.55), transparent 50%), linear-gradient(0deg, rgb(0 0 0 / 0.35), transparent 40%); box-shadow: inset 0 0 0 1px rgb(0 0 0 / 0.4); }
  .c0 { background-color: #00e5ff; } .c1 { background-color: #ffd400; } .c2 { background-color: #c24dff; } .c3 { background-color: #3ee06b; }
  .c4 { background-color: #ff4a5a; } .c5 { background-color: #4d7bff; } .c6 { background-color: #ff9a1f; }
  .tt-msg { margin: 0; min-height: 18px; font-size: 12px; letter-spacing: 0.08em; text-align: center; color: #ffd400; opacity: 0; transition: opacity 0.3s; }
  .tt-msg.on { opacity: 1; }
  @media (min-width: 1240px) { .tt-hud { display: flex; } }
</style>
