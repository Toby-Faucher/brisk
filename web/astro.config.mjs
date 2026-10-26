import { defineConfig } from 'astro/config';
import svelte from '@astrojs/svelte';
import { execSync } from 'node:child_process';

const commit = (() => {
  try {
    return execSync('git rev-parse --short HEAD', { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
  } catch {
    return 'unknown';
  }
})();

// Static build (web/dist), served on localhost: WebTransport and WebCodecs need a secure
// context, and http://localhost is one (see docs/m2-notes.md).
export default defineConfig({
  integrations: [svelte()],
  vite: { define: { __BRISK_COMMIT__: JSON.stringify(commit) } },
});
