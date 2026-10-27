import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api } from '../../state/api';
import { notes, type GameNote, type Tag } from '../../state/notes';
import GameNotes from './GameNotes.svelte';

const coop: Tag = { name: 'Co-op night', color: 'teal', symbol: 'users', note: 'Needs 4 controllers' };
const echo = () => vi.spyOn(api, 'putGameNote').mockImplementation(async (_id: number, b: GameNote) => ({ note: b.note.trimEnd(), tags: b.tags }));

afterEach(() => {
  vi.restoreAllMocks();
  notes.set({ tags: {}, games: {} });
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
    expect(body).toEqual({ name: 'Backlog', color: 'purple', symbol: 'trophy', note: '' });
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
  });

  it('notes render as text, never as HTML', async () => {
    const evil = '<img src=x onerror=alert(1)>';
    notes.set({ tags: { coop: { ...coop, note: evil } }, games: { '620': { note: evil, tags: ['coop'] } } });
    await render(GameNotes, { props: { appid: 620, name: 'Portal 2' } });
    await expect.element(page.getByRole('textbox', { name: 'Note for Portal 2' })).toHaveValue(evil);
    await expect.element(page.getByText(evil)).toBeInTheDocument();
    expect(document.querySelectorAll('img')).toHaveLength(0);
  });
});
