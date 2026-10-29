import type { Game } from '../state/api';

/** An installed Steam game with every other field empty; tests override only what they check. */
export const testGame = (o: Partial<Game> = {}): Game => ({
  appid: 620,
  name: 'Portal 2',
  installed: true,
  sizeBytes: null,
  lastPlayed: null,
  playtimeMin: 0,
  playtime2wkMin: 0,
  cover: '',
  hero: '',
  logo: '',
  achUnlocked: null,
  achTotal: null,
  updated: false,
  update: null,
  controller: null,
  coop: false,
  running: false,
  ...o,
});
