/** One emulator in Settings > Emulators. A second emulator is one more entry here (plus its views in Emulators.svelte). */
export type EmulatorSection = { id: string; name: string /** not built yet: kept out of the picker and the routes */; planned?: boolean };
export type EmulatorEntry = { id: string; name: string; sections: EmulatorSection[] };

export const EMULATORS: EmulatorEntry[] = [
  {
    id: 'dolphin',
    name: 'Dolphin',
    sections: [
      { id: 'controllers', name: 'Controllers' },
      { id: 'launch', name: 'Launch options', planned: true },
      { id: 'folders', name: 'Games folders', planned: true },
      { id: 'metadata', name: 'Metadata', planned: true },
    ],
  },
];

export const shownSections = (e: EmulatorEntry) => e.sections.filter((s) => !s.planned);
/** An unknown or missing id is the first entry. */
export const emulatorOf = (id: string | null | undefined) => EMULATORS.find((e) => e.id === id) ?? EMULATORS[0];
export const sectionOf = (e: EmulatorEntry, id: string | null | undefined) =>
  shownSections(e).find((s) => s.id === id) ?? shownSections(e)[0];
