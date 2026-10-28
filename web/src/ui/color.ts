/** Machine colours: the picker palette, hex cleanup and the text colour that reads on a header of that colour. */
export const PALETTE = ['#60a5fa', '#a78bfa', '#f472b6', '#f87171', '#fb923c', '#facc15', '#4ade80', '#2dd4bf']; // blue, violet, pink, red, orange, yellow, green, teal

const BASE = [0x0b, 0x0d, 0x12]; // #0b0d12, the dark end of the header gradient

/** `#rgb` or `#rrggbb` in any case, trimmed, as lowercase `#rrggbb`; null for anything else. */
export function normalizeHex(s: string): string | null {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(String(s).trim());
  if (!m) return null;
  const h = m[1].length === 3 ? [...m[1]].map((c) => c + c).join('') : m[1];
  return `#${h.toLowerCase()}`;
}

/** The palette colour for a position in server order; wraps, and tolerates negative or NaN. */
export function defaultColor(index: number): string {
  const n = Number.isFinite(index) ? Math.trunc(index) : 0;
  return PALETTE[((n % PALETTE.length) + PALETTE.length) % PALETTE.length];
}

/** Average colour of the header gradient (HEX 56%, #0b0d12 44%) as relative luminance, 0..1. */
function avgLuminance(h: string): number {
  const lin = (i: number) => {
    const c = (parseInt(h.slice(1 + i * 2, 3 + i * 2), 16) * 0.56 + BASE[i] * 0.44) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(0) + 0.7152 * lin(1) + 0.0722 * lin(2);
}

// The mix with the dark base keeps this under 0.3, so the cut sits lower: the palette's blue, violet, pink,
// red and orange stay white (0.10 to 0.13) and yellow, green and teal go dark (0.16 to 0.19).
const DARK_TEXT_ABOVE = 0.15;

/** Text colour that reads on `headerBg(hex)`. Invalid input gives white. */
export function onColor(hex: string): '#ffffff' | '#0b0d12' {
  const h = normalizeHex(hex);
  return h && avgLuminance(h) > DARK_TEXT_ABOVE ? '#0b0d12' : '#ffffff';
}

/** Header gradient for a machine colour: darkening toward the base under white text, lightening under dark text so it reads across the whole header. */
export function headerBg(hex: string): string {
  const h = normalizeHex(hex) ?? PALETTE[0];
  return onColor(h) === '#ffffff'
    ? `linear-gradient(135deg, color-mix(in srgb, ${h} 78%, #0b0d12), color-mix(in srgb, ${h} 34%, #0b0d12))`
    : `linear-gradient(135deg, ${h}, color-mix(in srgb, ${h} 60%, #ffffff))`;
}
