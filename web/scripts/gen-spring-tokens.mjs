// Writes the CSS spring tokens (--spring-*) into src/styles/tokens.css: anime.js solves each spring offline and
// `waapi.convertEase` samples it into a CSS `linear()` easing, so CSS gets real springs with no JS at runtime (the
// Stream page included). A spring also sets its own settling time: use it as the duration (--spring-*-dur).
//   node scripts/gen-spring-tokens.mjs          (rewrites the generated block; commit the result)
//   node scripts/gen-spring-tokens.mjs --check  (exit 1 if tokens.css is stale)
import fs from 'node:fs';
import { spring } from 'animejs/easings';
import { waapi } from 'animejs/waapi';

// mass 1. zeta = damping / (2 * sqrt(stiffness)): >= 1 never overshoots; Linear/Emil taste keeps it near 1.
const SPRINGS = {
  soft: { stiffness: 170, damping: 26, use: 'hover lifts: calm, no visible overshoot' },
  snappy: { stiffness: 380, damping: 34, use: 'presses and small controls: fast, a hair of overshoot' },
  settle: { stiffness: 150, damping: 19, use: 'panels and bars sliding in: a small, single settle' },
};
const SAMPLES = 40; // linear() points: 40 is visually identical to 100 for these curves at a third of the bytes

const FILE = new URL('../src/styles/tokens.css', import.meta.url);
const BEGIN = '/* BEGIN generated springs: node web/scripts/gen-spring-tokens.mjs (do not edit by hand) */';
const END = '/* END generated springs */';

const lines = [];
for (const [name, { stiffness, damping, use }] of Object.entries(SPRINGS)) {
  const s = spring({ stiffness, damping });
  let peak = 0;
  for (let i = 0; i <= 1000; i++) peak = Math.max(peak, s.ease(i / 1000));
  const zeta = damping / (2 * Math.sqrt(stiffness));
  lines.push(`  /* ${use}; stiffness ${stiffness}, damping ${damping}, zeta ${zeta.toFixed(2)}, overshoot ${((peak - 1) * 100).toFixed(1)}%, settles in ${s.settlingDuration} ms */`);
  lines.push(`  --spring-${name}: ${waapi.convertEase(s.ease, SAMPLES)};`);
  lines.push(`  --spring-${name}-dur: ${s.settlingDuration}ms;`);
}
const block = `${BEGIN}\n:root {\n${lines.join('\n')}\n}\n${END}`;

const css = fs.readFileSync(FILE, 'utf8');
const re = new RegExp(`${BEGIN.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&')}[\\s\\S]*?${END.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&')}`);
const next = re.test(css) ? css.replace(re, block) : `${css.trimEnd()}\n\n${block}\n`;
if (process.argv.includes('--check')) {
  if (next !== css) (console.error('tokens.css springs are stale: run node web/scripts/gen-spring-tokens.mjs'), process.exit(1));
  console.log('tokens.css springs are up to date');
} else {
  fs.writeFileSync(FILE, next);
  console.log(lines.filter((l) => l.includes('/*')).join('\n'));
}
