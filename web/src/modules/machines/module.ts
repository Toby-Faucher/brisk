import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'machines',
  title: 'Machines',
  icon: 'host',
  color: 'var(--c-host)',
  order: 3,
  widget: () => import('./Widget.svelte'),
  routes: [{ path: '/machines', title: 'Machines' }],
} satisfies ModuleManifest;
