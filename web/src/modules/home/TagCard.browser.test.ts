import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { Game } from '../../state/api';
import { games } from '../../state/games';
import { notes } from '../../state/notes';
import TagCard from './TagCard.svelte';

const g = (appid: number, name: string, o: Partial<Game> = {}): Game => ({ appid, name, installed: true, sizeBytes: null, lastPlayed: null, playtimeMin: 0, playtime2wkMin: 0, cover: `/c/${appid}.jpg`, hero: '', logo: '', achUnlocked: null, achTotal: null, updated: false, update: null, controller: null, coop: false, running: false, ...o });
const tag = (o = {}) => ({ name: 'Co-op night', color: 'teal' as const, symbol: 'users', note: 'Four controllers\nand snacks', pinned: true, ...o });

beforeEach(() => notes.set({ tags: {}, games: {} }));
afterEach(() => notes.set({ tags: {}, games: {} }));

describe('TagCard', () => {
  it('shows count, note and at most 8 covers, installed first', async () => {
    const list = Array.from({ length: 10 }, (_, i) => g(i + 1, `Game ${String(i + 1).padStart(2, '0')}`, { installed: i >= 2 }));
    games.set({ games: list, ownedAvailable: true });
    notes.set({ tags: { co: tag() }, games: Object.fromEntries(list.map((x) => [x.appid, { note: '', tags: ['co'] }])) });
    await render(TagCard, { tagId: 'co' });
    const link = page.getByRole('link', { name: 'Co-op night, 10 games' });
    await expect.element(link).toHaveAttribute('href', '/library?tag=co');
    await expect.element(page.getByText(/Four controllers/)).toBeInTheDocument();
    const imgs = [...document.querySelectorAll('img')];
    expect(imgs).toHaveLength(8);
    expect(imgs.every((i) => i.getAttribute('alt') === '' && i.loading === 'lazy')).toBe(true);
    expect(imgs[0].getAttribute('src')).toBe('/c/3.jpg'); // installed first, then by name
  });

  it('empty tag says No games yet', async () => {
    games.set({ games: [g(1, 'A')], ownedAvailable: true });
    notes.set({ tags: { co: tag({ note: '' }) }, games: {} });
    await render(TagCard, { tagId: 'co' });
    await expect.element(page.getByText('No games yet')).toBeInTheDocument();
    await expect.element(page.getByRole('link', { name: 'Co-op night, 0 games' })).toBeInTheDocument();
  });

  it('renders nothing for an unknown or unpinned tag', async () => {
    notes.set({ tags: { off: tag({ pinned: false }) }, games: {} });
    const a = await render(TagCard, { tagId: 'nope' });
    expect(document.querySelector('.card')).toBeNull();
    a.unmount();
    await render(TagCard, { tagId: 'off' });
    expect(document.querySelector('.card')).toBeNull();
  });
});
