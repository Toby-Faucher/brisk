# Emulator mappings editor, v2 (design)

Status: design, replaces the approach of `2026-10-27-emulator-controls.md`. Branch `emulator-launch`.
Anything marked "my call" is a decision I made so nobody has to ask. Anything marked "unverified" must be
checked against a file Dolphin wrote before it is relied on.

## 0. What changes from v1

Toby tested v1 and the new Classic Controller drawing looked bad. He had asked for the controller he built in
Settings (the `PadDiagram` drawing of his real pad). His answers:

1. Two ways, one mapping. Click a button on the pad drawing to choose what it does in the game, or pick a game
   control from a list and press a pad button. Both views show the same mapping.
2. The editor lives in Settings, Controller tab, as "Emulator mappings", one mapping per emulator. Dolphin's
   file is global to all Dolphin games, so per-game is misleading. The game panel shows a short summary and a link.
3. Controller types now: Classic Controller (done in v1) and GameCube controller. Later: Nunchuk, Wii Remote
   sideways, other emulators, per-game profiles.

What stays from v1 (it was built and tested): the line-based ini rewriter, allowlist tables, `rev`/409, 4 KiB
cap, one-time backup, atomic 0600 write, `running` and `appliesOn`, the Xbox-layout pad names, the first-press
listening rules, swap on conflict, autosave, the restart flow. What goes: `ClassicDiagram.svelte` and its
hotspot table, the Controls section in `GameDetail`, and the per-game routes.

## 1. UI

### 1.1 Where

Settings, Controller tab (`ControllerTest.svelte` stays as it is). A new card below the live test, titled
**Emulator mappings** (`EmulatorMappings.svelte`, new, in `modules/settings/`). My call: below, not above, because
the live test is the page's main job and most visits are for that. The tab hash `#controller` already exists;
the card gets the anchor `#controller/dolphin` (see 1.9).

One emulator per section. Today there is one section: **Dolphin**, with a one-line intro: "These buttons are
shared by every Dolphin game on this machine. The change is used the next time a game starts." When a second
emulator exists, each gets its own section (heading plus the same editor); no tabs until then.

### 1.2 Layout (top to bottom, inside the card)

1. Header row: "Dolphin" heading, the **type switch** (segmented control, two options: `Classic Controller`,
   `GameCube controller`), and on the right a pad chip: "Pad: {name}" or "No pad seen".
2. The editor, two columns on wide screens (diagram left, list right), one column on phones (diagram first):
   - **Diagram**: the existing pad drawing, made selectable (1.3).
   - **List**: the game controls of the chosen type (1.5), one row each: game control name, then the bound pad
     button as a chip, then "Change" button.
3. Under the editor, a side strip that depends on the selection: for a stick control, the Dead zone slider.
4. Footer row: "Reset to default" (ghost button), the status line (polite live region).
5. Restart bar (only when `running`): "Saved. Dolphin reads the mapping at start, so restart the game to use
   it." and the primary button "Restart game to apply". Needs a game to restart: it uses the running Dolphin
   game's appid, found with the existing `api.games()` and its running flag; when none is found the bar has
   only the text.

Empty and error states are in 1.8.

### 1.3 Reusing PadDiagram: what it exposes, what to extend

What it is today: `PadDiagram.svelte` takes `{view, dz, trails}`. It draws the 8BitDo to scale from `pad.svg.ts`
in a `<svg aria-hidden>`. Each drawn standard button is a `<g id="pad-..." data-button={index}>` with class
`key` and `.on` when `view.buttons[index].pressed`. The labels it draws: `A B X Y`, `LB RB`, `LT RT` (with the
analog percent), `Back`, `Start`, `Home`, `L3`, `R3` (the sticks, which also move), and four d-pad arms
`up down left right`. `ControllerTest.svelte` reads state with `navigator.getGamepads()` in a
`requestAnimationFrame` loop and turns a pad into a `PadView` with `padView(g, deadzone)` from
`controllertest.ts`. Faint dashed shapes (L4/R4, paddles, Square, Star) are not exposed to browsers and are not
selectable.

Extension (small, additive, my call: extend `PadDiagram`, do not fork it). New optional props, all default off,
so the Controller test looks and behaves exactly as today:

- `select?: { current: string | null; onpick: (pad: PadName) => void; labels: Record<PadName, string>; hint?: string | null }`.
  When present the diagram is interactive: `aria-hidden` is dropped from the svg (it gets `role="group"` and
  `aria-label="Pad buttons"`), and every pad element that maps to a pad name gets `role="button"`, `tabindex="0"`,
  `aria-pressed`, an `aria-label` (1.7) and click/Enter/Space handlers. Pointer area: add an invisible
  `fill: transparent; stroke: transparent; stroke-width: 6` hit rect/circle copy so small buttons (Back, Start)
  are easy to hit on a phone.
- Pad element to pad name, a pure table in `pad.svg.ts` (`PAD_NAME_BY_ID`), keyed by the existing ids:
  `pad-a A`, `pad-b B`, `pad-x X`, `pad-y Y`, `pad-lb LB`, `pad-rb RB`, `pad-lt LT`, `pad-rt RT`,
  `pad-back View`, `pad-start Menu`, `pad-home Guide`, `pad-l3 Left stick`, `pad-r3 Right stick`,
  `pad-up/down/left/right D-pad` (all four arms select the same `D-pad`; the whole cross highlights together).
  The API keeps v1's names; the drawing keeps its own printed labels (Back, Start, Home). The list chips use the
  drawing's printed labels too (`Back`, `Start`, `Home`), so what he sees in the list matches the drawing.
  `PAD_LABEL: Record<PadName, string>` in the same file does that translation (`View` shows as `Back`, `Menu`
  as `Start`, `Guide` as `Home`). L3 and R3 are labelled "Left stick" and "Right stick" in the list chips,
  since in a mapping they mean "the stick" (moving it), not the click.
- Per-button bound label: `labels[pad]` is the game function bound to that pad button ("A", "ZL", "Z",
  "Main stick"...). It is drawn as a second small text under (or beside) the pad's own letter, at 2.6 units
  (about the size of the existing LB/RB text), in `--accent` colour. An unbound button draws nothing there and
  its outline is dashed-light (see 1.6).
- Selected style: `.sel :is(path,circle) { stroke: var(--accent); stroke-width: 2.2px }`, plus the focus ring on
  keyboard focus (`:focus-visible`, never removed). Selected is not the same as pressed.
- Highlight of the pressed button: already there (`.on`, from `view`). In select mode the same `view` is passed
  from the pad loop, so pressing a physical button lights its drawn twin. Cheap and it is the proof that the
  right pad is being read.
- `ControllerTest.svelte` exports nothing new. To avoid a second polling loop, `EmulatorMappings.svelte` runs its
  own small loop (`requestAnimationFrame` while the card is on screen and the page is visible, the same guard
  `ControllerTest` uses with an `IntersectionObserver`) and builds the same `PadView` with `padView` and
  `padChoices`. Two loops reading `getGamepads()` are harmless.
- The `dz` and `trails` props: pass `dz={0}` and empty trails in select mode (no dead zone ring, no trails).

### 1.4 The two ways, one mapping

State: `sel` is either a game control id or a pad name (never both); one `mode`: `idle | listen | pick`.

Pad-first (click a pad button):

1. The user clicks (or Enters on) a drawn pad button, say `LB`. It gets the selected style. The list on the right
   highlights the row currently bound to LB (if any) and a small prompt appears under the diagram: "LB does
   {Z}. Pick what it should do:" followed by a radio group of the game controls that accept it (kind match,
   1.6), the current one checked. For an unbound button the prompt is "LB does nothing yet. Pick what it should
   do:" with none checked.
2. Choosing a control binds that control to LB (swap rules in 1.6) and saves. Esc or choosing the same control
   closes the prompt.

List-first (pick a game control):

1. The user clicks "Change" on a row, say `ZL`. The row is selected, its chip reads "Press a button on your
   pad", and the diagram highlights the pad button currently bound to it. Listening starts (1.5).
2. The first new press (or, on touch screens or with no pad, a click on a drawn pad button, which also works as
   the answer) binds it. My call: while a row is in "Change" mode, a click on the drawn pad is accepted as the
   answer, so the whole thing also works with a mouse and no pad connected. This is why the drawing is
   clickable in both modes.

Clicking a drawn pad button when no list row is waiting always means pad-first. Clicking a drawn pad button
while a row is waiting means "bind this row to it". Clicking a row's name (not "Change") selects it and
highlights its pad button without listening.

Both views always read the same `bindings` object; no second state.

### 1.5 Game controls per type and listening rules

Classic Controller (14 controls; labels are the real Classic Controller names): `A B X Y`, `L`, `R`, `ZL`, `ZR`,
`-`, `+`, `Home`, `Left stick`, `Right stick`, `D-pad`. Ids: `a b x y l r zl zr minus plus home ls rs dp`.
The list order is: face buttons, shoulders (`L R ZL ZR`), `- Home +`, then sticks and d-pad.

GameCube controller (11 controls): `A B X Y`, `Z`, `Start`, `L` (analog trigger), `R` (analog trigger),
`Control stick`, `C stick`, `D-pad`. Ids: `a b x y z start l r ms cs dp`. (Dolphin calls the left stick the
"Main Stick"; the real pad calls it the control stick, so the UI uses that.)

Listening (kept from v1, wording tightened):

- Start: take a baseline of the pad. A button held at start, or a stick already pushed, is ignored until it has
  been released once.
- The first button that goes from released to pressed wins. Triggers count above 0.5. The d-pad (indexes 12 to
  15) answers `D-pad`. Index 16 is `Guide`; 10 and 11 (L3, R3) answer nothing.
- Sticks: a push past 0.6 from centre answers with that stick, and it must fall below 0.3 before it can answer
  again. A gentle stick (under 0.6) never answers, so drift cannot bind.
- Two answers in one frame: the lowest index wins, stably.
- Wrong kind is ignored and the row says why and keeps listening: "That is a stick. A takes a button."
- Esc cancels (from anywhere in the card) and returns focus to the row's Change button. The Cancel button, a
  click elsewhere, Tab leaving the card, the page losing visibility, or the pad disconnecting also cancel.
- Starting a new listen cancels the old one. No timeout: it waits until cancelled (nothing is lost by waiting).
- If no pad is connected: the chip reads "Waiting for a pad. You can also click a button on the drawing." and the
  click path (1.4) still works.

Dead zone: shown in the side strip when a stick row is selected (either way of selecting). Label "Dead zone",
range 0 to 40 step 1 with a readout like "12%" (the API allows 0 to 50; the slider stops at 40 as in v1).
Hint: "How far to push before Dolphin sees movement." The readout updates while dragging, the save fires on
`change`. The default is 0. Keyboard: arrow keys move by 1, PageUp/PageDown by 10.

### 1.6 One-to-one, swap, unbound, kind match

- A pad button drives at most one game control. A game control has exactly one pad button (or is "Custom").
- Binding control `c` to pad button `p` when another control `d` holds `p`: `d` takes `c`'s old pad button
  (swap). Status line: "Swapped with ZL. A pad button can only do one thing." Same PUT carries both.
- When `p` is free: `c` takes it and its old pad button becomes unbound. No message beyond "{c} is now on {p}."
- GameCube has 11 pad names for 8 button-like controls plus 3 groups, so some pad buttons end up unbound
  (default: Back, Home, LB). Classic uses all 11, so none are unbound there unless the user binds a stick to
  the other stick, which is allowed.
- If `c` is "Custom" (no known pad button) and `p` is held by `d`: refuse, message "Back is used by ZL. Change
  ZL first." (a swap would leave `d` with nothing). My call: this is rare enough that refusing is the simplest
  honest answer.
- Kind match: buttons take the 11 button names, stick controls take only `Left stick` / `Right stick`, the
  d-pad control only `D-pad`. In pad-first mode the radio list only offers controls that accept the chosen pad
  button; a d-pad arm offers only the D-pad control, an L3/R3 hotspot offers the stick controls (and the d-pad
  control is not offered for sticks). A button that no control of this type accepts shows "Nothing to bind
  here." (cannot happen with the tables below, but the empty case is defined).
- How an unbound pad button is shown: outline in the dashed `--dim` style used for the faint extras, no bound
  label, and in pad-first mode the prompt says "does nothing yet". The list has no row for it (the list is
  game controls, not pad buttons). A tooltip/`aria-label` says "{name}, not used".
- Custom controls (the ini holds something brisk cannot show): the row chip reads "Custom" in the warn colour
  with the sentence "Set in Dolphin. Pick a button to replace it." Untouched until replaced.

### 1.7 Keyboard and screen reader

- The type switch is a `role="radiogroup"` with two radios, arrow keys move.
- Diagram: 14 or fewer drawn pad elements are `role="button"` in DOM order that matches a sensible tab order
  (face, shoulders, middle, sticks, d-pad). Names: "A button, does B. Press Enter to change." or "LB, not
  used. Press Enter to choose what it does." The list rows are plain buttons: "ZL, bound to LB. Press Enter
  to change." While listening: "ZL, listening. Press a button on your pad. Escape to cancel."
- A pad button with the selected style has `aria-pressed="true"`; the radio prompt in pad-first mode is a real
  `<fieldset>` with `<legend>` and `<input type="radio">`s (native arrow-key behaviour).
- One polite `role="status"` region announces: listening started, "{control} is now on {pad}", the swap line,
  Saved, Reset. Errors use `role="alert"`.
- Pressing a physical pad button also moves nothing in the DOM focus (it only highlights the drawn twin and
  answers a listen).
- No colour-only meaning: selected has a thicker stroke, unbound has a dashed outline, custom has the word.
  `prefers-reduced-motion` stops the listening pulse; the text "Press a button on your pad" stays.
- Contrast 4.5:1 or more for the bound labels on `--panel2`.

### 1.8 States

| State | What shows |
|---|---|
| loading | Card with the pad drawing in grey and the list as skeleton rows, `aria-busy` |
| idle | Nothing selected; prompt line: "Click a button on the pad, or pick a control from the list." |
| pad-selected | Selected style on the pad, radio prompt (1.4) |
| row-selected | Row highlighted, its pad button highlighted, "Change" button |
| listening | Row chip "Press a button on your pad", Cancel button, pulse on the row |
| saved | Status: "Saved. Used the next time a game starts." |
| restart-needed | The restart bar (1.2), only when `running` |
| restarting | "Restarting the game..." with the button disabled |
| conflict (409) | "Another device changed the mapping. Showing the latest." |
| load error | In the card: "Could not load the mapping." and Try again |
| save error | "Could not save the mapping." The chips return to the last confirmed values, Try again |
| no Dolphin | When GET answers 404 (hostd has no Dolphin entry in `emulators.json`): the Dolphin section is not shown at all. If no emulator section remains, the whole card is hidden. |
| no pad seen | Pad chip: "No pad seen"; clicking the drawing still works, listening waits |
| default (no file) | A small note under the heading: "Using the default mapping. The first change creates the file." |
| not ready (GameCube) | When `portReady` is false: "Port 1 is not set to a standard controller. Saving a mapping fixes that." |

Reset to default: no confirmation (the original file is backed up once and re-binding is easy). It asks the
server (`POST .../reset`), clears custom controls and dead zones, and shows "Reset to the default." Strings:
Reset to default, Restart game to apply, Try again, Cancel, Change, Dead zone.

### 1.9 Summary card in the game panel and the link

Replace the `Controls` section of `GameDetail.svelte` (`ControlsPanel`, deleted) with `ControlsSummary.svelte`,
shown for `isEmulatorGame(game)` when the first GET of the active type did not answer 404.

My call on content: a short card, no editing. Text: "Dolphin uses one mapping for every game on this machine.
Wii games use the Classic Controller mapping. GameCube games use the GameCube controller mapping." and a link
"Edit mappings". No per-button list: `emulators.json` has no platform field (unverified), so the card cannot
know which type a game uses. If an entry later gets a platform, the card can show that type's main buttons.
It is shown when the first GET of either type did not answer 404.

Link target: `/settings#controller/dolphin` (route is the Settings page; the hash is `#controller` plus a sub
anchor). `tabs.ts` `parseHash` today knows only the tab name, so it needs: split the hash at `/`, use the first
part as the tab, and pass the rest to the card, which scrolls itself into view and focuses its heading on
mount. A hash with an unknown sub part opens the tab and ignores the rest. With a machine hub the link keeps
the current machine (the app's route state, not part of the hash).

## 2. Backend

### 2.1 How Dolphin is configured (sources and what is unverified)

Primary sources: Dolphin's source tree (`github.com/dolphin-emu/dolphin`, files below) and its documented
config layout.

**GameCube pad.** File `GCPadNew.ini` in the Dolphin config directory, section `[GCPad1]` for port 1.
The groups and controls, from `Source/Core/Core/HW/GCPadEmu.cpp` (verified in the constructor):

- group Buttons: controls `A`, `B`, `X`, `Y`, `Z`, `Start` (the Start label is "START" in the UI)
- group Main Stick ("Control Stick"), group C-Stick ("C Stick"): octagonal analog sticks
- group Triggers: `L`, `R` (digital), `L-Analog`, `R-Analog`
- group D-Pad
- group Rumble, with output `Motor`; group Options, with `Always Connected`

Ini key names are `Group/Control`, same scheme as the Wii Remote file in this repo. So the keys brisk writes:

| Control | Keys |
|---|---|
| A, B, X, Y, Z, Start | `Buttons/A`, `Buttons/B`, `Buttons/X`, `Buttons/Y`, `Buttons/Z`, `Buttons/Start` |
| Control stick | `Main Stick/Up`, `Main Stick/Down`, `Main Stick/Left`, `Main Stick/Right` |
| C stick | `C-Stick/Up`, `C-Stick/Down`, `C-Stick/Left`, `C-Stick/Right` |
| L, R | `Triggers/L` and `Triggers/L-Analog`; `Triggers/R` and `Triggers/R-Analog` (the pair is written together, as for Classic L/R in v1) |
| D-pad | `D-Pad/Up`, `D-Pad/Down`, `D-Pad/Left`, `D-Pad/Right` |
| dead zone | `Main Stick/Dead Zone`, `C-Stick/Dead Zone` |
| device | `Device` (left alone, 2.5) |

Unverified: the exact ini key strings for the sticks, `Dead Zone` and `L-Analog` (the constructor shows the
control names, but the key text lives in `GCPadEmu.h` and `ControllerEmu`'s stick group, which I did not read).
They match what I remember from Dolphin-written `GCPadNew.ini` files. The same unverified note applies to the
dead zone number format (`12.000000`, as in v1). Task A must write a mapping once from Dolphin's own GameCube
controller window on the games VM, read the file, and fix this table first. Everything else in the module is
table-driven, so a wrong key name is a one-line fix.

**Telling Dolphin to use it.** `Dolphin.ini`, section `[Core]`, key `SIDevice0` is port 1; value `6` is a
standard controller (`SIDEVICE_GC_CONTROLLER`, the seventh entry of the `SIDevices` enum in
`Source/Core/Core/HW/SI/SI_Device.h`; verified from the enum order: none 0, N64 mic 1, N64 keyboard 2, N64 mouse
3, N64 controller 4, GBA 5, GC controller 6). Unverified: whether a fresh Dolphin config already defaults port
1 to a controller (I believe so for desktop builds), and the key text `SIDevice0` (memory of Dolphin-written
files). That is why brisk writes the line itself when it saves a GameCube mapping (2.3) instead of relying on the default.

**Wii Remote path (Classic).** Unchanged from v1: `WiimoteNew.ini`, `[Wiimote1]`, `Extension = Classic`,
`Source = 1`, the `Classic/*` keys plus the basic Wii Remote mirrors, the known-good file is
`deploy/dolphin/WiimoteNew.ini`.

**Which pad Dolphin uses for which game.** My call: both configurations are always live. Dolphin uses the
Wii Remote 1 setup for Wii titles and the GameCube port 1 setup for GameCube titles (and for Wii titles that
accept a GameCube pad, which is a game feature). So there is no "active controller type" for brisk to choose or
write. The type switch in the UI only picks which one you are editing. (A switch that "activates" a type would
promise something Dolphin does not do per game.) What brisk does write for GameCube is the `SIDevice0 = 6` line, once,
so the port is ready.

**Xbox-layout expressions** (verified live in v1 on the streamed pad, same device string
`evdev/0/Sunshine (libvirtualhid) X-Box Series Controller`). Pad name to Dolphin expression and group axes are
exactly the v1 tables in `controls.rs` (`button_expr`, `group_exprs`): `A SOUTH`, `B EAST`, `X WEST`, `Y NORTH`,
`LB TL`, `RB TR`, `LT` `Full Axis 2+`, `RT` `Full Axis 5+`, `View SELECT`, `Menu START`, `Guide MODE`; left stick
axes 0 and 1, right stick axes 3 and 4, d-pad hat axes 6 (left, right) and 7 (up, down). Values with a space are
written in backticks.

### 2.2 Default mappings

Classic: as in v1 (`a:B b:A x:Y y:X l:LT r:RT zl:LB zr:RB minus:View plus:Menu home:Guide ls:Left stick
rs:Right stick dp:D-pad`), equal to `deploy/dolphin/WiimoteNew.ini`.

GameCube (my call, position based like the real pad's diamond, not like Wii's letter swap): `a:A b:X x:B y:Y
z:RB start:Menu l:LT r:RT ms:Left stick cs:Right stick dp:D-pad`. Unbound: View, Guide, LB. Notes: GameCube B
is on the lower left of A, which on the Xbox diamond is X; GameCube X is to the right of A, which is Xbox B.
Rationale: muscle memory from the real pad. If Toby prefers letter-for-letter, only this table changes.
The repo gets a new known-good file `deploy/dolphin/GCPadNew.ini` (the `[GCPad1]` block with the default
above and the same `Device` line, no `Rumble/Motor` key), embedded with `include_str!` like the Wii one.

### 2.3 Rewriting and the second file

- `controls.rs` becomes generic over a `Kind` (Classic, GameCube): a static `Layout` per kind holds the file
  name, section header, control ids, `keys(id)`, the accepts rules, the default text, the dead zone keys and the
  backup name. The rewriter (split, section, norm, read, apply, atomic write) is shared and unchanged.
- Files: classic: `WiimoteNew.ini`, section `[Wiimote1]`, backup `WiimoteNew.ini.before-brisk-edit` (same as v1);
  gamecube: `GCPadNew.ini`, section `[GCPad1]`, backup `GCPadNew.ini.before-brisk-edit`.
- GameCube controls: `a b x y z start l r ms cs dp`. `ms` and `cs` accept the two stick names, `dp` accepts
  `D-pad`, the other eight accept the 11 button names. Pairing is injective, not bijective (3 free names).
- Port line: on every GameCube PUT and reset, after the pad file is written, ensure `Dolphin.ini`
  `[Core]` has `SIDevice0 = 6`: replace the value of an existing `SIDevice0` line, or append the line in
  `[Core]` (add the section if missing). Same rewriter, same atomic write, backup `Dolphin.ini.before-brisk-edit`
  made once before the first write. Nothing else in `Dolphin.ini` is touched, and `Dolphin.ini` is never created
  if absent (Dolphin builds it on first run; if it is absent hostd skips the port line and reports
  `portReady: false`; my call). If the pad file write succeeds and the port write fails, answer 500 and the
  next GET shows `portReady: false`.
- `portReady` (GameCube responses only): true when `Dolphin.ini` has `SIDevice0 = 6`.
- Running: Dolphin rewrites its ini files on exit from memory, so edits made while it runs can be overwritten.
  Same rule and same restart flow as v1; it applies to `Dolphin.ini` too. `running` is true when any Dolphin
  process runs (`emu::pids` over all Dolphin entries).
- Missing file: the type's embedded default is shown as `source: "default"`, `rev: "none"`. The first PUT or
  reset creates the file (dir 0700, file 0600) from the embedded text, then applies the edit. A file with no
  section gets the embedded section appended. An existing file is never replaced and `Device` is never touched.
- Security as in v1: values come only from fixed tables selected by validated names, or a number 0 to 50. User
  text never reaches a file. Keys come from the layout, never from the request.

### 2.4 API (my call: per emulator, type in the path)

All under the existing `/api` guard (Host allowlist, then secret or allowed tailnet login). Nothing new for auth.

| Method and path | Body | Answers |
|---|---|---|
| `GET /api/emulators/dolphin/controls/{type}` | none | 200, the object below |
| `PUT /api/emulators/dolphin/controls/{type}` | PUT body | 200, the object after the write |
| `POST /api/emulators/dolphin/controls/{type}/reset` | none (a body is ignored) | 200, the object after the write |

`{type}` is `classic` or `gamecube`; anything else is 404 `{"error":"no such controller type"}`. Why the type
in the path and not a query: the type picks the file, so it is part of the resource, and a bad type is a 404
naturally. Why per emulator: the file is global. `dolphin` is a fixed path segment, so a second emulator later
is a sibling route with its own adapter.

Gate: the machine must have at least one `emulators.json` entry whose emulator is `dolphin`; otherwise 404
`{"error":"no controls on this machine"}` (the web hides the section on 404). This replaces v1's per-appid check
(`emu::spec(...).controls`). `emu::pids` supplies `running` as before.

Response (see the fixtures `v2-classic-get.json`, `v2-gamecube-get.json`):

```json
{
  "emulator": "dolphin",
  "type": "gamecube",
  "controller": "GameCube controller, port 1",
  "source": "default",
  "rev": "none",
  "running": false,
  "appliesOn": "next-launch",
  "portReady": false,
  "bindings": { "a": "A", "...": "..." },
  "deadZone": { "ms": 0, "cs": 0 },
  "custom": { "a": false, "...": "..." }
}
```

- `bindings`, `custom`, `source`, `rev`, `running`, `appliesOn` mean what they meant in v1.
- `deadZone` keys are `ls`, `rs` for classic and `ms`, `cs` for gamecube. `portReady` is present only for gamecube.
- A pad name that no control holds is simply absent from `bindings` values (the web derives "unbound").
- PUT body: `{ "rev"?: string, "bindings": {...all controls of the type...}, "deadZone": {...} }`; fixtures
  `v2-classic-put.json`, `v2-gamecube-put.json`. Validation is v1's, per type: `deny_unknown_fields`, exactly the
  type's control ids, values in the allowlist or `null` only for a currently custom control, no pad name used
  twice, dead zone integers 0 to 50 for exactly the type's two keys, `rev` a string of at most 32 chars. First
  failure wins, 400 `{"error": "<short reason>"}`, never echoing input.
- Codes: 409 `{"error":"changed elsewhere","current":{...}}` on a stale `rev` (rev is the hash of the type's pad
  file only); 413 over 4 KiB (the body cap applies to PUT and reset); 500 `{"error":"could not save controls"}`
  on IO or a non-UTF-8 file (nothing changes); 405 from axum.
- Concurrency: one process-wide `Mutex` over the read, edit, write sequence (both files for a GameCube save),
  one-time backups, 0600 temp file in the same dir plus `rename`. `BRISK_DOLPHIN_DIR` as in v1; symlinked files
  are resolved with `canonicalize` first.

**The old route.** Remove `/api/games/{appid}/controls` and `/api/games/{appid}/controls/reset` outright: v1
is deployed on the games VM but unreleased and nothing else calls it, so a redirect or alias would be dead
weight. Remove `controls_gate`, the `controls: bool` flag on `emu::spec` (replace with "any dolphin entry"),
and the web's `api.controls/putControls/resetControls` per-appid methods. The deployed v1 binary keeps working
until the next deploy replaces both halves together (web and hostd ship as one update, so there is no window
where one half is old).

**Hub proxy.** The web client's `route()` scopes only paths that match `SCOPED` in `state/api.ts`. Add
`emulators` to that regex, so `/api/emulators/dolphin/controls/classic` becomes
`/api/m/{id}/emulators/dolphin/controls/classic`. The hub forwards `/api/m/{id}/*` to the machine's hostd
with the secret; whether the hub has its own path allowlist is unverified (check the hub proxy code in Task A's
read-only pass and add `emulators` if it has one). Add a route test in `state/api.test.ts` for the scoped path.

## 3. Tests

Follow the repo layers. Shared fixtures (created now, in
`crates/brisk-hostd/tests/fixtures/controls/`): `v2-classic-get.json`, `v2-classic-put.json`,
`v2-gamecube-get.json`, `v2-gamecube-put.json`. The v1 `get.json` and `put.json` are deleted by the
implementers when the v1 code goes.

### Rust (hostd)

Unit tests in `controls.rs`, for both layouts (parameterised over `Kind`):

- Round trip every control and every accepted name; the four stick keys and the mirrors (classic only) hold the
  expected expressions.
- Classic default applied to an empty section equals `deploy/dolphin/WiimoteNew.ini`; the GameCube default
  equals the new `deploy/dolphin/GCPadNew.ini`; reading each file gives the default with no custom.
- Unknown lines, other sections, `Device` and `Rumble/*` survive byte for byte; duplicates updated; missing
  keys appended inside the section; missing section added; CRLF kept; non-UTF-8 gives 500, file unchanged.
- Custom detection and `null` pass-through as in v1; a duplicate pad name in a hand-edited file reads custom.
- Dead zone read (`12`, `12.0`, `12.000000`), write `n.000000`, reset removes the keys, for both types.
- Port line: replaces `SIDevice0 = 0` with `6`; adds the line to an existing `[Core]`; adds `[Core]` if missing;
  leaves every other `Dolphin.ini` byte alone; absent file means skipped and `portReady: false`; value already
  6 means no write and no backup.
- Injection: control ids from the request never reach the file (keys come from the layout); names with
  newlines, `[Section]`, `=`, backticks, unicode lookalikes, case variants; long strings; `type` values like
  `../x`, `classic%0a`, `Classic` (all 404).

Integration tests in `tests/controls.rs` (harness style of `tests/notes.rs`, temp `data_dir` and `dolphin_dir`):

- Auth: 403 without the secret on GET, PUT, reset for both types; 200 with it.
- 404: unknown type; no dolphin entry in `emulators.json` (only a `mystery` emulator); a machine with no
  `emulators.json`.
- The old route is gone: `GET /api/games/4026531840/controls` is 404 (axum no-route).
- GET shapes: no file gives `source: "default"` and `rev: "none"`; with the repo file gives `file`. GameCube
  `portReady` true only with `SIDevice0 = 6` present.
- PUT happy path per type: mode 0600, response equals next GET, classic file has the mirrors, GameCube PUT
  also sets `SIDevice0 = 6` and makes `Dolphin.ini.before-brisk-edit` once.
- The two types are independent: a classic PUT leaves `GCPadNew.ini` untouched and the other way round.
- Atomic: no `.tmp` left; a failing rename (target is a directory) gives 500 and the original is intact.
- Backup once per file; none when there was no file.
- Validation matrix per type (unknown or missing id, extra key, wrong type, `null` on a non-custom control,
  stick name on a button, `D-pad` on a button, duplicate names, `z` in a classic body and `zl` in a GameCube
  body, dead zone 51 / -1 / 1.5, 4 KiB cap gives 413, bad JSON), file unchanged each time.
- Concurrency: stale `rev` gives 409 with `current`; two PUTs at once leave a valid file.
- Running: a fake proc dir with a `dolphin-emu` process gives `running: true`; PUT still writes.
- Reset per type: default restored, dead zone keys removed, custom replaced, GameCube reset also ensures the port.
- Fixture contract: each `v2-*-get.json` parses into the response struct and each `v2-*-put.json` passes the
  validator for its type.

### Web

Unit (vitest, pure):

- `modules/settings/emulators/mapping.ts` (pure, new): `CONTROL_SETS` per type (ids, labels, kind), `ACCEPTS`,
  `DEFAULTS`, `PAD_LABEL`, `bind(state, controlId, pad)` returning the new bindings and a `swappedWith`/`refused`
  result, `padHolder`, `unbound(type, bindings)`. Tests: plain bind, swap, bind into a free name frees the old
  one, same-name no-op, refuse when the control is custom and the name is held, kind match, GameCube leaves
  three pad names unbound by default, defaults are valid and equal the fixtures' bindings.
- `listen.ts` moves with it and keeps its v1 tests (first press wins, held at start, trigger threshold, d-pad,
  stick 0.6/0.3 hysteresis, lowest index wins, wrong kind reported, disconnect cancels).
- `pad.svg.test.ts` gains: every id in `PAD_NAME_BY_ID` exists in `ALL`; every pad name has a label.
- `store.test.ts`: the store with a mocked `api`: load per type, 404 hides, serial autosave (latest wins), `rev`
  carried, 409 replaces the view, failed save reverts, restart flow order (quit, get, put without `rev`, launch).
- `tabs.test.ts`: `#controller/dolphin` opens the Controller tab and yields the sub anchor; unknown sub part is
  ignored.
- `state/api.test.ts`: `route('/api/emulators/dolphin/controls/classic')` is scoped with a machine selected.
- Contract test: parses the four `v2-*.json` fixtures with the web types (key sets equal `CONTROL_SETS`, values
  in `ACCEPTS`).

Browser (`*.browser.test.ts`, API mocked from the fixtures):

- `EmulatorMappings.browser.test.ts`: renders the pad drawing in select mode with bound labels from the
  fixture; type switch changes the list and the labels; pad-first (click LB, prompt, radio pick, PUT body);
  list-first (Change, faked pad press via a stubbed `navigator.getGamepads`, bound and saved); list-first with a
  click on the drawing and no pad; swap message and both chips; unbound button shows dashed and "not used";
  Esc cancels and returns focus; "No pad seen"; Reset; dead zone only for sticks and saved on change; restart bar
  only when `running`; 404 hides the section; load error, save error and 409; `portReady: false` note; a custom
  control reads "Custom".
- A11y: roles, names, `aria-pressed`, radiogroup, status region text, no `aria-hidden` on the interactive svg.
- `Controller test` regression: with no `select` prop, `PadDiagram` still renders `aria-hidden` and no extra
  roles (existing `pad.svg` and Controller tests keep passing unchanged).
- `GameDetail.browser.test.ts`: an emulator id shows the summary card with the "Edit mappings" link to
  `#controller/dolphin`; a Steam id shows none.
- `scripts/check-wording.sh` scans the new strings.

## 4. Work split

Both tasks work against sections 1 to 3 and the four fixtures. They share nothing else. If the contract must
change, change the fixtures and this doc first.

### Task A: BACKEND (hostd)

Files:

- `crates/brisk-hostd/src/controls.rs` (generic `Kind`/`Layout`, GameCube layout, port-line writer)
- `crates/brisk-hostd/src/http.rs` (replace the three `/games/{appid}/controls*` routes with the three
  `/emulators/dolphin/controls/{type}*` routes; remove `controls_gate`)
- `crates/brisk-hostd/src/emu.rs` (drop the `controls` flag on `spec`; add a "has a dolphin entry" helper)
- `crates/brisk-hostd/tests/controls.rs` (rewrite per section 3)
- `crates/brisk-hostd/tests/fixtures/controls/` (delete `get.json`, `put.json` once the tests use `v2-*`)
- `deploy/dolphin/GCPadNew.ini` (new, embedded) and, if the verification changes anything,
  `deploy/dolphin/WiimoteNew.ini` stays as is
- `docs/env.md` (the `BRISK_DOLPHIN_DIR` row now covers `GCPadNew.ini` and `Dolphin.ini` too)
- `scripts/setup-dolphin.sh` and `scripts/test-setup-dolphin.sh` only if they should also install a default
  `GCPadNew.ini` (not required; the editor creates it)

First: write a mapping from Dolphin's own GameCube controller window on the games VM, read `GCPadNew.ini` back,
and fix the key table and dead zone format (section 2.1) before the rest. Also check the hub proxy for a path
allowlist. No web files. Done when `scripts/check.sh` passes and the section 3 Rust tests exist.

### Task B: WEB

Files:

- `web/src/modules/settings/PadDiagram.svelte` and `pad.svg.ts` (select mode, `PAD_NAME_BY_ID`, `PAD_LABEL`)
- `web/src/modules/settings/EmulatorMappings.svelte` (new), `emulators/mapping.ts`, `emulators/listen.ts`,
  `emulators/store.ts` and their tests (new; port from `modules/library/controls/{controls,classic,listen}.ts`)
- `web/src/modules/settings/ControllerTest.svelte` (render the new card below the live test)
- `web/src/modules/settings/Settings.svelte` and `tabs.ts` (the `#controller/dolphin` hash, scroll and focus)
- `web/src/modules/library/GameDetail.svelte` (swap `ControlsPanel` for `ControlsSummary.svelte`), new
  `web/src/modules/library/ControlsSummary.svelte`
- delete `web/src/modules/library/controls/*` (ClassicDiagram, ControlsPanel and the old tests) after porting
- `web/src/state/api.ts` (new `emulatorControls/putEmulatorControls/resetEmulatorControls(type)` methods and
  types, `emulators` in `SCOPED`, remove the per-appid methods) and `api.test.ts`
- tests per section 3 (web part), reading the fixtures by relative import

May mock the API from the fixtures until Task A lands. No hostd files. Done when `npm --prefix web run ci`
and the browser tests pass and `scripts/check-wording.sh` is clean.

### Then (not part of A or B)

Merge, `scripts/check.sh`, curl the three routes against a throwaway `BRISK_DOLPHIN_DIR`, deploy only when Toby
says, and check on the games VM that a GET leaves both files untouched, the backups appear after the first
save, and a GameCube game responds to the mapped pad.

## 5. Needs Toby (blockers only)

None block the work. Two things are worth his eye but have a default:

1. GameCube default layout (2.2): position based (GameCube B on Xbox X, GameCube X on Xbox B) by my call. If he
   wants letter for letter, it is one table.
2. After deploy, a real test on the VM that Dolphin takes the GameCube mapping (the key names in 2.1 are the
   unverified part; Task A checks them first).
