import type { CommandItem } from './registry';

const tier = (label: string, q: string) => {
  const l = label.toLowerCase();
  if (l.startsWith(q)) return 0;
  if (l.includes(' ' + q)) return 1; // word start
  return l.includes(q) ? 2 : -1;
};

/** Prefix, then word-start, then substring matches; stable within a tier. Empty query: routes first. */
export function rank(query: string, items: CommandItem[]): CommandItem[] {
  const q = query.trim().toLowerCase();
  if (!q) {
    const route = (i: CommandItem) => (i.id.startsWith('route:') ? 0 : 1);
    return [...items].sort((a, b) => route(a) - route(b)); // Array.sort is stable
  }
  return items
    .map((it) => [tier(it.label, q), it] as const)
    .filter(([t]) => t >= 0)
    .sort((a, b) => a[0] - b[0])
    .map(([, it]) => it);
}

export type KeyLike = { key: string; target?: unknown; ctrlKey?: boolean; metaKey?: boolean; altKey?: boolean; preventDefault(): void };

export function isTyping(t: unknown): boolean {
  const e = t as { tagName?: string; isContentEditable?: boolean } | null;
  return !!e && (['INPUT', 'TEXTAREA', 'SELECT'].includes(e.tagName ?? '') || !!e.isContentEditable);
}

/** Matches plain-key sequences like 'g l'. Returns true when the key was consumed. */
export function sequenceMatcher(seqs: Record<string, () => void>, timeoutMs = 1000) {
  let buf: string[] = [];
  let last = 0;
  return (e: KeyLike): boolean => {
    if (isTyping(e.target) || e.ctrlKey || e.metaKey || e.altKey) return false;
    const now = Date.now();
    if (now - last > timeoutMs) buf = [];
    last = now;
    const tryKeys = (b: string[]) => {
      const s = b.join(' ');
      if (seqs[s]) return 'done' as const;
      return Object.keys(seqs).some((k) => k.startsWith(s + ' ')) ? ('more' as const) : null;
    };
    let next = [...buf, e.key];
    let r = tryKeys(next);
    if (!r) r = tryKeys((next = [e.key])); // wrong second key may itself start a sequence
    if (r === 'done') {
      buf = [];
      seqs[next.join(' ')]();
      return true;
    }
    buf = r ? next : [];
    return !!r;
  };
}

/** One keydown listener: Ctrl/Cmd+K anywhere, '/' and 'g h|l|s' outside inputs. Returns the disposer. */
export function attachShortcuts(target: EventTarget, h: { open(): void; go(path: string): void }): () => void {
  const seq = sequenceMatcher({ 'g h': () => h.go('/'), 'g l': () => h.go('/library'), 'g s': () => h.go('/stream') });
  const on = (ev: Event) => {
    const e = ev as unknown as KeyLike;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      h.open();
    } else if (e.key === '/' && !isTyping(e.target) && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      h.open();
    } else if (seq(e)) e.preventDefault();
  };
  target.addEventListener('keydown', on);
  return () => target.removeEventListener('keydown', on);
}
