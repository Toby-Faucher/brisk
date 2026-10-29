// One way to add listeners that all go away together: every one is tied to `signal`.

type Events = WindowEventMap & DocumentEventMap & HTMLElementEventMap;

/** An `on(target, type, handler)` whose listeners are removed when `signal` aborts. `passive` is the default for each. */
export const listener =
  (signal: AbortSignal, passive?: boolean) =>
  <K extends keyof Events>(t: EventTarget, type: K, f: (e: Events[K]) => void, opts: AddEventListenerOptions = {}) =>
    t.addEventListener(type, f as EventListener, { passive, ...opts, signal });
