import { afterEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import FriendsCard from './FriendsCard.svelte';

const reply = (body: unknown) =>
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(JSON.stringify(body))),
  );
afterEach(() => vi.unstubAllGlobals());

describe('FriendsCard', () => {
  it('lists names with what they play; letter placeholder without an avatar', async () => {
    reply({
      available: true,
      online: [
        { name: 'TestFriend1', avatar: 'https://avatars.steamstatic.com/x.jpg', state: 'online', playing: 'Portal 2', appid: 620 },
        { name: 'testAway', avatar: '', state: 'away', playing: null, appid: null },
        { name: 'testBusyPlayer', avatar: '', state: 'busy', playing: 'Hades', appid: 1 },
      ],
    });
    await render(FriendsCard);
    await expect.element(page.getByRole('list')).toBeInTheDocument();
    await expect.element(page.getByText('TestFriend1')).toBeInTheDocument();
    await expect.element(page.getByText('playing Portal 2')).toBeInTheDocument();
    await expect.element(page.getByText('away', { exact: true })).toBeInTheDocument();
    await expect.element(page.getByText('playing Hades (busy)')).toBeInTheDocument();
    expect(page.getByRole('listitem').elements()).toHaveLength(3);
    const imgs = document.querySelectorAll('li img');
    expect(imgs).toHaveLength(1);
    expect(imgs[0].getAttribute('alt')).toBe('');
    expect(document.querySelectorAll('li span.av')[0].textContent).toBe('T');
  });

  it('says so when friends are not available or no one is online', async () => {
    reply({ available: false, online: [] });
    const a = await render(FriendsCard);
    await expect.element(page.getByText('Friends are not available')).toBeInTheDocument();
    a.unmount();
    reply({ available: true, online: [] });
    await render(FriendsCard);
    await expect.element(page.getByText('No one online')).toBeInTheDocument();
  });

  it('polls through the visible-only poller, every 60 s, and cleans up', async () => {
    reply({ available: true, online: [] });
    const stop = vi.fn();
    const poll = vi.fn((fn: () => void, _doc?: unknown, _every?: number) => (fn(), stop));
    const r = await render(FriendsCard, { props: { poll } });
    expect(poll.mock.calls[0][2]).toBe(60_000);
    r.unmount();
    expect(stop).toHaveBeenCalled();
  });
});
