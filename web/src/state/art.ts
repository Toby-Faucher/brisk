// Cached art lives on the machine behind /api, so an <img src> cannot load it (it cannot send the secret header). Same
// pattern as host screenshots: fetch with the auth headers, show the object URL. Other URLs (Steam's CDN) pass through.
import { api } from './api';

/** `{@attach artSrc(url)}` on an <img>: sets `src` once it is loadable. A failed fetch fires the element's `error` event. */
export const artSrc = (src: string | undefined) => (el: HTMLImageElement) => {
  if (!src) return;
  if (!src.startsWith('/api/')) {
    el.src = src;
    return;
  }
  let live = true;
  el.removeAttribute('src');
  api.artImage(src).then(
    (u) => live && (el.src = u),
    () => live && el.dispatchEvent(new Event('error')),
  );
  return () => {
    live = false;
  };
};

/** The same for a CSS background on any element. */
export const artBg = (src: string | undefined) => (el: HTMLElement) => {
  if (!src) return;
  if (!src.startsWith('/api/')) {
    el.style.backgroundImage = `url(${src})`;
    return;
  }
  let live = true;
  api.artImage(src).then(
    (u) => live && (el.style.backgroundImage = `url(${u})`),
    () => {},
  );
  return () => {
    live = false;
  };
};
