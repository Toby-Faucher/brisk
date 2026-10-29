<script lang="ts" module>
  // Filled 24x24 icons from the mockups (docs/ui-mockups/round1b-bento-shaders.html, round2-stream-library.html).
  const PATHS: Record<string, string> = {
    bolt: '<path d="M13.2 2 4.5 13.5h6L9.6 22l9.9-12.2h-6.4z"/>',
    home: '<path d="M12 3 3 10.5V21h6.5v-6h5v6H21V10.5z"/>',
    lib: '<rect x="3" y="3" width="8" height="8" rx="2.5"/><rect x="13" y="3" width="8" height="8" rx="2.5"/><rect x="3" y="13" width="8" height="8" rx="2.5"/><rect x="13" y="13" width="8" height="8" rx="2.5"/>',
    stream: '<circle cx="12" cy="12" r="9.5"/><path d="M10 8v8l6.5-4z" style="fill:var(--bg)"/>',
    host: '<rect x="2.5" y="4" width="19" height="12.5" rx="3"/><rect x="8" y="18.5" width="8" height="2.5" rx="1.25"/>',
    set: '<rect x="3" y="5.5" width="18" height="2.4" rx="1.2"/><circle cx="9" cy="6.7" r="3.2"/><rect x="3" y="10.8" width="18" height="2.4" rx="1.2"/><circle cx="15.5" cy="12" r="3.2"/><rect x="3" y="16.1" width="18" height="2.4" rx="1.2"/><circle cx="8" cy="17.3" r="3.2"/>',
    agent: '<path d="M12 2l2.4 6.6L21 11l-6.6 2.4L12 20l-2.4-6.6L3 11l6.6-2.4z"/><path d="M19 3l.9 2.1L22 6l-2.1.9L19 9l-.9-2.1L16 6l2.1-.9z"/>',
    news: '<rect x="3.5" y="3.5" width="17" height="17" rx="4"/><rect x="7" y="8" width="10" height="2" rx="1" style="fill:var(--bg)"/><rect x="7" y="12" width="10" height="2" rx="1" style="fill:var(--bg)"/><rect x="7" y="16" width="6" height="2" rx="1" style="fill:var(--bg)"/>',
    play: '<path d="M7 4.5v15l13-7.5z"/>',
    dl: '<path d="M11 3h2v9l3-3 1.4 1.4L12 16l-5.4-5.6L8 9l3 3z"/><rect x="5" y="18" width="14" height="2.4" rx="1.2"/>',
    search: '<circle cx="10.5" cy="10.5" r="6" stroke="currentColor" stroke-width="2.2" fill="none"/><path d="M15.5 15.5L20 20" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"/>',
    chev: '<path d="M9 5.5 15.5 12 9 18.5 7.4 16.9 12.3 12 7.4 7.1z"/>',
    plus: '<path d="M11 4h2v7h7v2h-7v7h-2v-7H4v-2h7z"/>',
    fit: '<path d="M4 9.5c0-1 .8-1.8 1.8-1.8H7v8.6H5.8c-1 0-1.8-.8-1.8-1.8zM20 9.5c0-1-.8-1.8-1.8-1.8H17v8.6h1.2c1 0 1.8-.8 1.8-1.8zM8.5 11h7v2h-7z"/>',
    vol: '<path d="M3 9.5v5h3.6L11 18.5v-13L6.6 9.5z"/><path d="M14 8.5a5 5 0 0 1 0 7M16.5 6a8.5 8.5 0 0 1 0 12" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="round"/>',
    mute: '<path d="M3 9.5v5h3.6L11 18.5v-13L6.6 9.5z"/><path d="M15 9.5l5 5m0-5l-5 5" stroke="currentColor" stroke-width="1.9" fill="none" stroke-linecap="round"/>',
    kbd: '<rect x="2" y="6" width="20" height="12" rx="3.2"/><rect x="5" y="9.4" width="2.2" height="2" rx=".6" style="fill:var(--bg)"/><rect x="8.8" y="9.4" width="2.2" height="2" rx=".6" style="fill:var(--bg)"/><rect x="12.6" y="9.4" width="2.2" height="2" rx=".6" style="fill:var(--bg)"/><rect x="16.4" y="9.4" width="2.2" height="2" rx=".6" style="fill:var(--bg)"/><rect x="7" y="13.2" width="10" height="2" rx=".8" style="fill:var(--bg)"/>',
    clip: '<rect x="5" y="4" width="14" height="17" rx="3"/><rect x="8.5" y="2.3" width="7" height="4" rx="1.6" style="fill:var(--bg)" stroke="currentColor" stroke-width="1.4"/><rect x="8" y="10" width="8" height="1.8" rx=".9" style="fill:var(--bg)"/><rect x="8" y="13.6" width="5.5" height="1.8" rx=".9" style="fill:var(--bg)"/>',
    quality: '<path d="M12 3l2.1 5.2 5.5.4-4.2 3.6 1.3 5.4L12 14.6 7.3 17.6l1.3-5.4L4.4 8.6l5.5-.4z"/>',
    chart: '<rect x="3" y="12" width="4.2" height="8" rx="1.4"/><rect x="9.9" y="6" width="4.2" height="14" rx="1.4"/><rect x="16.8" y="9" width="4.2" height="11" rx="1.4"/>',
    full: '<path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15" stroke="currentColor" stroke-width="2.2" fill="none" stroke-linecap="round"/>',
    pad: '<path d="M7.5 6h9a5 5 0 0 1 4.9 4l1 5.2a2.6 2.6 0 0 1-4.5 2.2L15.6 15H8.4l-2.3 2.4a2.6 2.6 0 0 1-4.5-2.2l1-5.2A5 5 0 0 1 7.5 6z"/><rect x="5.6" y="9.9" width="4.4" height="1.6" rx=".6" style="fill:var(--bg)"/><rect x="7" y="8.5" width="1.6" height="4.4" rx=".6" style="fill:var(--bg)"/><circle cx="16.4" cy="9.6" r="1" style="fill:var(--bg)"/><circle cx="16.4" cy="12.8" r="1" style="fill:var(--bg)"/>',
    shot: '<path d="M9 4.5 7.6 6.5H5A2.5 2.5 0 0 0 2.5 9v8.5A2.5 2.5 0 0 0 5 20h14a2.5 2.5 0 0 0 2.5-2.5V9A2.5 2.5 0 0 0 19 6.5h-2.6L15 4.5z"/><circle cx="12" cy="13" r="3.8" style="fill:var(--bg)"/><circle cx="12" cy="13" r="2.2"/>',
    replay: '<path d="M12 3.5a8.5 8.5 0 1 1-8.2 10.7l2.3-.6A6.1 6.1 0 1 0 7.7 7.7L10 10H3.5V3.5l2.5 2.5A8.5 8.5 0 0 1 12 3.5z"/><path d="M10.2 9v6.4l5.3-3.2z"/>',
    x: '<path d="M6 6l12 12M18 6L6 18" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" fill="none"/>',
    power: '<path d="M12 3v8" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" fill="none"/><path d="M7 6.5a8 8 0 1 0 10 0" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" fill="none"/>',
  };
</script>

<script lang="ts">
  let { name, color = 'currentColor', size = 18 }: { name: string; color?: string; size?: number } = $props();
</script>

<svg viewBox="0 0 24 24" width={size} height={size} fill={color} aria-hidden="true">{@html PATHS[name] ?? ''}</svg>

<style>
  svg { flex: 0 0 auto; }
</style>
