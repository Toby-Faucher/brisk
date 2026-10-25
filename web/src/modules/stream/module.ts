import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'stream',
  title: 'Stream',
  icon: 'stream',
  color: 'var(--c-stream)',
  order: 2,
  routes: [{ path: '/stream', title: 'Stream' }],
} satisfies ModuleManifest;
