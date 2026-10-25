<script lang="ts" module>
  // Filled 24x24 icons from the mockups (docs/ui-mockups/round1b-bento-shaders.html).
  const PATHS: Record<string, string> = {
    bolt: '<path d="M13.2 2 4.5 13.5h6L9.6 22l9.9-12.2h-6.4z"/>',
    home: '<path d="M12 3 3 10.5V21h6.5v-6h5v6H21V10.5z"/>',
    lib: '<rect x="3" y="3" width="8" height="8" rx="2.5"/><rect x="13" y="3" width="8" height="8" rx="2.5"/><rect x="3" y="13" width="8" height="8" rx="2.5"/><rect x="13" y="13" width="8" height="8" rx="2.5"/>',
    stream: '<circle cx="12" cy="12" r="9.5"/><path d="M10 8v8l6.5-4z" fill="#0b0c0f"/>',
    host: '<rect x="2.5" y="4" width="19" height="12.5" rx="3"/><rect x="8" y="18.5" width="8" height="2.5" rx="1.25"/>',
    set: '<rect x="3" y="5.5" width="18" height="2.4" rx="1.2"/><circle cx="9" cy="6.7" r="3.2"/><rect x="3" y="10.8" width="18" height="2.4" rx="1.2"/><circle cx="15.5" cy="12" r="3.2"/><rect x="3" y="16.1" width="18" height="2.4" rx="1.2"/><circle cx="8" cy="17.3" r="3.2"/>',
    agent: '<path d="M12 2l2.4 6.6L21 11l-6.6 2.4L12 20l-2.4-6.6L3 11l6.6-2.4z"/><path d="M19 3l.9 2.1L22 6l-2.1.9L19 9l-.9-2.1L16 6l2.1-.9z"/>',
    news: '<rect x="3.5" y="3.5" width="17" height="17" rx="4"/><rect x="7" y="8" width="10" height="2" rx="1" fill="#0b0c0f"/><rect x="7" y="12" width="10" height="2" rx="1" fill="#0b0c0f"/><rect x="7" y="16" width="6" height="2" rx="1" fill="#0b0c0f"/>',
    play: '<path d="M7 4.5v15l13-7.5z"/>',
    dl: '<path d="M11 3h2v9l3-3 1.4 1.4L12 16l-5.4-5.6L8 9l3 3z"/><rect x="5" y="18" width="14" height="2.4" rx="1.2"/>',
    search: '<circle cx="10.5" cy="10.5" r="6" stroke="currentColor" stroke-width="2.2" fill="none"/><path d="M15.5 15.5L20 20" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"/>',
    plus: '<path d="M11 4h2v7h7v2h-7v7h-2v-7H4v-2h7z"/>',
    fit: '<path d="M4 9.5c0-1 .8-1.8 1.8-1.8H7v8.6H5.8c-1 0-1.8-.8-1.8-1.8zM20 9.5c0-1-.8-1.8-1.8-1.8H17v8.6h1.2c1 0 1.8-.8 1.8-1.8zM8.5 11h7v2h-7z"/>',
  };
</script>

<script lang="ts">
  let { name, color = 'currentColor', size = 18 }: { name: string; color?: string; size?: number } = $props();
</script>

<svg viewBox="0 0 24 24" width={size} height={size} fill={color} aria-hidden="true">{@html PATHS[name] ?? ''}</svg>

<style>
  svg { flex: 0 0 auto; }
</style>
