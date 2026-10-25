import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'agents',
  title: 'Agents',
  icon: 'agent',
  color: 'var(--c-agent)',
  order: 10,
  routes: [],
  soon: true,
} satisfies ModuleManifest;
