import { describe, expect, it } from 'vitest';
import { buildBackup, MAX_BACKUP_BYTES, parseBackup } from './notesBackup';
import type { Notes } from './notes';

const notes: Notes = {
  tags: { coop: { name: 'Co-op night', color: 'teal', symbol: 'users', note: 'Needs 4 pads', pinned: true }, 'to-play': { name: 'To play', color: 'blue', symbol: 'star', note: '' } },
  games: { 620: { note: 'Bring snacks', tags: ['coop'] }, 400: { note: '', tags: ['to-play', 'coop'] } },
};
const file = (over: Record<string, unknown> = {}) => JSON.stringify({ format: 'brisk-notes', version: 1, exported: 'x', ...notes, ...over });
const err = (text: string) => {
  const r = parseBackup(text);
  if (r.ok) throw new Error('expected a rejection');
  return r.error;
};
const tag = (over: Record<string, unknown> = {}) => ({ name: 'A', color: 'red', symbol: 'star', note: '', ...over });

describe('buildBackup', () => {
  it('has the format header, the ISO date and the notes as given', () => {
    const b = buildBackup(notes, new Date('2026-10-27T12:34:56Z'));
    expect(b).toEqual({ format: 'brisk-notes', version: 1, exported: '2026-10-27T12:34:56.000Z', tags: notes.tags, games: notes.games });
  });
});

describe('parseBackup', () => {
  it('round trips', () => {
    const r = parseBackup(JSON.stringify(buildBackup(notes, new Date())));
    expect(r).toEqual({ ok: true, tags: notes.tags, games: notes.games });
  });
  it('ignores unknown fields and drops unknown tag ids and repeats', () => {
    const r = parseBackup(file({ extra: 1, tags: { a: tag({ x: 1 }) }, games: { 5: { note: 'n', tags: ['a', 'zzz', 'a'], more: true } } }));
    expect(r).toEqual({ ok: true, tags: { a: tag() }, games: { 5: { note: 'n', tags: ['a'] } } });
  });
  it('skips games left with no note and no tags', () => {
    const r = parseBackup(file({ tags: {}, games: { 5: { note: ' ', tags: ['gone'] } } }));
    expect(r).toEqual({ ok: true, tags: {}, games: {} });
  });
  it('accepts a file with no tags and no games', () => {
    expect(parseBackup(JSON.stringify({ format: 'brisk-notes', version: 1 }))).toEqual({ ok: true, tags: {}, games: {} });
  });

  it.each([
    ['not JSON', '{nope', /JSON/],
    ['an array', '[]', /brisk notes/],
    ['wrong format', file({ format: 'other' }), /brisk notes/],
    ['wrong version', file({ version: 2 }), /version/],
    ['tags not an object', file({ tags: [] }), /tags/],
    ['games not an object', file({ games: 3 }), /games/],
    ['bad tag id chars', file({ tags: { 'Bad Id': tag() }, games: {} }), /tag id/i],
    ['tag id too long', file({ tags: { ['a'.repeat(17)]: tag() }, games: {} }), /tag id/i],
    ['auto- tag id', file({ tags: { 'auto-x': tag() }, games: {} }), /auto-/],
    ['tag not an object', file({ tags: { a: 'x' }, games: {} }), /Tag a/],
    ['empty name', file({ tags: { a: tag({ name: '  ' }) }, games: {} }), /name/],
    ['long name', file({ tags: { a: tag({ name: 'x'.repeat(25) }) }, games: {} }), /name/],
    ['bad color', file({ tags: { a: tag({ color: 'mauve' }) }, games: {} }), /color/],
    ['bad symbol', file({ tags: { a: tag({ symbol: 'Star!' }) }, games: {} }), /symbol/],
    ['empty symbol', file({ tags: { a: tag({ symbol: '' }) }, games: {} }), /symbol/],
    ['long tag note', file({ tags: { a: tag({ note: 'x'.repeat(501) }) }, games: {} }), /note/],
    ['note not a string', file({ tags: { a: tag({ note: 4 }) }, games: {} }), /note/],
    ['pinned not a boolean', file({ tags: { a: tag({ pinned: 'yes' }) }, games: {} }), /pinned/],
    ['appid zero', file({ tags: {}, games: { 0: { note: 'n', tags: [] } } }), /appid/],
    ['appid not a number', file({ tags: {}, games: { abc: { note: 'n', tags: [] } } }), /appid/],
    ['appid with a sign', file({ tags: {}, games: { '-5': { note: 'n', tags: [] } } }), /appid/],
    ['appid beyond safe integers', file({ tags: {}, games: { '99999999999999999999': { note: 'n', tags: [] } } }), /appid/],
    ['game not an object', file({ tags: {}, games: { 5: null } }), /Game 5/],
    ['long game note', file({ tags: {}, games: { 5: { note: 'x'.repeat(4001), tags: [] } } }), /note/],
    ['game tags not an array', file({ tags: {}, games: { 5: { note: 'n', tags: 'a' } } }), /tags/],
    ['game tag id not a string', file({ tags: {}, games: { 5: { note: 'n', tags: [1] } } }), /tags/],
  ])('rejects %s', (_n, text, re) => expect(err(text)).toMatch(re));

  it('counts characters, not UTF-16 units', () => {
    const emoji = '😀';
    expect(parseBackup(file({ tags: { a: tag({ name: emoji.repeat(24), note: emoji.repeat(500) }) }, games: { 5: { note: emoji.repeat(4000), tags: [] } } })).ok).toBe(true);
    expect(err(file({ tags: { a: tag({ name: emoji.repeat(25) }) }, games: {} }))).toMatch(/name/);
    expect(err(file({ tags: {}, games: { 5: { note: emoji.repeat(4001), tags: [] } } }))).toMatch(/note/);
  });
  it('does not count trailing whitespace, as the server does', () => {
    expect(parseBackup(file({ tags: { a: tag({ note: `${'x'.repeat(500)}   \n` }) }, games: {} })).ok).toBe(true);
  });
  it('trims the tag name', () => {
    const r = parseBackup(file({ tags: { a: tag({ name: '  Hi  ' }) }, games: {} }));
    expect(r.ok && r.tags.a.name).toBe('Hi');
  });

  it('limits: 64 tags, 2000 games, 16 tags per game', () => {
    const tags = (n: number) => Object.fromEntries(Array.from({ length: n }, (_, i) => [`t${i}`, tag()]));
    const games = (n: number) => Object.fromEntries(Array.from({ length: n }, (_, i) => [i + 1, { note: 'n', tags: [] }]));
    expect(parseBackup(file({ tags: tags(64), games: {} })).ok).toBe(true);
    expect(err(file({ tags: tags(65), games: {} }))).toMatch(/64 tags/);
    expect(parseBackup(file({ tags: {}, games: games(2000) })).ok).toBe(true);
    expect(err(file({ tags: {}, games: games(2001) }))).toMatch(/2000 games/);
    const ids = Array.from({ length: 17 }, (_, i) => `t${i}`);
    expect(parseBackup(file({ tags: tags(17), games: { 5: { note: '', tags: ids.slice(0, 16) } } })).ok).toBe(true);
    expect(err(file({ tags: tags(17), games: { 5: { note: '', tags: ids } } }))).toMatch(/16 tags/);
  });
  it('rejects text over 2 MiB', () => {
    expect(err(' '.repeat(MAX_BACKUP_BYTES + 1))).toMatch(/2 MiB/);
  });
});
