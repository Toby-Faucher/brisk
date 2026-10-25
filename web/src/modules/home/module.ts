import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'home',
  title: 'Home',
  icon: 'home',
  color: 'var(--accent)',
  order: 0,
  routes: [{ path: '/', title: 'Home' }],
} satisfies ModuleManifest;
