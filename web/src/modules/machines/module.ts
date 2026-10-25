import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'machines',
  title: 'Machines',
  icon: 'host',
  color: 'var(--c-host)',
  order: 3,
  routes: [{ path: '/machines', title: 'Machines' }],
} satisfies ModuleManifest;
