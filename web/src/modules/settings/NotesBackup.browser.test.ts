import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, ApiError } from '../../state/api';
import { notes, type Notes } from '../../state/notes';
import NotesBackup from './NotesBackup.svelte';

const data: Notes = {
  tags: {
    coop: { name: 'Co-op night', color: 'teal', symbol: 'users', note: 'Needs 4 pads' },
    backlog: { name: 'Backlog', color: 'blue', symbol: 'star', note: '' },
  },
  games: { 620: { note: 'Bring snacks', tags: ['coop'] }, 400: { note: '', tags: ['backlog'] } },
};
const backup = JSON.stringify({ format: 'brisk-notes', version: 1, exported: '2026-10-27T00:00:00.000Z', ...data });
const upload = (text: string, name = 'notes.json') =>
  userEvent.upload(page.getByLabelText('Restore from file'), new File([text], name, { type: 'application/json' }));
const none: Notes = { tags: {}, games: {} };
const spies = (live: Notes = none) => ({
  putTag: vi.spyOn(api, 'putTag').mockImplementation(async (_i, t) => t),
  putGame: vi.spyOn(api, 'putGameNote').mockImplementation(async (_i, g) => g),
  get: vi.spyOn(api, 'notes').mockResolvedValue(live),
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  notes.set({ tags: {}, games: {} });
});

describe('NotesBackup', () => {
  it('Download sends a dated JSON file with the notes', async () => {
    spies(data);
    let blob: Blob | undefined;
    vi.spyOn(URL, 'createObjectURL').mockImplementation((b) => ((blob = b as Blob), 'blob:x'));
    const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    vi.useFakeTimers({ toFake: ['Date'], now: new Date(2026, 9, 27, 23, 59) }); // late evening local time; timers stay real
    let name = '';
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      name = this.download;
    });
    await render(NotesBackup);
    await userEvent.click(page.getByRole('button', { name: 'Download notes and tags' }));
    expect(revoke).not.toHaveBeenCalled(); // not before the download has started
    await vi.waitFor(() => expect(revoke).toHaveBeenCalledWith('blob:x'), { timeout: 3000 });
    expect(name).toBe('brisk-notes-2026-10-27.json');
    const j = JSON.parse(await blob!.text());
    expect(j).toMatchObject({ format: 'brisk-notes', version: 1, ...data });
    expect(typeof j.exported).toBe('string');
  });

  it('a bad file shows the error and sends nothing', async () => {
    const s = spies();
    await render(NotesBackup);
    await upload('{"format":"brisk-notes","version":1,"tags":{"auto-x":{}}}');
    await expect.poll(() => page.getByRole('alert').element().textContent).toContain('auto-');
    expect(page.getByRole('dialog').query()).toBeNull();
    expect(s.putTag).not.toHaveBeenCalled();
    expect(s.putGame).not.toHaveBeenCalled();
  });

  it('a file over 2 MiB is refused', async () => {
    const s = spies();
    await render(NotesBackup);
    await upload(' '.repeat(2 * 1024 * 1024 + 1));
    await expect.poll(() => page.getByRole('alert').element().textContent).toContain('2 MiB');
    expect(s.putTag).not.toHaveBeenCalled();
  });

  it('a good file asks first, with the counts and the replace-only wording', async () => {
    const s = spies();
    await render(NotesBackup);
    await upload(backup);
    const dialog = page.getByRole('dialog', { name: 'Restore notes and tags?' });
    await expect.element(dialog).toBeInTheDocument();
    await expect.poll(() => dialog.element().textContent).toContain('2 tags and 2 games');
    await expect.poll(() => dialog.element().textContent).toContain('replaces the one here, note and tags included');
    await expect.poll(() => dialog.element().textContent).toContain('that are not in the file are kept');
    expect(dialog.element().textContent).not.toContain('already here'); // nothing overlaps on an empty host
    expect(s.putTag).not.toHaveBeenCalled();
    await userEvent.click(page.getByRole('button', { name: 'Cancel' }));
    await expect.element(dialog).not.toBeInTheDocument();
    expect(s.putTag).not.toHaveBeenCalled();
  });

  it('Restore sends the tags, then the games, in order, then refreshes the store', async () => {
    const s = spies();
    await render(NotesBackup);
    await upload(backup);
    await userEvent.click(page.getByRole('button', { name: 'Restore' }));
    await expect.poll(() => page.getByRole('status').element().textContent).toContain('Imported 2 tags and 2 games.');
    expect(s.putTag.mock.calls.map((c) => c[0])).toEqual(['coop', 'backlog']);
    expect(s.putGame.mock.calls.map((c) => c[0])).toEqual(['400', '620']); // JS orders integer keys ascending
    expect(s.putGame).toHaveBeenCalledWith('620', data.games[620]);
    expect(s.putTag.mock.invocationCallOrder.at(-1)!).toBeLessThan(s.putGame.mock.invocationCallOrder[0]);
    await vi.waitFor(() => expect(s.get).toHaveBeenCalled());
  });

  it('a failed PUT stops and says how many went in', async () => {
    const s = spies();
    s.putGame.mockRejectedValue(new ApiError('too many games', 409));
    await render(NotesBackup);
    await upload(backup);
    await userEvent.click(page.getByRole('button', { name: 'Restore' }));
    await expect.poll(() => page.getByRole('alert').element().textContent).toContain('Stopped after 2 of 4');
    await expect.poll(() => page.getByRole('alert').element().textContent).toContain('too many games');
    expect(s.putGame).toHaveBeenCalledTimes(1);
    await expect.poll(() => document.activeElement === page.getByRole('alert').element()).toBe(true);
    await vi.waitFor(() => expect(s.get).toHaveBeenCalled());
  });

  it('the dialog says how many games replace one that is here', async () => {
    spies({ tags: {}, games: { 620: { note: 'newer', tags: [] } } });
    await render(NotesBackup);
    await upload(backup);
    const dialog = page.getByRole('dialog', { name: 'Restore notes and tags?' });
    await expect.poll(() => dialog.element().textContent).toContain('1 of the games in the file replace a game that is already here');
  });

  it('refuses a restore that would pass 64 tags, and sends nothing', async () => {
    const live: Notes = { tags: Object.fromEntries(Array.from({ length: 64 }, (_, i) => [`t${i}`, data.tags.coop])), games: {} };
    const s = spies(live);
    await render(NotesBackup);
    await upload(backup);
    await expect.poll(() => page.getByRole('alert').element().textContent).toContain('limit is 64');
    expect(page.getByRole('dialog').query()).toBeNull();
    expect(s.putTag).not.toHaveBeenCalled();
  });

  it('an empty file says nothing to import and opens no dialog', async () => {
    const s = spies();
    await render(NotesBackup);
    await upload('{"format":"brisk-notes","version":1}');
    await expect.poll(() => page.getByRole('status').element().textContent).toContain('Nothing to import');
    expect(page.getByRole('dialog').query()).toBeNull();
    expect(s.get).not.toHaveBeenCalled();
  });

  it('the status region exists before any text, and focus lands on it after Restore', async () => {
    spies();
    await render(NotesBackup);
    const status = page.getByRole('status');
    expect(status.element().textContent).toBe('');
    await upload(backup);
    await userEvent.click(page.getByRole('button', { name: 'Restore' }));
    await expect.poll(() => status.element().textContent).toContain('Imported 2 tags and 2 games.');
    await expect.poll(() => document.activeElement === status.element()).toBe(true);
  });

  it('Stop ends the restore between PUTs and reports the count', async () => {
    const s = spies();
    let release!: () => void;
    s.putTag.mockImplementation((_i, t) => new Promise((r) => (release = () => r(t))));
    await render(NotesBackup);
    await upload(backup);
    await userEvent.click(page.getByRole('button', { name: 'Restore' }));
    await userEvent.click(page.getByRole('button', { name: 'Stop restoring' }));
    release();
    await expect.poll(() => page.getByRole('alert').element().textContent).toContain('Stopped after 1 of 4');
    expect(s.putTag).toHaveBeenCalledTimes(1);
    expect(s.putGame).not.toHaveBeenCalled();
    expect(page.getByRole('button', { name: 'Stop restoring' }).query()).toBeNull();
    await expect.poll(() => document.activeElement === page.getByRole('alert').element()).toBe(true);
  });
});
