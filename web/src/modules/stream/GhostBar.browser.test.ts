import { describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import GhostBar from './GhostBar.svelte';
import Toasts, { toast } from '../../shell/Toasts.svelte';

const props = (extra = {}) => ({
  host: 'bawn',
  rttMs: 5,
  preset: 'balanced' as const,
  codec: 'h264' as const,
  captured: false,
  clipDenied: false,
  upload: null,
  onfiles: vi.fn(),
  onvolume: vi.fn(),
  oncapture: vi.fn(),
  onquality: vi.fn(),
  onforget: vi.fn(),
  onfullscreen: vi.fn(),
  ondisconnect: vi.fn(),
  onshot: vi.fn(),
  onreplay: vi.fn(),
  ...extra,
});

describe('screenshot button', () => {
  it('calls onshot, and the tip says when the game keeps the shot', async () => {
    const p = props();
    await render(GhostBar, { props: p });
    (page.getByRole('button', { name: 'Screenshot' }).element() as HTMLElement).click(); // the bar is parked off screen
    expect(p.onshot).toHaveBeenCalledTimes(1);
    await expect.element(page.getByText('Screenshot · Ctrl+Alt+S')).toBeInTheDocument();
  });
  it('an emulator game says it keeps the shot with the game', async () => {
    await render(GhostBar, { props: props({ storesShots: true }) });
    await expect.element(page.getByText('Screenshot · kept with the game · Ctrl+Alt+S')).toBeInTheDocument();
  });
});

describe('toast with a thumbnail', () => {
  it('shows a decorative image beside the text in the live region', async () => {
    await render(Toasts);
    toast('Saved x.png · kept with Mario', { thumb: URL.createObjectURL(new Blob([new Uint8Array([1])], { type: 'image/png' })) });
    await expect.element(page.getByText('Saved x.png · kept with Mario')).toBeInTheDocument();
    const img = document.querySelector('.toasts img.thumb') as HTMLImageElement;
    expect(img.getAttribute('alt')).toBe('');
    expect(img.src.startsWith('blob:')).toBe(true);
    expect(document.querySelector('.toasts')?.getAttribute('aria-live')).toBe('polite');
  });
});
