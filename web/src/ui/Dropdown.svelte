<script lang="ts" generics="T extends string">
  // A rounded pick-one menu in place of a native <select> (Bits UI Select: arrow keys, Enter, Escape, type-ahead,
  // focus back on the button). The button shows the current label; `label` is its accessible name.
  import { Select } from 'bits-ui';
  import Icon from './Icon.svelte';

  let { value = $bindable(), options, label, onchange }: { value: T; options: [T, string][]; label: string; onchange?: (v: T) => void } = $props();
  const items = $derived(options.map(([v, l]) => ({ value: v, label: l })));
  const current = $derived(options.find(([v]) => v === value)?.[1] ?? '');
</script>

<Select.Root type="single" {items} bind:value={() => value, (v) => v !== value && ((value = v as T), onchange?.(v as T))}>
  <Select.Trigger class="dd" aria-label={label}><span>{current}</span><Icon name="chev" size={14} /></Select.Trigger>
  <Select.Portal>
    <Select.Content class="dd-menu glass" sideOffset={6} align="end">
      <Select.Viewport>
        {#each items as it (it.value)}
          <Select.Item class="dd-item" value={it.value} label={it.label}>
            {#snippet children({ selected })}<span>{it.label}</span>{#if selected}<i aria-hidden="true">✓</i>{/if}{/snippet}
          </Select.Item>
        {/each}
      </Select.Viewport>
    </Select.Content>
  </Select.Portal>
</Select.Root>

<style>
  /* Bits renders these: global, prefixed dd- */
  :global(.dd) { display: inline-flex; align-items: center; gap: 8px; padding: 5px 12px 5px 12px; border-radius: 999px; border: 1px solid var(--line); background: var(--panel); color: var(--muted); font: inherit; font-size: 13px; cursor: pointer; white-space: nowrap; }
  :global(.dd:hover), :global(.dd[data-state='open']) { color: var(--fg); border-color: var(--line-hover); }
  :global(.dd svg) { rotate: 90deg; transition: rotate .18s var(--ease); }
  :global(.dd[data-state='open'] svg) { rotate: -90deg; }
  :global(.dd-menu) { z-index: 50; min-width: var(--bits-select-anchor-width); padding: 5px; border-radius: 14px; font-size: 13px; outline: none; }
  :global(.dd-item) { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 6px 10px; border-radius: 9px; color: var(--muted); cursor: pointer; outline: none; }
  :global(.dd-item[data-highlighted]) { background: var(--panel2); color: var(--fg); box-shadow: 0 0 0 2px var(--accent) inset; }
  :global(.dd-item[data-selected]) { color: var(--fg); }
  :global(.dd-item i) { font-style: normal; color: var(--accent); }
  @media (prefers-reduced-motion: no-preference) {
    :global(.dd-menu) { transition: opacity var(--dur-tab) var(--ease), translate var(--dur-tab) var(--ease); }
    :global(.dd-menu[data-ending-style]) { opacity: 0; translate: 0 -4px; transition-duration: var(--dur-exit); }
  }
  @starting-style {
    :global(.dd-menu[data-state='open']) { opacity: 0; translate: 0 -4px; }
  }
</style>
