import { describe, expect, it } from 'vitest';
import oldFx from '../../../crates/brisk-hostd/tests/fixtures/stage1/games-old.json';
import newFx from '../../../crates/brisk-hostd/tests/fixtures/stage1/games-new.json';
import type { Game } from './api';
import {
  cssSafe,
  findGame,
  gameCaps,
  gameKey,
  gameSegment,
  keyForms,
  pick,
  rowKey,
  sameKey,
  toIdForm,
  toNumberForm,
  without,
} from './gameId';

const oldGames = oldFx.games as unknown as Game[];
const newGames = newFx.games as unknown as Game[];
const [oldPortal, oldMk] = oldGames;
const [newPortal, newMk] = newGames;

describe('gameKey', () => {
  it('is the id, else the number as text', () => {
    expect(gameKey(newPortal)).toBe('steam:620');
    expect(gameKey(newMk)).toBe('emu:dolphin:mario-kart-wii');
    expect(gameKey(oldPortal)).toBe('620');
    expect(gameKey(oldMk)).toBe('3907440760');
  });
});

describe('gameCaps', () => {
  it('reads caps from a new hostd game, whatever the number', () => {
    expect(gameCaps(newPortal)).toEqual(newFx.games[0].caps);
    expect(gameCaps(newMk).controls).toBe(true);
    expect(gameCaps({ ...newMk, appid: 5 }).controls).toBe(true);
    expect(gameCaps({ ...newPortal, appid: 0xe0000009 }).install).toBe(true);
  });
  it('derives caps for an old hostd game from the emulator field or the old range', () => {
    expect(gameCaps(oldPortal)).toMatchObject({ install: true, controls: false, achievements: true, screenshots: true });
    expect(gameCaps(oldMk)).toMatchObject({ install: false, controls: true, achievements: false, screenshots: false });
    expect(gameCaps({ appid: 0xe0000000 }).controls).toBe(true); // the old range alone
    expect(gameCaps({ appid: 0xdfffffff }).controls).toBe(false);
    expect(gameCaps({ appid: 5, emulator: 'dolphin' }).controls).toBe(true); // the field alone
    expect(gameCaps(oldPortal).launchOptions).toBe(false);
  });
});

describe('key forms', () => {
  it('converts between the id and the number form for Steam only', () => {
    expect(toIdForm('620')).toBe('steam:620');
    expect(toIdForm('steam:620')).toBe('steam:620');
    expect(toIdForm('3907440760')).toBe('3907440760'); // an old emulator number has no id without the library
    expect(toIdForm('emu:dolphin:x')).toBe('emu:dolphin:x');
    expect(toNumberForm('steam:620')).toBe('620');
    expect(toNumberForm('620')).toBe('620');
    expect(toNumberForm('emu:dolphin:x')).toBe('emu:dolphin:x');
  });
  it('sameKey matches the two Steam forms and nothing else', () => {
    expect(sameKey('620', 'steam:620')).toBe(true);
    expect(sameKey('620', 'steam:621')).toBe(false);
    expect(sameKey('emu:dolphin:x', 'emu:dolphin:x')).toBe(true);
    expect(sameKey('emu:dolphin:x', 'emu:dolphin:y')).toBe(false);
  });
  it('findGame finds by either form', () => {
    expect(findGame(newGames, '620')).toBe(newPortal);
    expect(findGame(oldGames, 'steam:620')).toBe(oldPortal);
    expect(findGame(newGames, 'emu:dolphin:mario-kart-wii')).toBe(newMk);
    expect(findGame(newGames, 'nope')).toBeUndefined();
    expect(findGame(newGames, null)).toBeUndefined();
  });
  it('rowKey shows a session or stream answer by gameId, else appid, null for the Desktop', () => {
    expect(rowKey({ gameId: 'steam:620', appid: 620 })).toBe('steam:620');
    expect(rowKey({ appid: 620 })).toBe('620');
    expect(rowKey({ gameId: null, appid: null })).toBeNull();
    expect(rowKey({})).toBeNull();
  });
});

describe('notes and prefs maps with both key forms', () => {
  it('pick reads the id, the number, and an emulator entry not remapped yet', () => {
    expect(pick({ 'steam:620': 'a' }, newPortal)).toBe('a');
    expect(pick({ '620': 'old' }, newPortal)).toBe('old'); // a hub that is still old
    expect(pick({ 'steam:620': 'new' }, oldPortal)).toBe('new'); // an old machine under a new hub
    expect(pick({ '3907440760': 'n' }, newMk)).toBe('n'); // under the legacy number until the remap runs
    expect(pick({ 'emu:dolphin:mario-kart-wii': 'id', '3907440760': 'n' }, newMk)).toBe('id'); // the id wins
    expect(pick({ '621': 'x' }, newPortal)).toBeUndefined();
    expect(pick({ '620': 'old' }, '620')).toBe('old');
    expect(pick({ 'steam:620': 'new' }, '620')).toBe('new');
  });
  it('without drops every form of the game', () => {
    expect(without({ '620': 1, 'steam:620': 2, '730': 3 }, newPortal)).toEqual({ '730': 3 });
    expect(keyForms(newMk)).toEqual(['emu:dolphin:mario-kart-wii', '3907440760']);
  });
});

describe('url encoding', () => {
  it('encodes the colon once, in one path segment', () => {
    expect(gameSegment('steam:620')).toBe('steam%3A620');
    expect(gameSegment('emu:dolphin:mario-kart-wii')).toBe('emu%3Adolphin%3Amario-kart-wii');
    expect(gameSegment('emu:dolphin:a~2.b_c')).toBe('emu%3Adolphin%3Aa~2.b_c');
    expect(decodeURIComponent(gameSegment('emu:dolphin:x'))).toBe('emu:dolphin:x');
  });
  it('cssSafe makes a valid view-transition name part', () => {
    expect(cssSafe('emu:dolphin:mario-kart-wii')).toBe('emu_dolphin_mario-kart-wii');
  });
});
