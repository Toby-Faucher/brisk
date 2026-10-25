import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'settings',
  title: 'Settings',
  icon: 'set',
  color: 'var(--c-set)',
  order: 4,
  routes: [{ path: '/settings', title: 'Settings' }],
} satisfies ModuleManifest;
