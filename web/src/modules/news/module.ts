import type { ModuleManifest } from '../../shell/registry';

export default {
  id: 'news',
  title: 'News',
  icon: 'news',
  color: 'var(--c-news)',
  order: 11,
  routes: [],
  soon: true,
} satisfies ModuleManifest;
