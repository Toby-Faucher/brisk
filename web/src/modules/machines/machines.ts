import { ApiError, type HostInfo } from '../../state/api';

export type Machine = { id: string; info: HostInfo };

/** One host today; the page renders a list so more machines slot in later. */
export const machinesFrom = (info: HostInfo): Machine[] => [{ id: info.name.trim().toLowerCase() || 'bawn', info }];

export function restartError(e: unknown): string {
  if (e instanceof ApiError && e.status === 429) return 'Sunshine was restarted a moment ago, try again in a minute';
  return `Could not restart Sunshine: ${(e as Error).message}`;
}
