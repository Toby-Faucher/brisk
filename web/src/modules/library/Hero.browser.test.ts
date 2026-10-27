import { describe, expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { Game } from '../../state/api';
import Hero from './Hero.svelte';

const game: Game = { appid: 620, name: 'Portal 2', installed: true, sizeBytes: null, lastPlayed: 1700000000, playtimeMin: 600, playtime2wkMin: 0, cover: '', hero: 'data:image/gif;base64,R0lGODlhAQABAAAAACw=', logo: '', running: false };

describe('Hero art', () => {
  it('hero falls back to the hue wash when the art fails', async () => {
    const { container } = await render(Hero, { props: { game, busy: false, onpick: () => {} } });
    const hero = container.querySelector('.hero')!;
    await expect.poll(() => hero.querySelector('.art')).not.toBeNull();
    hero.querySelector('img[hidden]')!.dispatchEvent(new Event('error'));
    await expect.poll(() => hero.querySelector('.art')).toBeNull();
    expect(hero.getAttribute('style')).toContain('radial-gradient');
    expect(hero.querySelector('h3')!.textContent).toContain('Portal 2');
  });
});
