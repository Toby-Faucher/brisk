import { defineConfig } from 'vitest/config';
import { playwright } from '@vitest/browser-playwright';
import { svelte } from '@sveltejs/vite-plugin-svelte';

const BROWSER = '*.browser.test.ts';
const HOME = process.env.HOME ?? '';

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
              launchOptions: {
                executablePath: process.env.BRISK_CHROMIUM ?? `${HOME}/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell`,
              },
            }),
            instances: [{ browser: 'chromium' }],
            viewport: { width: 1280, height: 800 },
          },
        },
      },
    ],
  },
});
