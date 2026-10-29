// Keeping a stream screenshot with the game on the machine that streams it.
import { errMsg } from '../../state/errMsg';

export const MAX_SHOT_BYTES = 8 << 20;

export type Kept = { ok: true } | { ok: false; why: string };

/** Send one PNG with `post`. Too big or refused: a short reason for the toast, never a throw. */
export async function keepShot(png: Blob, post: (b: Blob) => Promise<unknown>): Promise<Kept> {
  if (png.size > MAX_SHOT_BYTES) return { ok: false, why: `it is ${(png.size / 1048576).toFixed(1)} MB and the limit is 8 MB` };
  try {
    await post(png);
    return { ok: true };
  } catch (e) {
    return { ok: false, why: errMsg(e) };
  }
}
