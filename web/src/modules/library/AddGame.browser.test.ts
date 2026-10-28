import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import { api, ApiError, type UploadApi, type UploadDone } from '../../state/api';
import AddGame from './AddGame.svelte';

const done: UploadDone = { system: 'wii', registered: true, duplicate: false, game: null };
/** An upload api whose PUTs wait until the test lets them through. */
function fake(o: { createFail?: unknown; finish?: UploadDone } = {}) {
  const gates: (() => void)[] = [];
  const log: string[] = [];
  const a: UploadApi = {
    async create(name, size) {
      log.push(`create ${name}`);
      if (o.createFail) throw o.createFail;
      return { id: `u${log.length}`, chunk: 32 << 20, offset: 0 };
    },
    put: (id, offset, blob, signal) => {
      log.push(`put ${id} ${offset}`);
      return new Promise((r, no) => {
        gates.push(() => r({ offset: offset + (blob as Blob).size }));
        signal?.addEventListener('abort', () => no(new DOMException('aborted', 'AbortError')));
      });
    },
    status: async (id) => ({ id, name: '', offset: 0, size: 0, state: 'open' }),
    finish: async () => o.finish ?? done,
    abort: async (id) => void log.push(`abort ${id}`),
  };
  return { a, gates, log };
}
const pick = async (name: string, size = 3000) => {
  const input = document.querySelector('input[type=file]') as HTMLInputElement;
  const f = new File([new Uint8Array(size)], name);
  const dt = new DataTransfer();
  dt.items.add(f);
  input.files = dt.files;
  input.dispatchEvent(new Event('change', { bubbles: true }));
};
const open = async () => {
  await render(AddGame);
  await userEvent.click(page.getByRole('button', { name: 'Add game' }));
};

afterEach(() => vi.restoreAllMocks());

describe('Add game dialog', () => {
  it('opens with a way to choose files and a scan, and no machine picker on a single host', async () => {
    await open();
    await expect.element(page.getByRole('dialog', { name: 'Add game' })).toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'Choose files' })).toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'Scan games folder' })).toBeInTheDocument();
    expect(page.getByLabelText('Machine to send to').elements()).toHaveLength(0);
  });

  it('shows a progress bar while a file goes, then Added', async () => {
    const f = fake();
    vi.spyOn(api, 'uploadsFor').mockReturnValue(f.a);
    vi.spyOn(api, 'games').mockResolvedValue({ games: [], ownedAvailable: true });
    await open();
    await pick('Mario Kart Wii.iso');
    await expect.element(page.getByRole('progressbar', { name: 'Sending Mario Kart Wii.iso' })).toHaveAttribute('aria-valuenow', '0');
    await expect.poll(() => f.gates.length).toBe(1);
    f.gates[0]();
    await expect.element(page.getByText('Added', { exact: true })).toBeInTheDocument();
    await expect.element(page.getByRole('status').first()).toHaveTextContent('1 of 1 done');
  });

  it('says when a file is stored but nothing can play it', async () => {
    const f = fake({ finish: { system: 'gba', registered: false, duplicate: false, game: null } });
    vi.spyOn(api, 'uploadsFor').mockReturnValue(f.a);
    await open();
    await pick('Game.gba', 10);
    await expect.poll(() => f.gates.length).toBe(1);
    f.gates[0]();
    await expect.element(page.getByText('Added. Stored, but no emulator can play it yet.')).toBeInTheDocument();
  });

  it('shows a plain failure with Try again, and Cancel deletes the upload', async () => {
    const f = fake({ createFail: new ApiError('x', 507) });
    vi.spyOn(api, 'uploadsFor').mockReturnValue(f.a);
    await open();
    await pick('Big.iso');
    await expect.element(page.getByText('This machine keeps 5 GB free, so there is no room for this file.')).toBeInTheDocument();
    await expect.element(page.getByRole('button', { name: 'Try Big.iso again' })).toBeInTheDocument();
    await userEvent.click(page.getByRole('button', { name: 'Cancel Big.iso' }));
    await expect.element(page.getByText('Cancelled', { exact: true })).toBeInTheDocument();
  });

  it('cancel while sending asks the machine to delete the upload', async () => {
    const f = fake();
    vi.spyOn(api, 'uploadsFor').mockReturnValue(f.a);
    await open();
    await pick('A.iso');
    await expect.poll(() => f.gates.length).toBe(1);
    await userEvent.click(page.getByRole('button', { name: 'Cancel A.iso' }));
    await expect.poll(() => f.log).toContain('abort u1');
    await expect.element(page.getByText('Cancelled', { exact: true })).toBeInTheDocument();
  });

  it('is operable from the keyboard: the Choose files button takes Enter', async () => {
    await open();
    const b = page.getByRole('button', { name: 'Choose files' });
    (b.element() as HTMLElement).focus();
    const click = vi.spyOn(HTMLInputElement.prototype, 'click').mockImplementation(() => {});
    await userEvent.keyboard('{Enter}');
    expect(click).toHaveBeenCalled();
  });

  it('Escape closes it and the button gets focus back', async () => {
    await open();
    await userEvent.keyboard('{Escape}');
    await expect.poll(() => page.getByRole('dialog').elements().length).toBe(0);
    await expect.element(page.getByRole('button', { name: 'Add game' })).toHaveFocus();
  });
});
