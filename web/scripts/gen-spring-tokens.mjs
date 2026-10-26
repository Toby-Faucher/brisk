// Writes the CSS spring tokens (--spring-*) into src/styles/tokens.css: anime.js solves each spring offline and
// `waapi.convertEase` samples it into a CSS `linear()` easing, so CSS gets real springs with no JS at runtime (the
// Stream page included). A spring sets its own duration (--spring-*-dur): its settling time, or `cap` when given (the
// curve is then sampled over the capped window and lands on 1; the cut tail is under 1 % of the travel).
//   node scripts/gen-spring-tokens.mjs          (rewrites the generated block; commit the result)
//   npm run springs:check                       (exit 1 if tokens.css is stale; src/styles/tokens.test.ts does the same)
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { spring } from 'animejs/easings';
import { waapi } from 'animejs/waapi';

// mass 1. zeta = damping / (2 * sqrt(stiffness)): >= 1 never overshoots; Linear/Emil taste keeps it near 1.
export const SPRINGS = {
  soft: { stiffness: 400, damping: 40, cap: 450, use: 'hover lifts: quick, no overshoot' },
  snappy: { stiffness: 380, damping: 34, use: 'presses, glides, small controls: fast, a hair of overshoot' },
  settle: { stiffness: 150, damping: 19, use: 'panels and bars sliding in: a small, single settle' },
};
const SAMPLES = 40; // linear() points: 40 is visually identical to 100 for these curves at a third of the bytes

export const FILE = fileURLToPath(new URL('../src/styles/tokens.css', import.meta.url));
const BEGIN = '/* BEGIN generated springs: node web/scripts/gen-spring-tokens.mjs (do not edit by hand) */';
const END = '/* END generated springs */';
const esc = (s) => s.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&');

/** Time (ms) at which the sampled linear() curve first reaches 90 % of the travel. */
export function t90(points, dur) {
  const i = points.findIndex((v) => v >= 0.9);
  const a = points[i - 1], b = points[i], step = dur / (points.length - 1);
  return Math.round((i - 1 + (0.9 - a) / (b - a)) * step);
}

export function springs() {
  return Object.entries(SPRINGS).map(([name, { stiffness, damping, cap, use }]) => {
    const s = spring({ stiffness, damping });
    const dur = cap && cap < s.settlingDuration ? cap : s.settlingDuration;
    const k = dur / s.settlingDuration;
    const ease = (t) => (t >= 1 ? 1 : s.ease(t * k));
    const css = waapi.convertEase(ease, SAMPLES);
    const points = css.slice(7, -1).split(',').map(Number);
    let peak = 0;
    for (let i = 0; i <= 1000; i++) peak = Math.max(peak, s.ease(i / 1000));
    const zeta = damping / (2 * Math.sqrt(stiffness));
    return { name, css, dur, t90: t90(points, dur), use, stiffness, damping, zeta, peak, settle: s.settlingDuration };
  });
}

export function block() {
  const lines = springs().flatMap((x) => [
    `  /* ${x.use}; stiffness ${x.stiffness}, damping ${x.damping}, zeta ${x.zeta.toFixed(2)}, overshoot ${((x.peak - 1) * 100).toFixed(1)}%, ` +
      `90% at ${x.t90} ms, ${x.dur === x.settle ? `settles in ${x.dur} ms` : `capped at ${x.dur} ms (settles in ${x.settle})`} */`,
    `  --spring-${x.name}: ${x.css};`,
    `  --spring-${x.name}-dur: ${x.dur}ms;`,
  ]);
  return `${BEGIN}\n:root {\n${lines.join('\n')}\n}\n${END}`;
}

/** tokens.css with the generated block replaced (or appended). */
export function render(css) {
  const re = new RegExp(`${esc(BEGIN)}[\\s\\S]*?${esc(END)}`);
  return re.test(css) ? css.replace(re, block()) : `${css.trimEnd()}\n\n${block()}\n`;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === fs.realpathSync(process.argv[1])) {
  const css = fs.readFileSync(FILE, 'utf8');
  const next = render(css);
  if (process.argv.includes('--check')) {
    if (next !== css) (console.error('tokens.css springs are stale: run node web/scripts/gen-spring-tokens.mjs'), process.exit(1));
    console.log('tokens.css springs are up to date');
  } else {
    fs.writeFileSync(FILE, next);
    for (const x of springs()) console.log(`--spring-${x.name}: 90% at ${x.t90} ms, duration ${x.dur} ms, overshoot ${((x.peak - 1) * 100).toFixed(1)}%`);
  }
}
