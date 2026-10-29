// The chrome-headless-shell for the check scripts and the browser tests: BRISK_CHROMIUM if set, otherwise the one the
// installed playwright-core expects (its revision comes from playwright-core/browsers.json, so a bump needs no edits here).
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';

const pw = dirname(createRequire(import.meta.url).resolve('playwright-core'));
/** @type {{ browsers: { name: string, revision: string }[] }} */
const manifest = JSON.parse(readFileSync(join(pw, 'browsers.json'), 'utf8'));
const rev = manifest.browsers.find((b) => b.name === 'chromium-headless-shell')?.revision;

/** Playwright's own install dir for that revision (what `npx playwright install chromium-headless-shell` fills). */
export const shellDir = join(process.env.PLAYWRIGHT_BROWSERS_PATH ?? join(homedir(), '.cache/ms-playwright'), `chromium_headless_shell-${rev}`);

/** Path to pass as `executablePath`. */
export const exe = process.env.BRISK_CHROMIUM ?? join(shellDir, 'chrome-headless-shell-linux64/chrome-headless-shell');
