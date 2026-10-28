import { describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import LatencyLayers from './LatencyLayers.svelte';

const samples = [10, 12, null, 30].map((d, i) => ({ t: i * 3000, direct: d, hub: 20 + i }));

describe('LatencyLayers', () => {
  it('an image with the latest numbers; gaps get a tick; the legend toggles a layer', async () => {
    await render(LatencyLayers, { props: { samples } });
    await expect.element(page.getByRole('img')).toHaveAttribute('aria-label', 'Latency: you to machine 30 ms, hub to machine 23 ms, over the last 4 pings');
    expect(document.querySelectorAll('.loss.direct')).toHaveLength(1);
    const btn = page.getByRole('button', { name: 'You → machine' });
    await expect.element(btn).toHaveAttribute('aria-pressed', 'true');
    await userEvent.click(btn);
    await expect.element(btn).toHaveAttribute('aria-pressed', 'false');
    expect(document.querySelectorAll('.edge.direct')).toHaveLength(0);
  });

  it('empty and all-lost read as states, not blank charts', async () => {
    const r = await render(LatencyLayers, { props: { samples: [] } });
    await expect.element(page.getByText('Waiting for the first ping…')).toBeInTheDocument();
    await r.rerender({ samples: [{ t: 0, direct: null, hub: null }] });
    await expect.element(page.getByText('No answer')).toBeInTheDocument();
  });
});
