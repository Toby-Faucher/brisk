import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { Game } from '../../state/api';
import Hero from './Hero.svelte';

const game: Game = { appid: 620, name: 'Portal 2', installed: true, sizeBytes: null, lastPlayed: 1700000000, playtimeMin: 600, playtime2wkMin: 0, cover: '', hero: 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7', logo: '', running: false };

describe('Hero art', () => {
  it('hero falls back to the hue wash when the art fails', async () => {
    const { container } = await render(Hero, { props: { game, busy: false, onpick: () => {} } });
    const hero = container.querySelector('.hero')!;
    await expect.poll(() => hero.querySelector('.art')).not.toBeNull();
    hero.querySelector('img[hidden]')!.dispatchEvent(new Event('error'));
    await expect.poll(() => hero.querySelector('.art')).toBeNull();
    expect(hero.querySelector('img[hidden]')).toBeNull();
    expect(hero.querySelector('h3')!.textContent).toContain('Portal 2');
  });

  it('a loaded logo replaces the title visually, keeping the h3', async () => {
    const { container } = await render(Hero, { props: { game: { ...game, logo: game.hero }, busy: false, onpick: () => {} } });
    const hero = container.querySelector('.hero')!;
    const logo = hero.querySelector('img.logo')!;
    expect(logo.classList.contains('on')).toBe(false);
    logo.dispatchEvent(new Event('load'));
    await expect.poll(() => logo.classList.contains('on')).toBe(true);
    expect(hero.querySelector('h3')!.classList.contains('vh')).toBe(true);
    expect(hero.querySelector('h3')!.textContent).toContain('Portal 2');
  });
});
