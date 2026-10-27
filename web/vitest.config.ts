import { defineConfig } from 'vitest/config';
import { playwright } from '@vitest/browser-playwright';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';

const BROWSER = '*.browser.test.ts';

// Playwright finds its own headless shell unless BRISK_CHROMIUM names one; say how to fix a missing one up front.
const exe = process.env.BRISK_CHROMIUM;
if (!exe) {
  const pw = dirname(createRequire(import.meta.url).resolve('playwright-core'));
  const rev = JSON.parse(readFileSync(join(pw, 'browsers.json'), 'utf8')).browsers.find(
    (b: { name: string }) => b.name === 'chromium-headless-shell',
  )?.revision;
  const dir = join(process.env.PLAYWRIGHT_BROWSERS_PATH ?? join(homedir(), '.cache/ms-playwright'), `chromium_headless_shell-${rev}`);
  if (!existsSync(dir))
    console.warn(
      `browser tests: no ${dir}; run \`npx playwright install chromium-headless-shell\` or set BRISK_CHROMIUM=/path/to/chrome-headless-shell`,
    );
}

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
              launchOptions: exe ? { executablePath: exe } : {},
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
