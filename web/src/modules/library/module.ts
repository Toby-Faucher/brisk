import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'library',
  title: 'Library',
  icon: 'lib',
  color: 'var(--c-lib)',
  order: 1,
  routes: [{ path: '/library', title: 'Library' }],
} satisfies ModuleManifest;
