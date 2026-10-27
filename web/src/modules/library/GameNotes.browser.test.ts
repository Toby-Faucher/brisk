import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, ApiError } from '../../state/api';
import { gameNoteSaver, notes, notesLoaded, type GameNote, type Tag } from '../../state/notes';
import GameNotes from './GameNotes.svelte';

const coop: Tag = { name: 'Co-op night', color: 'teal', symbol: 'users', note: 'Needs 4 controllers' };
const echo = () => vi.spyOn(api, 'putGameNote').mockImplementation(async (_id: number, b: GameNote) => ({ note: b.note.trimEnd(), tags: b.tags }));

beforeEach(() => notesLoaded.set(true));
afterEach(async () => {
  // nothing pending carries over to the next test
  vi.spyOn(api, 'putGameNote').mockImplementation(async (_id: number, b: GameNote) => b);
  await gameNoteSaver.retry();
  vi.restoreAllMocks();
  notes.set({ tags: {}, games: {} });
  gameNoteSaver.status.set({});
});

describe('GameNotes', () => {
  it('typing saves once and shows Saved', async () => {
    const put = echo();
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await userEvent.type(page.getByRole('textbox', { name: 'Note for Portal 2' }), 'Bring snacks');
    await expect.element(page.getByText('Saved', { exact: true }), { timeout: 3000 }).toBeInTheDocument();
    expect(put).toHaveBeenCalledExactlyOnceWith(620, { note: 'Bring snacks', tags: [] });
  });

  it('a failed save shows Not saved and keeps the text', async () => {
    vi.spyOn(api, 'putGameNote').mockRejectedValue(new Error('offline'));
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    const box = page.getByRole('textbox', { name: 'Note for Portal 2' });
    await userEvent.type(box, 'Keep me');
    await userEvent.click(document.body); // blur saves at once
    await expect.element(page.getByText(/Not saved/)).toBeInTheDocument();
    await expect.element(box).toHaveValue('Keep me');
  });

  it('adding a tag chip saves the game with its id', async () => {
    notes.set({ tags: { coop }, games: {} });
    const put = echo();
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await userEvent.click(page.getByRole('button', { name: 'Add a tag' }));
    await userEvent.click(page.getByRole('button', { name: 'Co-op night' }));
    expect(put).toHaveBeenCalledExactlyOnceWith(620, { note: '', tags: ['coop'] });
    await expect.element(page.getByRole('button', { name: 'Edit tag Co-op night' })).toBeInTheDocument();
  });

  it('creating a tag calls putTag, then putGameNote with the new id', async () => {
    const putTag = vi.spyOn(api, 'putTag').mockImplementation(async (_id: string, t: Tag) => t);
    const put = echo();
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await userEvent.click(page.getByRole('button', { name: 'Add a tag' }));
    await userEvent.click(page.getByRole('button', { name: 'New tag…' }));
    await userEvent.type(page.getByRole('textbox', { name: 'Name' }), 'Backlog');
    await userEvent.click(page.getByRole('radio', { name: 'purple' }));
    await userEvent.click(page.getByRole('radio', { name: 'trophy' }));
    await userEvent.click(page.getByRole('button', { name: 'Save tag' }));
    await vi.waitFor(() => expect(put).toHaveBeenCalled());
    const [id, body] = putTag.mock.calls[0];
    expect(id).toMatch(/^[a-z0-9-]{1,16}$/);
    expect(body).toEqual({ name: 'Backlog', color: 'purple', symbol: 'trophy', note: '', pinned: false });
    expect(put).toHaveBeenCalledExactlyOnceWith(620, { note: '', tags: [id] });
    expect(putTag.mock.invocationCallOrder[0]).toBeLessThan(put.mock.invocationCallOrder[0]);
  });

  it('deleting a used tag asks first and says how many games use it', async () => {
    notes.set({ tags: { coop }, games: { '620': { note: '', tags: ['coop'] }, '400': { note: 'x', tags: ['coop'] } } });
    const del = vi.spyOn(api, 'deleteTag').mockResolvedValue(undefined);
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await userEvent.click(page.getByRole('button', { name: 'Edit tag Co-op night' }));
    await userEvent.click(page.getByRole('button', { name: 'Delete tag' }));
    await expect.element(page.getByText('It is on 2 games. Their notes stay.')).toBeInTheDocument();
    expect(del).not.toHaveBeenCalled();
    await userEvent.click(page.getByRole('button', { name: 'Delete', exact: true }));
    expect(del).toHaveBeenCalledExactlyOnceWith('coop');
    await expect.element(page.getByRole('button', { name: 'Edit tag Co-op night' })).not.toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'Add a tag' })).toHaveFocus();
  });

  it('notes render as text, never as HTML', async () => {
    const evil = '<img src=x onerror=alert(1)>';
    notes.set({ tags: { coop: { ...coop, note: evil } }, games: { '620': { note: evil, tags: ['coop'] } } });
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await expect.element(page.getByRole('textbox', { name: 'Note for Portal 2' })).toHaveValue(evil);
    await expect.element(page.getByText(evil)).toBeInTheDocument();
    expect(document.querySelectorAll('img')).toHaveLength(0);
  });

  it('waits for the notes to load: nothing to edit and no PUT before that', async () => {
    notesLoaded.set(false);
    let answer!: (n: { tags: Record<string, Tag>; games: Record<string, GameNote> }) => void;
    vi.spyOn(api, 'notes').mockReturnValue(new Promise((r) => (answer = r)));
    const put = echo();
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    const box = page.getByRole('textbox', { name: 'Note for Portal 2' });
    await expect.element(box).toBeDisabled();
    await expect.element(page.getByRole('button', { name: 'Add a tag' })).toBeDisabled();
    await expect.element(page.getByText('Loading notes…')).toBeInTheDocument();
    answer({ tags: { coop }, games: { '620': { note: 'From hostd', tags: ['coop'] } } });
    await expect.element(box).toBeEnabled();
    await expect.element(box).toHaveValue('From hostd');
    expect(put).not.toHaveBeenCalled();
  });

  it('says when the notes could not load', async () => {
    notesLoaded.set(false);
    vi.spyOn(api, 'notes').mockRejectedValue(new Error('down'));
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await expect.element(page.getByText('Could not load notes, retrying')).toBeInTheDocument();
    await expect.element(page.getByRole('textbox', { name: 'Note for Portal 2' })).toBeDisabled();
  });

  it('a refused save says why and is not retried on focus', async () => {
    const put = vi.spyOn(api, 'putGameNote').mockRejectedValue(new ApiError('too many notes or tags', 409));
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await userEvent.type(page.getByRole('textbox', { name: 'Note for Portal 2' }), 'x');
    await userEvent.click(document.body);
    await expect.element(page.getByText('Not saved: too many notes or tags')).toBeInTheDocument();
    window.dispatchEvent(new Event('focus'));
    await new Promise((r) => setTimeout(r, 50));
    expect(put).toHaveBeenCalledTimes(1);
  });

  it('shows no save status of another game', async () => {
    gameNoteSaver.status.set({ 400: { st: 'failed' } });
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await expect.element(page.getByRole('textbox', { name: 'Note for Portal 2' })).toBeInTheDocument();
    expect(page.getByText(/Not saved/).elements()).toHaveLength(0);
  });
});
