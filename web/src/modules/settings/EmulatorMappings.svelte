<script lang="ts">
  // Emulator mappings (Settings, Controller tab): what the pad buttons do in Dolphin. Two ways, one mapping: click a
  // button on the pad drawing and pick its game function, or press "Change" on a game control and press a pad button.
  // Spec: docs/plans/2026-10-28-emulator-controls-v2.md, section 1.
  import { onMount, tick } from 'svelte';
  import { get } from 'svelte/store';
  import { padName, type PadLike } from '../../lib/gamepad';
  import { toast } from '../../shell/Toasts.svelte';
  import { api, type ControllerType } from '../../state/api';
  import Card from '../../ui/Card.svelte';
  import { isEmulatorGame } from '../library/library';
  import { padChoices, padView, type PadView } from './controllertest';
  import { Listener, wrongKind } from './emulators/listen';
  import { accepts, ACCEPTS, bind, CONTROL_SETS, controlOf, drawnLabels, padHolder, PAD_BY_INDEX, TYPE_LABEL } from './emulators/mapping';
  import { createMappings, SAVED, type State, type Target } from './emulators/store';
  import PadDiagram from './PadDiagram.svelte';
  import { PAD_LABEL, type PadName } from './pad.svg';

  /** Changes each time the page is opened at #controller/dolphin: scroll here and focus the heading. */
  let { focusKey = 0 }: { focusKey?: number } = $props();

  const TYPES: ControllerType[] = ['classic', 'gamecube'];
  const stores = { classic: createMappings('classic', { toast }), gamecube: createMappings('gamecube', { toast }) };
  const asked = new Set<ControllerType>();
  const noPad = { index: -1, id: '', connected: false, mapping: '', buttons: [], axes: [] } as PadLike;

  let root: HTMLElement | undefined = $state();
  let heading: HTMLElement | undefined = $state();
  let type = $state<ControllerType>('classic');
  let s = $state.raw<State>(get(stores.classic.state));
  $effect(() => stores[type].state.subscribe((v) => (s = v)));
  const ctl = $derived(stores[type]);

  // One selection: a game control row or a pad button, never both. `listener` is set while a row waits for a press.
  let selRow = $state<string | null>(null);
  let selPad = $state<PadName | null>(null);
  let listener = $state.raw<Listener | null>(null);
  let hint = $state('');
  let padNow = $state(''); // name of the connected pad ('' none)
  let view = $state.raw<PadView>(padView(noPad, 0));
  let dzLive = $state<Record<string, number | null>>({});
  let target = $state.raw<Target | null>(null);

  const draft = $derived(s.draft);
  const defs = $derived(CONTROL_SETS[type]);
  const labels = $derived(draft ? drawnLabels(type, draft.bindings) : {});
  const listening = $derived(listener?.control.id ?? null);
  const holder = $derived(selPad && draft ? padHolder(draft.bindings, selPad) : null);
  const current = $derived(selPad ?? (selRow && draft ? ((draft.bindings[selRow] as PadName | null) ?? null) : null));
  const offered = $derived(selPad ? defs.filter((c) => ACCEPTS[c.kind].includes(selPad!)) : []);
  // A stick is selected either way: its dead zone slider shows.
  const stickId = $derived.by(() => {
    const id = selRow ?? holder;
    return id && controlOf(type, id).kind === 'stick' ? id : null;
  });
  const dz = $derived(stickId ? (dzLive[stickId] ?? draft?.deadZone[stickId] ?? 0) : 0);
  const needTarget = $derived(!!s.data?.running && s.changed);

  const line = $derived(
    listening
      ? hint || (padNow ? `Listening for ${controlOf(type, listening).name}. Press a button on your pad.` : 'Waiting for a pad.')
      : [s.note?.tone === 'info' ? s.note.text : '', s.saved && !s.saving ? SAVED : ''].filter(Boolean).join(' '),
  );
  const barShown = $derived(s.restarting || !!s.restartError || needTarget);

  const nameOf = (id: string) => controlOf(type, id).name;
  const chipOf = (id: string) => (draft?.bindings[id] ? PAD_LABEL[draft.bindings[id] as PadName] : null);
  const focusSoon = (sel: string) => tick().then(() => root?.querySelector<HTMLElement>(sel)?.focus());

  function pickType(t: ControllerType) {
    if (t === type) return;
    stopListen();
    selRow = selPad = null;
    type = t;
    load(t);
  }
  function load(t: ControllerType) {
    if (asked.has(t)) return;
    asked.add(t);
    void stores[t].load();
  }
  function typeKeys(e: KeyboardEvent) {
    const d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!d) return;
    e.preventDefault();
    const t = TYPES[(TYPES.indexOf(type) + d + TYPES.length) % TYPES.length];
    pickType(t);
    focusSoon(`[data-type="${t}"]`);
  }

  function selectRow(id: string) {
    if (listener && listener.control.id !== id) stopListen();
    selRow = id;
    selPad = null;
  }
  function startListen(id: string) {
    selRow = id;
    selPad = null;
    hint = '';
    listener = new Listener(controlOf(type, id)); // replaces any listener before it
    focusSoon(`[data-change="${id}"]`); // the button turns into Cancel: keep focus on it
  }
  function stopListen(refocus = false) {
    const id = listener?.control.id;
    listener = null;
    hint = '';
    if (refocus && id) focusSoon(`[data-change="${id}"]`);
  }

  /** Bind through mapping.bind; the status line says what happened. */
  function bindTo(id: string, pad: string) {
    if (!draft) return;
    const c = controlOf(type, id);
    const r = bind(type, draft.bindings, id, pad);
    if (r.refused) return ctl.setNote(r.refused);
    if (r.bindings === draft.bindings) return;
    const other = r.swappedWith && nameOf(r.swappedWith);
    ctl.setBindings(r.bindings, {
      text: other ? `Swapped with ${other}. A pad button can only do one thing.` : `${c.name} is now on ${PAD_LABEL[pad as PadName]}.`,
      tone: 'info',
    });
  }

  /** A click or Enter on a drawn pad button. */
  function pickPad(pad: PadName) {
    if (listener) {
      const c = listener.control;
      if (!accepts(type, c.id, pad)) return void (hint = wrongKind(c, pad));
      stopListen(true);
      return bindTo(c.id, pad);
    }
    selPad = pad;
    selRow = null;
  }

  function heard(h: ReturnType<Listener['push']>) {
    if (h.type === 'wrong') hint = h.hint;
    else if (h.type === 'cancel') {
      stopListen();
      ctl.setNote('The pad was disconnected. Listening stopped.');
    } else if (h.type === 'bind' && listener) {
      const id = listener.control.id;
      stopListen(true);
      bindTo(id, h.pad);
    }
  }

  function commitDz(id: string, n: number) {
    ctl.setDeadZone(id, n);
    dzLive[id] = null;
  }
  function dzKeys(e: KeyboardEvent, id: string) {
    const d = { PageUp: 10, PageDown: -10 }[e.key];
    if (!d) return;
    e.preventDefault();
    const n = Math.min(50, Math.max(0, dz + d));
    dzLive[id] = n;
    commitDz(id, n);
  }

  // The pad loop: one rAF loop while the card is on screen and the page has focus. It lights the drawing and feeds the listener.
  let raf = 0;
  let seen = '';
  let inView = true;
  function frame() {
    raf = requestAnimationFrame(frame);
    const all = (navigator.getGamepads?.() ?? []) as (PadLike | null)[];
    const first = padChoices(all)[0];
    const g = first && all.find((p) => p?.connected && p.index === first.index);
    const v = (view = padView(g ?? noPad, 0));
    if ((first?.id ?? '') !== seen) padNow = (seen = first?.id ?? '') ? padName(seen) : '';
    if (listener) heard(listener.push(g ? v : null));
  }
  const active = () => inView && document.hasFocus() && document.visibilityState === 'visible';
  function sync() {
    if (active() && !raf) raf = requestAnimationFrame(frame);
    if (!active() && raf) {
      cancelAnimationFrame(raf);
      raf = 0;
      stopListen(); // leaving the page cancels
    }
  }

  async function findTarget() {
    try {
      const g = (await api.games()).games.find((x) => isEmulatorGame(x) && x.running);
      target = g ? { appid: g.appid, name: g.name } : null;
    } catch {
      target = null;
    }
  }
  $effect(() => {
    if (needTarget) void findTarget();
  });

  $effect(() => {
    if (focusKey > 0 && heading) {
      heading.scrollIntoView({ block: 'start' });
      heading.focus({ preventScroll: true });
    }
  });

  onMount(() => {
    asked.add('classic');
    void stores.classic.load();
    const ac = new AbortController();
    const on = (t: EventTarget, name: string, f: (e: never) => void) => t.addEventListener(name, f as EventListener, { signal: ac.signal });
    on(window, 'focus', () => (sync(), void ctl.refresh()));
    on(window, 'blur', sync);
    on(window, 'keydown', keydown);
    on(document, 'visibilitychange', sync);
    on(window, 'pointerdown', (e: PointerEvent) => {
      if (listener && !root?.contains(e.target as Node)) stopListen(); // a click elsewhere cancels
    });
    const io = root && new IntersectionObserver((es) => ((inView = es[es.length - 1].isIntersecting), sync()));
    if (root) io?.observe(root);
    sync();
    return () => {
      ac.abort();
      io?.disconnect();
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
    };
  });

  function keydown(e: KeyboardEvent) {
    // Listening: Esc cancels from anywhere. A pad prompt: only when focus is in the card.
    if (e.key !== 'Escape' || (!listener && !(selPad && root?.contains(e.target as Node)))) return;
    e.preventDefault();
    if (listener) return stopListen(true);
    const p = selPad;
    selPad = null;
    focusSoon(`[data-pad="${p}"]`);
  }
  function focusout(e: FocusEvent) {
    // Tab leaving the card cancels; a click on empty space inside it does not.
    const to = e.relatedTarget as Node | null;
    if (listener && to && !root?.contains(to)) stopListen();
  }
</script>

{#if s.load !== 'hidden'}
  <section bind:this={root} class="em" aria-labelledby="em-h" aria-busy={s.load === 'loading'} onfocusout={focusout}>
    <Card cols={2} class="emcard">
      <h4 id="em-h" tabindex="-1" bind:this={heading}>Emulator mappings</h4>

      <div class="head">
        <h5>Dolphin</h5>
        <div class="seg" role="radiogroup" aria-label="Controller type">
          {#each TYPES as t (t)}
            <button
              type="button"
              class="segb"
              role="radio"
              data-type={t}
              aria-checked={type === t}
              tabindex={type === t ? 0 : -1}
              onkeydown={typeKeys}
              onclick={() => pickType(t)}>{TYPE_LABEL[t]}</button>
          {/each}
        </div>
        <span class="chip">{padNow ? `Pad: ${padNow}` : 'No pad seen'}</span>
      </div>
      <p class="note first">These buttons are shared by every Dolphin game on this machine. The change is used the next time a game starts.</p>

      {#if s.load === 'loading'}
        <div class="grid" aria-hidden="true">
          <div class="skel dia"></div>
          <div class="skel rows"></div>
        </div>
        <p class="note">Loading the mapping...</p>
      {:else if s.load === 'error' || !draft}
        <div class="err"><span role="alert">{s.note?.text ?? 'Could not load the mapping.'}</span><button type="button" class="btn ghost sm" onclick={() => ctl.retry()}>Try again</button></div>
      {:else}
        {#if s.data?.source === 'default'}<p class="note">Using the default mapping. The first change creates the file.</p>{/if}
        {#if type === 'gamecube' && s.data?.portReady === false}<p class="note warn">Port 1 is not set to a standard controller. Saving a mapping fixes that.</p>{/if}

        <div class="grid">
          <div class="dia">
            <PadDiagram {view} dz={0} trails={['', '']} select={{ current, onpick: pickPad, labels }} />
            <div class="prompt">
              {#if selPad}
                <fieldset>
                  <legend>{holder ? `${PAD_LABEL[selPad]} does ${nameOf(holder)}.` : `${PAD_LABEL[selPad]} does nothing yet.`} Pick what it should do:</legend>
                  {#each offered as c (c.id)}
                    <label class="radio"
                      ><input
                        type="radio"
                        name="em-pick"
                        value={c.id}
                        checked={holder === c.id}
                        onclick={() => holder === c.id && (selPad = null)}
                        onchange={() => selPad && bindTo(c.id, selPad)} />{c.name}</label>
                  {:else}
                    <span class="note">Nothing to bind here.</span>
                  {/each}
                </fieldset>
              {:else}
                <p class="note">Click a button on the pad, or pick a control from the list.</p>
              {/if}
            </div>
          </div>

          <ul class="list" aria-label="Game controls">
            {#each defs as c (c.id)}
              {@const chip = chipOf(c.id)}
              {@const mine = listening === c.id}
              <li class="item" class:on={selRow === c.id || (!!selPad && holder === c.id)} class:listen={mine}>
                <button type="button" class="nm" aria-pressed={selRow === c.id} onclick={() => selectRow(c.id)}>{c.name}</button>
                <span class="bind" class:custom={chip === null} class:wait={mine}>
                  {#if mine}{padNow ? 'Press a button on your pad' : 'Waiting for a pad. You can also click a button on the drawing.'}{:else}{chip ?? 'Custom'}{/if}
                </span>
                {#if mine}
                  <button type="button" class="btn ghost sm" data-change={c.id} aria-label="{c.name}, listening. Press a button on your pad. Escape to cancel." onclick={() => stopListen(true)}>Cancel</button>
                {:else}
                  <button type="button" class="btn ghost sm" data-change={c.id} aria-label={chip ? `${c.name}, bound to ${chip}. Press Enter to change.` : `${c.name}, custom. Press Enter to change.`} onclick={() => startListen(c.id)}>Change</button>
                {/if}
                {#if chip === null && !mine}<span class="sub">Set in Dolphin. Pick a button to replace it.</span>{/if}
                {#if mine && hint}<span class="sub hint">{hint}</span>{/if}
              </li>
            {/each}
          </ul>
        </div>

        {#if stickId}
          <div class="dz">
            <label for="em-dz">Dead zone</label>
            <input
              id="em-dz"
              type="range"
              min="0"
              max={Math.max(40, dz)}
              step="1"
              value={dz}
              aria-describedby="em-dz-h"
              oninput={(e) => (dzLive[stickId] = e.currentTarget.valueAsNumber)}
              onchange={(e) => commitDz(stickId, e.currentTarget.valueAsNumber)}
              onkeydown={(e) => dzKeys(e, stickId)} />
            <span class="mono">{dz}%</span>
            <span id="em-dz-h" class="note full">How far to push before Dolphin sees movement.</span>
          </div>
        {/if}

        <div class="foot">
          <button type="button" class="btn ghost" disabled={s.restarting} onclick={() => ctl.reset()}>Reset to default</button>
          <p class="status" role="status">{line}</p>
        </div>
        {#if s.note?.tone === 'error'}
          <div class="err"><span role="alert">{s.note.text}</span><button type="button" class="btn ghost sm" onclick={() => ctl.retry()}>Try again</button></div>
        {/if}

        {#if barShown}
          <div class="bar" role="status" aria-label="Restart">
            {#if s.restarting}
              <span>Restarting the game...</span>
            {:else if s.restartError}
              <span role="alert">{s.restartError}.</span>
            {:else}
              <span><b>Saved.</b> Dolphin reads the mapping at start, so restart the game to use it.</span>
            {/if}
            {#if target}<button type="button" class="btn" disabled={s.restarting} onclick={() => target && ctl.restart(target)}>{s.restartError ? 'Try again' : 'Restart game to apply'}</button>{/if}
          </div>
        {/if}
      {/if}
    </Card>
  </section>
{/if}

<style>
  .em { margin-top: 14px; }
  .em :global(.emcard) { max-width: 1000px; }
  .em :global(.emcard:hover) { transform: none; }
  h5 { margin: 0; font-size: 15px; font-weight: 600; }
  #em-h:focus-visible { outline: 2px solid var(--accent); outline-offset: 4px; border-radius: 4px; }
  .head { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 16px; margin-top: 12px; }
  .chip { margin-left: auto; font-size: 12.5px; color: var(--muted); }
  .seg { display: inline-flex; border: 1px solid var(--line); border-radius: 999px; overflow: hidden; }
  .segb { padding: 5px 14px; border: 0; background: transparent; color: var(--muted); font: inherit; font-size: 13px; cursor: pointer; }
  .segb[aria-checked='true'] { background: color-mix(in srgb, var(--accent) 20%, transparent); color: var(--fg); box-shadow: inset 0 0 0 1px var(--accent); border-radius: 999px; }
  .segb:focus-visible, .nm:focus-visible, input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .note { margin: 8px 0 0; font-size: 12px; color: var(--muted); }
  .note.first { margin-top: 10px; }
  .note.warn { color: var(--warn); }
  .grid { display: grid; grid-template-columns: minmax(0, 1.5fr) minmax(240px, 1fr); gap: 20px; margin-top: 14px; align-items: start; }
  @media (max-width: 760px) { .grid { grid-template-columns: 1fr; } }
  .skel { border-radius: 16px; background: var(--panel2); }
  .skel.dia { aspect-ratio: 160 / 120; }
  .skel.rows { min-height: 220px; }
  .prompt { margin-top: 8px; min-height: 3em; }
  fieldset { margin: 0; padding: 10px 12px; border: 1px solid var(--line); border-radius: 12px; display: flex; flex-wrap: wrap; gap: 6px 14px; }
  legend { padding: 0 6px; font-size: 13px; }
  .radio { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; cursor: pointer; }
  .radio input { accent-color: var(--accent); }
  .list { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
  .item { display: grid; grid-template-columns: minmax(70px, 1fr) minmax(0, 1.4fr) auto; align-items: center; gap: 4px 10px; padding: 4px 8px; border-radius: 10px; }
  .item.on { background: color-mix(in srgb, var(--accent) 14%, transparent); box-shadow: inset 0 0 0 1px var(--accent); }
  .nm { justify-self: start; padding: 2px 4px; border: 0; background: none; color: var(--fg); font: inherit; font-size: 13.5px; cursor: pointer; text-align: left; border-radius: 6px; }
  .bind { font-family: var(--mono); font-size: 12.5px; color: var(--accent); overflow-wrap: anywhere; }
  .bind.custom { color: var(--warn); }
  .bind.wait { font-family: var(--font); color: var(--fg); }
  .item.listen { animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { background: color-mix(in srgb, var(--accent) 26%, transparent); } }
  @media (prefers-reduced-motion: reduce) { .item.listen { animation: none; } }
  .sub { grid-column: 1 / -1; font-size: 12px; color: var(--muted); }
  .sub.hint { color: var(--warn); }
  .dz { display: grid; grid-template-columns: auto minmax(0, 260px) auto; gap: 8px 12px; align-items: center; margin-top: 14px; font-size: 13px; }
  .dz input { accent-color: var(--accent); min-width: 0; }
  .dz .full { grid-column: 1 / -1; margin: 0; }
  .foot { display: flex; flex-wrap: wrap; align-items: center; gap: 10px 16px; margin-top: 14px; }
  .status { margin: 0; min-height: 1.4em; color: var(--muted); font-size: 13px; }
  .err { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; margin-top: 10px; color: var(--bad); font-size: 13.5px; }
  .bar { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; justify-content: space-between; margin-top: 14px; padding: 10px 14px; border-radius: 14px; background: var(--panel2); box-shadow: 0 0 0 1px var(--line) inset; font-size: 13.5px; }
  .sm { padding: 4px 10px; font-size: 12.5px; min-height: 24px; }
  .btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
</style>
