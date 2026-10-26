// Screen Wake Lock while the stream is live, so the laptop does not dim or sleep mid-game (Chrome 84+, secure
// context). The browser drops the lock whenever the tab is hidden; it is asked for again when the tab is visible.
// No UI, no timers: a refusal (NotAllowedError, low battery, unsupported) is one console.debug and nothing else.

type Sentinel = { released: boolean; release(): Promise<void> };
type Nav = { wakeLock?: { request(type: 'screen'): Promise<Sentinel> } };
type Doc = Pick<Document, 'visibilityState' | 'addEventListener' | 'removeEventListener'>;

export function keepAwake(nav: Nav = navigator as Nav, doc: Doc = document) {
  let want = false;
  let lock: Sentinel | null = null;
  let asking = false;
  let said = false;
  const acquire = async () => {
    if (!want || asking || (lock && !lock.released) || doc.visibilityState !== 'visible' || !nav.wakeLock) return;
    asking = true;
    try {
      const l = await nav.wakeLock.request('screen');
      if (want) lock = l;
      else void l.release().catch(() => {}); // turned off while asking
    } catch (e) {
      if (!said) (said = true), console.debug('wake lock:', (e as Error)?.name ?? e);
    } finally {
      asking = false;
    }
  };
  const drop = () => {
    const l = lock;
    lock = null;
    if (l && !l.released) void l.release().catch(() => {});
  };
  const onVisible = () => void acquire();
  doc.addEventListener('visibilitychange', onVisible);
  return {
    /** true while the stream is live and the setting is on. */
    set(on: boolean) {
      want = on;
      if (on) void acquire();
      else drop();
    },
    stop() {
      want = false;
      drop();
      doc.removeEventListener('visibilitychange', onVisible);
    },
  };
}
