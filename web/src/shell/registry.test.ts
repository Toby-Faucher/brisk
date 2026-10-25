import { describe, expect, it } from 'vitest';
import { loadModules, type ModuleManifest } from './registry';

describe('registry', () => {
  it('modules_sorted_by_order', () => {
    const mods = loadModules();
    expect(mods.map((m) => m.id)).toEqual(['home', 'library', 'stream', 'machines', 'settings', 'agents', 'news', 'workouts']);
    const orders = mods.map((m) => m.order);
    expect(orders).toEqual([...orders].sort((a, b) => a - b));
  });

  it('soon_modules_have_no_routes_and_are_flagged', () => {
    const soon = loadModules().filter((m) => m.soon);
    expect(soon.map((m) => m.id)).toEqual(['agents', 'news', 'workouts']);
    for (const m of soon) expect(m.routes).toEqual([]);
    for (const m of loadModules().filter((m) => !m.soon)) expect(m.routes.length).toBeGreaterThan(0);
  });

  it('adding_a_module_folder_needs_no_shell_edit', () => {
    const fake: ModuleManifest = { id: 'fake', title: 'Fake', icon: 'plus', color: 'var(--accent)', order: 2.5, routes: [{ path: '/fake', title: 'Fake' }] };
    const glob = {
      '../modules/b/module.ts': { default: { ...fake, id: 'b', order: 9 } },
      '../modules/fake/module.ts': { default: fake },
      '../modules/a/module.ts': { default: { ...fake, id: 'a', order: 1 } },
    };
    expect(loadModules(glob).map((m) => m.id)).toEqual(['a', 'fake', 'b']);
  });
});
