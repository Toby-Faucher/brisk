<script lang="ts">
  // A list of arguments, one per row (no shell string). Used for the custom command and the extra arguments.
  let { values, name, first, disabled = false, onchange }: { values: string[]; name: string; first?: string; disabled?: boolean; onchange: (v: string[], now: boolean) => void } = $props();
  const label = (i: number) => (i === 0 && first ? first : `${name} ${i + 1}`);
  // Focus lands on the new row after Add.
  let list: HTMLElement | undefined = $state();
  function add() {
    onchange([...values, ''], true);
    queueMicrotask(() => setTimeout(() => list?.querySelector<HTMLInputElement>('li:last-child input')?.focus(), 0));
  }
</script>

<ul class="args" bind:this={list}>
  {#each values as v, i (i)}
    <li>
      <input type="text" class="in" aria-label={label(i)} value={v} {disabled} spellcheck="false" autocomplete="off" oninput={(e) => onchange(values.map((x, j) => (j === i ? e.currentTarget.value : x)), false)} />
      <button type="button" class="btn ghost sm" {disabled} aria-label="Remove {label(i)}" onclick={() => onchange(values.filter((_, j) => j !== i), true)}>Remove</button>
    </li>
  {/each}
</ul>
<button type="button" class="btn ghost sm add" {disabled} onclick={add}>{values.length ? `Add ${name.toLowerCase()}` : first ? 'Add the program' : `Add ${name.toLowerCase()}`}</button>

<style>
  .args { list-style: none; margin: 0 0 6px; padding: 0; display: grid; gap: 6px; }
  .args li { display: flex; gap: 8px; align-items: center; }
  .in { flex: 1; min-width: 0; font: inherit; font-family: var(--mono); font-size: 13px; padding: 6px 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--panel); color: var(--fg); }
  .in:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .in:disabled { opacity: .55; }
</style>
