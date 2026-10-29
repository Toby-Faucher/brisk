// Web storage that never throws: a private window, blocked site data or no storage at all
// (even reading `localStorage` can throw) reads as nothing and drops the write.

/** The stored value, or null when there is none or storage is out of reach. */
export function safeGet(storage: () => Storage, key: string): string | null {
  try {
    return storage().getItem(key);
  } catch {
    return null;
  }
}

/** Store `value`, or remove the key for null; a failure is ignored. */
export function safeSet(storage: () => Storage, key: string, value: string | null): void {
  try {
    if (value === null) storage().removeItem(key);
    else storage().setItem(key, value);
  } catch {}
}
