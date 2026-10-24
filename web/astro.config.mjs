import { defineConfig } from 'astro/config';
import svelte from '@astrojs/svelte';

// Static build (web/dist), served on localhost: WebTransport and WebCodecs need a secure
// context, and http://localhost is one (see docs/m2-notes.md).
export default defineConfig({
  integrations: [svelte()],
});
