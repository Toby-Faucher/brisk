import { defineConfig } from 'vitest/config';
import { playwright } from '@vitest/browser-playwright';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { existsSync } from 'node:fs';
import { exe, shellDir } from './scripts/browser.mjs';

const BROWSER = '*.browser.test.ts';

// Playwright finds its own headless shell unless BRISK_CHROMIUM names one; say how to fix a missing one up front.
const custom = process.env.BRISK_CHROMIUM !== undefined;
if (!custom && !existsSync(shellDir))
  console.warn(
    `browser tests: no ${shellDir}; run \`npx playwright install chromium-headless-shell\` or set BRISK_CHROMIUM=/path/to/chrome-headless-shell`,
  );

// Two projects: `unit` (node, what `npm test` runs, no DOM) and `browser` (real Svelte components in the
// chrome-headless-shell Playwright already has, `npm run test:browser`).
export default defineConfig({
  test: {
    projects: [
      { test: { name: 'unit', include: ['src/**/*.test.ts'], exclude: [`src/**/${BROWSER}`] } },
      {
        plugins: [svelte()],
        // Astro's router is not in this build; tests import the same stub to see where navigate() went.
        resolve: { alias: { 'astro:transitions/client': new URL('./src/test/astro-transitions.ts', import.meta.url).pathname } },
        test: {
          name: 'browser',
          include: [`src/**/${BROWSER}`],
          // Vitest browser mode had critical advisories for an exposed API (research/ecosystem-frontend-2026-10.md):
          // its server stays on loopback, never 0.0.0.0, on a fixed port from Brisk's test range.
          api: { host: '127.0.0.1', port: 4460, strictPort: true },
          browser: {
            enabled: true,
            headless: true,
            provider: playwright({
              launchOptions: custom ? { executablePath: exe } : {},
              contextOptions: { reducedMotion: 'reduce' }, // dialog/menu animations off, so nothing waits on them
            }),
            instances: [{ browser: 'chromium' }],
            viewport: { width: 1280, height: 800 },
          },
        },
      },
    ],
  },
});
