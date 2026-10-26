import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'history',
  title: 'History',
  icon: 'chart',
  color: 'var(--c-hist)',
  order: 3.5,
  routes: [{ path: '/history', title: 'History' }],
} satisfies ModuleManifest;
