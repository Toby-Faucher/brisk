// Launch options as a form: JSON <-> editable rows, the gamescope flag mapping, the per-field inherit flags and the
// plain-words summary. Pure. hostd validates again on every save; this only keeps the form from sending nonsense.
import type { Game, LaunchOpts, LaunchOverride, LaunchWrapper } from '../../state/api';

/** gamescope flags that take a number, with hostd's largest value (crates/brisk-hostd/src/launch.rs). */
export const GS_NUM = [
  { flag: '-W', label: 'Screen width', max: 16384 },
  { flag: '-H', label: 'Screen height', max: 16384 },
  { flag: '-w', label: 'Game width', max: 16384 },
  { flag: '-h', label: 'Game height', max: 16384 },
  { flag: '-r', label: 'Refresh rate (Hz)', max: 1000 },
] as const;
export const GS_BOOL = [
  { flag: '-f', label: 'Fullscreen' },
  { flag: '-b', label: 'Borderless' },
  { flag: '--fsr', label: 'FSR upscaling' },
] as const;

export type Gs = { on: boolean; num: Record<string, string>; flags: Record<string, boolean> };
export type Form = { mango: boolean; gamemode: boolean; gs: Gs; env: [string, string][]; custom: string[]; extra: string[] };
/** true: the field follows the default. */
export type Inherit = { wrappers: boolean; env: boolean; custom: boolean; extra: boolean };
export const FIELDS = ['wrappers', 'env', 'custom', 'extra'] as const;

export const noGs = (): Gs => ({ on: false, num: {}, flags: {} });
export const emptyForm = (): Form => ({ mango: false, gamemode: false, gs: noGs(), env: [], custom: [], extra: [] });

export function gsArgs(g: Gs): string[] {
  const a: string[] = [];
  for (const n of GS_NUM) if (g.num[n.flag]?.trim()) a.push(n.flag, g.num[n.flag].trim());
  for (const b of GS_BOOL) if (g.flags[b.flag]) a.push(b.flag);
  return a;
}
export function gsParse(args: string[]): Gs {
  const g: Gs = { on: true, num: {}, flags: {} };
  for (let i = 0; i < args.length; i++) {
    if (GS_BOOL.some((b) => b.flag === args[i])) g.flags[args[i]] = true;
    else if (GS_NUM.some((n) => n.flag === args[i])) g.num[args[i]] = args[++i] ?? '';
  }
  return g;
}
/** Problems with the numbers, in words; empty when gamescope is off. A blank number means "leave the flag out". */
export function gsProblems(g: Gs): string[] {
  if (!g.on) return [];
  return GS_NUM.flatMap((n) => {
    const v = g.num[n.flag]?.trim();
    return !v || (/^\d{1,5}$/.test(v) && +v >= 1 && +v <= n.max) ? [] : [`${n.label} needs a whole number from 1 to ${n.max}.`];
  });
}

export function formOf(o: LaunchOpts): Form {
  const gs = o.wrappers.find((w): w is { gamescope: { args: string[] } } => typeof w === 'object');
  return {
    mango: o.wrappers.includes('mangohud'),
    gamemode: o.wrappers.includes('gamemode'),
    gs: gs ? gsParse(gs.gamescope.args) : noGs(),
    env: Object.entries(o.env),
    custom: [...(o.custom ?? [])],
    extra: [...o.extra_args],
  };
}

/** Blank rows at the end are an entry in progress, not an argument. */
export const trimTail = (a: string[]) => {
  let n = a.length;
  while (n && a[n - 1] === '') n--;
  return a.slice(0, n);
};

export function wrappersOf(f: Form): LaunchWrapper[] {
  const w: LaunchWrapper[] = [];
  if (f.mango) w.push('mangohud');
  if (f.gamemode) w.push('gamemode');
  if (f.gs.on) w.push({ gamescope: { args: gsArgs(f.gs) } });
  return w;
}
export const envOf = (f: Form): Record<string, string> => Object.fromEntries(f.env.filter(([k]) => k));

export function optionsOf(f: Form): LaunchOpts {
  const custom = trimTail(f.custom);
  return { wrappers: wrappersOf(f), env: envOf(f), custom: custom.length ? custom : null, extra_args: trimTail(f.extra) };
}

/** The switch off means no custom command, in the same save (hostd refuses a command while it is off). */
export const withAllow = (f: Form, allow: boolean): Form => (allow ? f : { ...f, custom: [] });

export const inheritOf = (o: LaunchOverride | null | undefined): Inherit => ({
  wrappers: o?.wrappers == null,
  env: o?.env == null,
  custom: o?.custom == null,
  extra: o?.extra_args == null,
});
export const allInherit = (i: Inherit) => FIELDS.every((k) => i[k]);

/** Only the fields marked Override. An overridden empty command is `[]`, which clears the default's. */
export function overrideOf(f: Form, i: Inherit): LaunchOverride {
  const o = optionsOf(f);
  const r: LaunchOverride = {};
  if (!i.wrappers) r.wrappers = o.wrappers;
  if (!i.env) r.env = o.env;
  if (!i.custom) r.custom = o.custom ?? [];
  if (!i.extra) r.extra_args = o.extra_args;
  return r;
}

const list = (a: string[]) => (a.length < 2 ? a.join('') : `${a.slice(0, -1).join(', ')} and ${a[a.length - 1]}`);

/** What the effective options do, in plain sentences. */
export function summary(o: LaunchOpts): string {
  const f = formOf(o);
  const names: string[] = [];
  if (f.gs.on) {
    const bits = [
      ...(f.gs.num['-W'] || f.gs.num['-H'] ? [`screen ${f.gs.num['-W'] ?? '?'} by ${f.gs.num['-H'] ?? '?'}`] : []),
      ...(f.gs.num['-w'] || f.gs.num['-h'] ? [`game ${f.gs.num['-w'] ?? '?'} by ${f.gs.num['-h'] ?? '?'}`] : []),
      ...(f.gs.num['-r'] ? [`${f.gs.num['-r']} Hz`] : []),
      ...GS_BOOL.filter((b) => f.gs.flags[b.flag]).map((b) => b.label.toLowerCase().replace('fsr upscaling', 'FSR')),
    ];
    names.push(bits.length ? `gamescope (${bits.join(', ')})` : 'gamescope');
  }
  if (f.gamemode) names.push('GameMode');
  if (f.mango) names.push('MangoHud');
  const out: string[] = [];
  if (names.length) out.push(`Runs under ${list(names)}.`);
  const c = trimTail(f.custom);
  if (c.length) out.push(`Your command wraps the game: ${c.join(' ')}.`);
  const env = Object.entries(o.env).map(([k, v]) => `${k}=${v}`);
  if (env.length) out.push(`Environment: ${env.join(', ')}.`);
  if (o.extra_args.length) out.push(`Extra arguments: ${o.extra_args.join(' ')}.`);
  return out.join(' ') || 'Starts the game as it is, with no options.';
}

/** The id to put in a URL: the Stage 1 id, else the old number (an old hostd). */
export const launchIdOf = (g: Pick<Game, 'id' | 'appid'>) => g.id ?? String(g.appid);
/** Offer the section when hostd says it can; a hostd without caps (old) has no launch options. */
export const hasLaunchOptions = (g: Pick<Game, 'caps'>) => g.caps?.launchOptions === true;
