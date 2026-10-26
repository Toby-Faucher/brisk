// Stand-in for `astro:transitions/client` in browser component tests (aliased in vitest.config.ts).
import { vi } from 'vitest';

export const navigate = vi.fn(async (_href: string) => {});
