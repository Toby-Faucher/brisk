<script lang="ts">
  // The launch options form, shared by Settings (the defaults) and a game's panel (an override). With `inherit` each
  // field also has Inherit / Override; an inherited field shows the default's values and cannot be edited.
  import { FIELDS, GS_BOOL, GS_NUM, gsProblems, type Form, type Inherit } from './form';
  import ArgList from './ArgList.svelte';

  let {
    form,
    envAllowed,
    allow,
    inherit,
    uid,
    onchange,
    oninherit,
    onallow,
  }: {
    form: Form;
    envAllowed: string[];
    allow: boolean;
    inherit?: Inherit;
    /** Makes ids unique when the editor is on a page twice. */
    uid: string;
    onchange: (f: Form, now: boolean) => void;
    oninherit?: (i: Inherit) => void;
    /** Defaults only: the Allow custom commands switch is drawn in the custom section. */
    onallow?: (on: boolean) => void;
  } = $props();

  const up = (fn: (f: Form) => void, now = false) => {
    const n = structuredClone($state.snapshot(form));
    fn(n);
    onchange(n, now);
  };
  const names = $derived([...new Set([...envAllowed, ...form.env.map(([k]) => k)])]);
  const free = (own: string) => names.filter((n) => n === own || !form.env.some(([k]) => k === n));
  const gsBad = $derived(gsProblems(form.gs));
  const locked = (f: keyof Inherit) => !!inherit?.[f];
  const TITLE = { wrappers: 'Wrappers', env: 'Environment variables', custom: 'Your own command', extra: 'Extra arguments' } as const;
</script>

<div class="lo">
  {#snippet head(f: (typeof FIELDS)[number])}
    <div class="hd">
      <h4 id="{uid}-{f}">{TITLE[f]}</h4>
      {#if inherit && oninherit}
        <div class="seg" role="radiogroup" aria-label="{TITLE[f]}: follow the default or override it">
          {#each [true, false] as inh (inh)}
            <label class="r"><input type="radio" name="{uid}-{f}-mode" checked={inherit[f] === inh} onchange={() => oninherit({ ...inherit, [f]: inh })} />{inh ? 'Inherit' : 'Override'}</label>
          {/each}
        </div>
      {/if}
    </div>
  {/snippet}

  <section aria-labelledby="{uid}-wrappers">
    {@render head('wrappers')}
    <fieldset class="body" disabled={locked('wrappers')}>
      <legend class="vh">Wrappers</legend>
      <label class="chk"><input type="checkbox" checked={form.mango} onchange={(e) => up((f) => (f.mango = e.currentTarget.checked), true)} />MangoHud <small>shows an overlay with FPS</small></label>
      <label class="chk"><input type="checkbox" checked={form.gamemode} onchange={(e) => up((f) => (f.gamemode = e.currentTarget.checked), true)} />GameMode <small>asks the system for full speed</small></label>
      <label class="chk"><input type="checkbox" checked={form.gs.on} onchange={(e) => up((f) => (f.gs.on = e.currentTarget.checked), true)} />gamescope <small>runs the game in its own small compositor</small></label>
      {#if form.gs.on}
        <div class="gs" role="group" aria-label="gamescope settings">
          {#each GS_NUM as n (n.flag)}
            <label class="num">{n.label}<input type="text" inputmode="numeric" class="in" value={form.gs.num[n.flag] ?? ''} aria-invalid={gsBad.some((b) => b.startsWith(n.label))} oninput={(e) => up((f) => (f.gs.num[n.flag] = e.currentTarget.value))} /></label>
          {/each}
          {#each GS_BOOL as b (b.flag)}
            <label class="chk"><input type="checkbox" checked={!!form.gs.flags[b.flag]} onchange={(e) => up((f) => (f.gs.flags[b.flag] = e.currentTarget.checked), true)} />{b.label}</label>
          {/each}
          {#each gsBad as b (b)}<p class="bad" role="alert">{b}</p>{/each}
          <p class="note">Leave a number empty to keep gamescope's own choice.</p>
        </div>
      {/if}
    </fieldset>
  </section>

  <section aria-labelledby="{uid}-env">
    {@render head('env')}
    <fieldset class="body" disabled={locked('env')}>
      <legend class="vh">Environment variables</legend>
      <ul class="args">
        {#each form.env as [k, v], i (i)}
          <li>
            <select class="in sel" aria-label="Variable {i + 1}" value={k} onchange={(e) => up((f) => (f.env[i][0] = e.currentTarget.value), true)}>
              {#each free(k) as n (n)}<option value={n}>{n}</option>{/each}
            </select>
            <input type="text" class="in" aria-label="Value of {k}" value={v} spellcheck="false" autocomplete="off" oninput={(e) => up((f) => (f.env[i][1] = e.currentTarget.value))} />
            <button type="button" class="btn ghost sm" aria-label="Remove {k}" onclick={() => up((f) => f.env.splice(i, 1), true)}>Remove</button>
          </li>
        {/each}
      </ul>
      <button type="button" class="btn ghost sm" disabled={!free('').length} onclick={() => up((f) => f.env.push([free('')[0], '']), true)}>Add a variable</button>
      <p class="note">Only the variables in the list can be set. A value is one line, 512 characters at most.</p>
    </fieldset>
  </section>

  <section aria-labelledby="{uid}-custom">
    {@render head('custom')}
    {#if onallow}
      <label class="chk allow"><input type="checkbox" role="switch" checked={allow} onchange={(e) => onallow(e.currentTarget.checked)} />Allow custom commands</label>
    {/if}
    {#if onallow}<p class="note">Anyone who can log in to brisk can already control this machine through the stream, so this switch is a catch against mistakes, not a security boundary.</p>{/if}
    <p class="note">{allow ? 'A command runs on this machine as you.' : 'A command runs on this machine as you, so it stays off until you allow it.'}{#if onallow && allow} Turning the switch off clears the saved command.{/if}</p>
    {#if allow}
      <fieldset class="body" disabled={locked('custom')}>
        <legend class="vh">Your own command</legend>
        <ArgList values={form.custom} name="Custom argument" first="Program" disabled={locked('custom')} onchange={(v, now) => up((f) => (f.custom = v), now)} />
        <p class="note">The first row is the program: a name like mytool, or a full path that starts with a slash. It cannot contain "..". Every other row is one argument. Nothing goes through a shell, so quotes, pipes and $VARIABLES mean nothing. Up to 32 rows.</p>
      </fieldset>
    {:else if !onallow}
      <p class="note"><a class="link" href="/settings#launch">Allow custom commands in Settings</a> to set one.</p>
    {/if}
  </section>

  <section aria-labelledby="{uid}-extra">
    {@render head('extra')}
    <fieldset class="body" disabled={locked('extra')}>
      <legend class="vh">Extra arguments</legend>
      <ArgList values={form.extra} name="Extra argument" disabled={locked('extra')} onchange={(v, now) => up((f) => (f.extra = v), now)} />
      <p class="note">Added after the game's own arguments. One argument per row.</p>
    </fieldset>
  </section>
</div>

<style>
  .lo { display: grid; gap: 18px; margin-top: 12px; }
  .hd { display: flex; flex-wrap: wrap; gap: 6px 14px; align-items: center; justify-content: space-between; }
  h4 { margin: 0; font-size: 14px; font-weight: 600; }
  .seg { display: inline-flex; gap: 12px; font-size: 13px; }
  .r, .chk { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; cursor: pointer; }
  .chk small { color: var(--muted); }
  .body { border: 0; margin: 8px 0 0; padding: 0; display: grid; gap: 8px; justify-items: start; min-width: 0; }
  .body:disabled { opacity: .6; }
  .allow { margin-top: 8px; }
  .vh { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .gs { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px 12px; align-items: end; padding: 8px 0 0 22px; width: 100%; box-sizing: border-box; }
  .gs .note, .gs .bad { grid-column: 1 / -1; }
  .num { display: grid; gap: 4px; font-size: 12.5px; color: var(--muted); }
  .note { margin: 6px 0 0; font-size: 12px; color: var(--muted); }
  .bad { margin: 0; font-size: 12.5px; color: var(--warn); }
  .args { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; width: 100%; }
  .args li { display: flex; gap: 8px; align-items: center; }
  .in { font: inherit; font-size: 13px; padding: 6px 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--panel); color: var(--fg); min-width: 0; }
  .args .in:not(.sel) { flex: 1; }
  .in[aria-invalid='true'] { border-color: var(--warn); }
  .in:focus-visible, input:focus-visible, select:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
  .link { color: var(--accent); }
  input[type='checkbox'], input[type='radio'] { accent-color: var(--accent); }
</style>
