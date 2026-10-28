<script lang="ts">
  // The Controls section of an emulator game's detail panel: the Classic Controller drawing, a side card for the
  // selected control, listening for a pad button, Reset, and the apply bar. Spec: docs/plans/2026-10-27-emulator-controls.md.
  import { onMount } from 'svelte';
  import { padName, type PadLike } from '../../../lib/gamepad';
  import { toast } from '../../../shell/Toasts.svelte';
  import type { ControlId } from '../../../state/api';
  import { padChoices, padView } from '../../settings/controllertest';
  import ClassicDiagram from './ClassicDiagram.svelte';
  import { bind, CONTROL_IDS, control, PAD_BY_INDEX, XBOX_DEFAULT } from './classic';
  import { createControls, SAVED } from './controls';
  import { Listener } from './listen';

  let { appid, name }: { appid: number; name: string } = $props();

  // svelte-ignore state_referenced_locally -- the detail panel keys this component by game
  const ctl = createControls(appid, name, { toast });
  const st = ctl.state;

  let root: HTMLElement;
  let diagram: ClassicDiagram | undefined;
  let sel = $state<ControlId>('a');
  let listener = $state<Listener | null>(null);
  let hint = $state('');
  let padNow = $state(''); // name of the connected pad ('' none)
  let pressing = $state('');
  let dzLive = $state<{ ls: number | null; rs: number | null }>({ ls: null, rs: null });

  const draft = $derived($st.draft);
  const listening = $derived(listener?.id ?? null);
  const selCtl = $derived(control(sel));
  const bound = $derived(draft?.bindings[sel] ?? null);
  const customMap = $derived(Object.fromEntries(CONTROL_IDS.map((id) => [id, draft?.bindings[id] === null])) as Record<ControlId, boolean>);
  const changedFromDefault = $derived(bound !== null && bound !== XBOX_DEFAULT[sel]);
  const stickId = $derived(sel === 'ls' || sel === 'rs' ? sel : null);
  const dz = $derived(stickId ? (dzLive[stickId] ?? draft?.deadZone[stickId] ?? 0) : 0);

  const line = $derived(
    listening
      ? hint || (padNow ? `Listening for ${selCtl.name}. Press a button on your pad.` : 'Waiting for a pad.')
      : [$st.note?.tone === 'info' ? $st.note.text : '', $st.saved && !$st.saving ? SAVED : ''].filter(Boolean).join(' '),
  );
  const barShown = $derived($st.restarting || !!$st.restartError || (!!$st.data?.running && $st.changed));

  function commitDz(id: 'ls' | 'rs', n: number) {
    ctl.setDeadZone(id, n);
    dzLive[id] = null;
  }
  function select(id: ControlId) {
    if (listener && listener.id !== id) stopListen();
    sel = id;
  }
  function startListen(id: ControlId) {
    sel = id;
    hint = '';
    listener = new Listener(id); // replaces any listener before it
  }
  function stopListen(refocus = false) {
    listener = null;
    hint = '';
    if (refocus) diagram?.focus(sel);
  }

  function heard(h: ReturnType<Listener['push']>) {
    if (h.type === 'wrong') hint = h.hint;
    else if (h.type === 'cancel') {
      stopListen();
      ctl.setNote('The pad was disconnected. Listening stopped.');
    } else if (h.type === 'bind' && draft) {
      const id = sel;
      const r = bind(draft.bindings, id, h.pad);
      stopListen(true);
      if (r.bindings === draft.bindings) return;
      const other = r.swappedWith && control(r.swappedWith).name;
      ctl.setBindings(r.bindings, {
        text: other ? `Swapped with ${other} (a pad button can only drive one control).` : `${control(id).name} is now bound to ${h.pad}.`,
        tone: 'info',
      });
    }
  }

  // The pad loop: one rAF loop while the panel is open and the page has focus. It feeds the chip and the listener.
  let raf = 0;
  let seen = '';
  function frame() {
    raf = requestAnimationFrame(frame);
    const all = (navigator.getGamepads?.() ?? []) as (PadLike | null)[];
    const first = padChoices(all)[0];
    const g = first && all.find((p) => p?.connected && p.index === first.index);
    const v = g ? padView(g, 0.08) : null;
    if ((first?.id ?? '') !== seen) padNow = (seen = first?.id ?? '') ? padName(seen) : '';
    const b = v?.buttons.find((x) => (x.index === 6 || x.index === 7 ? x.value > 0.5 : x.pressed) && PAD_BY_INDEX[x.index]);
    pressing = b ? PAD_BY_INDEX[b.index] : '';
    if (listener) heard(listener.push(v));
  }
  const active = () => document.hasFocus() && document.visibilityState === 'visible';
  function sync() {
    if (active() && !raf) raf = requestAnimationFrame(frame);
    if (!active() && raf) {
      cancelAnimationFrame(raf);
      raf = 0;
      stopListen(); // leaving the page cancels
    }
  }

  onMount(() => {
    void ctl.load();
    const ac = new AbortController();
    const on = (t: EventTarget, type: string, f: (e: never) => void) => t.addEventListener(type, f as EventListener, { signal: ac.signal });
    on(window, 'focus', () => (sync(), void ctl.refresh()));
    on(window, 'blur', sync);
    on(document, 'visibilitychange', sync);
    on(window, 'pointerdown', (e: PointerEvent) => {
      if (listener && !root.contains(e.target as Node)) stopListen(); // a click elsewhere cancels
    });
    sync();
    return () => {
      ac.abort();
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
    };
  });

  function keydown(e: KeyboardEvent) {
    if (!listener) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation(); // the detail panel must not close on it
      stopListen(true);
    } else if (e.key === 'Tab') stopListen();
  }
</script>

{#if $st.load !== 'hidden'}
  <section bind:this={root} class="sec" aria-labelledby="ctl-h" aria-busy={$st.load === 'loading'} onkeydowncapture={keydown}>
    <h3 id="ctl-h">Controls</h3>
    <p class="lab">These buttons are shared by every Dolphin game on this machine. Click a control, then press the pad button you want.</p>

    {#if $st.load === 'loading'}
      <div class="skel" aria-hidden="true"></div>
      <p class="lab">Loading the controls...</p>
    {:else if $st.load === 'error' || !draft}
      <div class="err"><span role="alert">{$st.note?.text ?? 'Could not load the controls.'}</span><button class="btn ghost sm" onclick={() => ctl.retry()}>Try again</button></div>
    {:else}
      <p class="chip">
        {#if padNow}<span>Pad: {padNow}</span>{#if pressing}<span class="press" aria-hidden="true">{pressing}</span>{/if}
        {:else}No pad seen. Click a button on your pad, or connect one.{/if}
      </p>

      <div class="grid">
        <div class="dia">
          <ClassicDiagram bind:this={diagram} bindings={draft.bindings} custom={customMap} selected={sel} {listening} onselect={select} onlisten={startListen} />
        </div>
        <div class="side">
          <div class="card">
            <span class="lab">Selected control</span>
            <div class="big">{selCtl.name}</div>
            <div class="row">
              <span class="lab">Pad button</span>
              <span class="bind" class:custom={bound === null}>{bound ?? 'Custom'}</span>
              {#if changedFromDefault}<span class="mark">changed</span>{/if}
            </div>
            {#if bound === null}<p class="lab">This control was set in Dolphin and brisk cannot show it. Pick a button to replace it.</p>{/if}
            {#if listening === sel}
              <p class="lab">{padNow ? 'Press a button on your pad.' : 'Waiting for a pad.'} Esc to cancel.</p>
              {#if hint}<p class="hint">{hint}</p>{/if}
              <button class="btn ghost" onclick={() => stopListen(true)}>Cancel</button>
            {:else}
              <button class="btn" onclick={() => startListen(sel)}>Listen for a button</button>
            {/if}
            {#if stickId}
              <div class="dz">
                <label for="dz" class="lab">Dead zone</label>
                <input
                  id="dz"
                  type="range"
                  min="0"
                  max={Math.max(40, dz)}
                  step="1"
                  value={dz}
                  aria-describedby="dz-h"
                  oninput={(e) => (dzLive[stickId] = e.currentTarget.valueAsNumber)}
                  onchange={(e) => commitDz(stickId, e.currentTarget.valueAsNumber)}
                />
                <span class="mono lab">{dz}%</span>
                <span id="dz-h" class="lab full">How far to push before Dolphin sees movement.</span>
              </div>
            {/if}
          </div>
          <button class="btn ghost" disabled={$st.restarting} onclick={() => ctl.reset()}>Reset to Xbox default</button>
        </div>
      </div>

      <p class="status" role="status">{line}</p>
      {#if $st.note?.tone === 'error'}
        <div class="err"><span role="alert">{$st.note.text}</span><button class="btn ghost sm" onclick={() => ctl.retry()}>Try again</button></div>
      {/if}

      {#if barShown}
        <div class="bar" role="status">
          {#if $st.restarting}
            <span>Restarting the game...</span>
          {:else if $st.restartError}
            <span role="alert">{$st.restartError}.</span>
          {:else}
            <span><b>Saved to the machine.</b> Dolphin reads the mapping at start, so restart the game to use it.</span>
          {/if}
          <button class="btn" disabled={$st.restarting} onclick={() => ctl.restart()}>{$st.restartError ? 'Try again' : 'Restart game to apply'}</button>
        </div>
      {/if}
    {/if}
  </section>
{/if}

<style>
  .sec { padding: 14px 22px 0; display: grid; gap: 10px; }
  h3 { margin: 6px 0 0; font-size: 13px; font-weight: 600; color: var(--muted); letter-spacing: .06em; text-transform: uppercase; }
  .lab { margin: 0; }
  .skel { aspect-ratio: 560 / 320; border-radius: 16px; background: var(--panel2); max-width: 640px; }
  .chip { margin: 0; display: flex; gap: 8px; align-items: center; font-size: 12.5px; color: var(--muted); }
  .press { font-family: var(--mono); color: var(--accent); }
  .grid { display: grid; grid-template-columns: minmax(0, 1.6fr) minmax(220px, 1fr); gap: 20px; align-items: start; }
  .dia { overflow-x: auto; }
  @media (max-width: 820px) { .grid { grid-template-columns: 1fr; } }
  .side { display: grid; gap: 10px; justify-items: start; }
  .card { display: grid; gap: 8px; justify-items: start; width: 100%; padding: 14px; border-radius: 16px; background: var(--panel2); box-shadow: 0 0 0 1px var(--line) inset; }
  .big { font-size: 20px; font-weight: 600; }
  .row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .bind { font-family: var(--mono); font-weight: 500; color: var(--accent); }
  .bind.custom { color: var(--warn); }
  .mark { font-size: 11px; font-weight: 600; padding: 1px 8px; border-radius: 999px; color: var(--warn); border: 1px solid var(--line); }
  .hint { margin: 0; color: var(--warn); font-size: 13px; }
  .dz { display: grid; grid-template-columns: auto 1fr auto; gap: 8px; align-items: center; width: 100%; }
  .dz .full { grid-column: 1 / -1; }
  .dz input { accent-color: var(--accent); min-width: 0; }
  .status { margin: 0; min-height: 1.4em; color: var(--muted); font-size: 13px; }
  .err { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; color: var(--bad); font-size: 13.5px; }
  .bar { display: flex; gap: 12px; align-items: center; justify-content: space-between; flex-wrap: wrap; padding: 10px 14px; border-radius: 14px; background: var(--panel2); box-shadow: 0 0 0 1px var(--line) inset; font-size: 13.5px; }
  .sm { padding: 4px 10px; font-size: 12.5px; min-height: 24px; }
  .btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
</style>
