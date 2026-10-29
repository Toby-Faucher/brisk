# Emulator controller editor (design)

Status: v1 was built, then replaced by `2026-10-28-emulator-controls-v2.md` (merged to main). Toby chose "option 1, Pad diagram" from the
mock-up round. The UI is decided: this doc specifies it, it does not redesign it.

## 1. Intent and scope

An emulator game in the Library (Dolphin, added in `emulators.json`) streams with the Xbox-layout pad that
Sunshine shows to the machine. Dolphin maps that pad to a Wii Remote with a Classic Controller through
`WiimoteNew.ini`. Today the only way to change that mapping is to edit the ini by hand or open Dolphin's own
window. This feature lets you do it from the game's detail panel: click a control on a drawn Classic
Controller, press the pad button you want, and the bound button shows next to the control.

First version:

- Emulator: Dolphin only.
- Controller: Wii Remote 1 with a Classic Controller, mapped from the Xbox-layout streamed pad.
- One mapping for the whole machine (Dolphin has one `WiimoteNew.ini`). Every Dolphin game on that machine
  shares it. The UI says so.
- Edits are saved to the machine's file at once and used the next time the game starts.

Non-goals: other emulators, Nunchuk or GameCube pad types, more than one player, per-game profiles, an
in-stream panel, presets (the mock-up's Switch-style and Left-handed presets are out; only "Reset to Xbox
default" ships), importing or exporting a mapping, editing IR, shake, rumble or motion keys.

## 2. Data model

One JSON shape for the API and the UI. camelCase, like the rest of `/api`.

Control ids (Classic Controller): `a b x y l r zl zr minus plus home ls rs dp`.

Pad names (the mock-up's names, the Xbox layout the stream shows):

- Buttons: `A B X Y LB RB LT RT View Menu Guide` (11 names).
- Groups: `Left stick`, `Right stick`, `D-pad`.
- `L3` and `R3` exist on the pad but the Classic Controller has no stick click, so they are never bound.

Which names a control accepts (this is the allowlist, in both hostd and the web):

| Control ids | Accept |
|---|---|
| `a b x y l r zl zr minus plus home` (11 controls) | the 11 button names |
| `ls`, `rs` | `Left stick`, `Right stick` |
| `dp` | `D-pad` |

Eleven controls take eleven button names, two sticks take two stick names, so a full mapping is always a
one-to-one pairing within each row. That is why swap-on-conflict always has a partner (section 5).

```json
{
  "emulator": "dolphin",
  "controller": "Wii Remote 1, Classic Controller",
  "source": "file",
  "rev": "5b1f0c9a2d7e4a31",
  "running": false,
  "appliesOn": "next-launch",
  "bindings": { "a": "B", "b": "A", "...": "...", "dp": "D-pad" },
  "deadZone": { "ls": 0, "rs": 0 },
  "custom": { "a": false, "b": false, "...": "...", "dp": false }
}
```

- `bindings[id]`: a pad name, or `null` when the control is custom.
- `custom[id]`: `true` when hostd could not map that control's lines back to one known pad name. The control
  shows as "Custom" in the UI, cannot be listened for until you pick it (picking overwrites), and its lines are
  kept untouched on write while it stays `null`. A control that repeats a pad name already used by an earlier
  control (hand-edited file) is also read as custom, so the pairing rule holds.
- `deadZone[id]`: whole percent, 0 to 50, for `ls` and `rs` only.
- `source`: `"file"` (read from `WiimoteNew.ini`) or `"default"` (no file or no `[Wiimote1]` section yet; the
  values are the Xbox default).
- `rev`: opaque string for the file's content (see concurrency). `"none"` when there is no file.
- `running`, `appliesOn`: see section 4.

Control order and labels are web-side constants. The Xbox default (this is the known-good file, not the
mock-up's `DEFAULT`):

`a:B b:A x:Y y:X l:LT r:RT zl:LB zr:RB minus:View plus:Menu home:Guide ls:Left stick rs:Right stick dp:D-pad`

Note: the mock-up's `DEFAULT` has `l:LB r:RB zl:LT zr:RT`. That does not match
`deploy/dolphin/WiimoteNew.ini`, where `ZL = TL`, `ZR = TR` (the bumpers) and the analog triggers `L`, `R`
are `Full Axis 2+` and `Full Axis 5+` (the triggers). The file wins; the mock-up default is a mock-up slip.

PUT body: `{ "rev"?: string, "bindings": {id: padName | null}, "deadZone": {"ls": n, "rs": n} }`. All 14
bindings must be present. `null` means "leave this control's lines as they are" and is only allowed where GET
said `custom: true`. Shared fixtures: section 7.

## 3. Translation to Dolphin

Dolphin's saved names (verified live): face buttons are `EAST SOUTH NORTH WEST`, shoulders `TL TR`, then
`SELECT START MODE`, triggers `Full Axis 2+` and `Full Axis 5+`. Values with a space are written in backticks;
single words are bare, as in the known-good file.

### Buttons and triggers (both ways)

| Pad name | Dolphin expression |
|---|---|
| `A` | `SOUTH` |
| `B` | `EAST` |
| `X` | `WEST` |
| `Y` | `NORTH` |
| `LB` | `TL` |
| `RB` | `TR` |
| `LT` | `` `Full Axis 2+` `` |
| `RT` | `` `Full Axis 5+` `` |
| `View` | `SELECT` |
| `Menu` | `START` |
| `Guide` | `MODE` |

Check against the default: Classic A is `EAST` (Xbox B), Classic B is `SOUTH` (Xbox A), Classic X is `NORTH`
(Xbox Y), Classic Y is `WEST` (Xbox X).

Reading: strip the optional backticks, trim, collapse inner runs of spaces, compare exactly (case sensitive,
Dolphin writes these names in that case). No match, or any `|`, `&`, `!`, `+` combination, `` ` `` inside, or a
modifier: the control is custom.

### Sticks (four axes each), d-pad (hat)

Pad stick to axes: `Left stick` is axes 0 (x) and 1 (y), `Right stick` is axes 3 (x) and 4 (y). The d-pad hat
is axis 6 (x) and axis 7 (y). Up is the negative y end.

| Control bound to | Up | Down | Left | Right |
|---|---|---|---|---|
| `Left stick` | `` `Axis 1-` `` | `` `Axis 1+` `` | `` `Axis 0-` `` | `` `Axis 0+` `` |
| `Right stick` | `` `Axis 4-` `` | `` `Axis 4+` `` | `` `Axis 3-` `` | `` `Axis 3+` `` |
| `D-pad` | `` `Axis 7-` `` | `` `Axis 7+` `` | `` `Axis 6-` `` | `` `Axis 6+` `` |

These fill the four keys `Up`, `Down`, `Left`, `Right` of the control's group. Reading: all four lines must
match one row; otherwise the control is custom. So `Classic/Left Stick/*` set to the Right stick row reads as
`ls = Right stick`.

### Ini keys written (all under `[Wiimote1]`)

| Control | Classic key(s) | Basic Wii Remote mirror(s) |
|---|---|---|
| `a` | `Classic/Buttons/A` | `Buttons/A` |
| `b` | `Classic/Buttons/B` | `Buttons/B` |
| `x` | `Classic/Buttons/X` | `Buttons/1` |
| `y` | `Classic/Buttons/Y` | `Buttons/2` |
| `zl` | `Classic/Buttons/ZL` | none |
| `zr` | `Classic/Buttons/ZR` | none |
| `l` | `Classic/Triggers/L` and `Classic/Triggers/L-Analog` | none |
| `r` | `Classic/Triggers/R` and `Classic/Triggers/R-Analog` | none |
| `minus` | `Classic/Buttons/-` | `Buttons/-` |
| `plus` | `Classic/Buttons/+` | `Buttons/+` |
| `home` | `Classic/Buttons/Home` | `Buttons/Home` |
| `ls` | `Classic/Left Stick/{Up,Down,Left,Right}` | none |
| `rs` | `Classic/Right Stick/{Up,Down,Left,Right}` | none |
| `dp` | `Classic/D-Pad/{Up,Down,Left,Right}` | `D-Pad/{Up,Down,Left,Right}` |
| dead zone | `Classic/Left Stick/Dead Zone`, `Classic/Right Stick/Dead Zone` | none |

Source of truth when reading is the `Classic/*` key; the mirrors are write-only and always set to the same
expression, as in `deploy/dolphin/WiimoteNew.ini`. The mirrors keep the plain Wii Remote working if the
extension is ever unplugged in Dolphin.

Dead zone: the known-good file has no dead zone key, so Dolphin's own default applies. Dolphin stores a stick's
`Dead Zone` as a percent with six decimals (`Classic/Left Stick/Dead Zone = 12.000000`, range 0 to 50).
hostd reads a missing key as 0 and writes `{n}.000000`. Reset removes both keys. This format is from memory of
Dolphin's own files, not from a file on this box: task A must set a dead zone once in Dolphin's window, read
the file back, and fix this paragraph if the format differs. A value hostd cannot parse (for example a
different number format) reads as 0 and is rewritten only if you move the slider.

Not written, ever: the `Device` line, `Source`, `Extension`, `IR/*`, `Shake/*`, `Nunchuk/*`, `Rumble/*`,
`Options/*`, `Extension` keys, `Classic/Left Stick/Radius`, `Modifier`, calibration and any other key, and
every other section (`[Wiimote2]` and so on). The `Device` line is not read for the mapping, never changed and
never shown. If Toby changes the pad's name in Dolphin, brisk leaves it.

### Rewriting the file

Line-based, not a full ini parse, so unknown content survives byte for byte:

1. Read the file as bytes; if it is not UTF-8, stop with 500 `unreadable` and do not write (never lose data).
2. Find `[Wiimote1]` (a line that equals it after trim) and its end (next `[` line or end of file).
3. For each key to write: replace the value of each matching line in the section (all duplicates), keeping
   its key text, as `Key = value`. A missing key is appended at the end of the section, before any blank
   lines that close it.
4. Everything else (comments, other sections, unknown keys, blank lines, order) is copied as read. Line
   endings follow the file (LF unless the first line ends in CRLF).
5. Values come only from the translation tables above, selected by an already validated pad name or a number
   already parsed into 0 to 50. User text is never copied into the file.

Reading is tolerant: `Key=value` without spaces, extra spaces, comment lines, backticks or none.

### Missing file or section

Decision (my call): hostd embeds `deploy/dolphin/WiimoteNew.ini` with `include_str!` and uses it when the file
or the `[Wiimote1]` section does not exist.

- Why embed and not read from a path: the deployed binary does not ship the repo's `deploy/` folder, so a
  runtime path may not exist; the compile-time copy cannot drift because a unit test asserts the embedded text
  equals what the Xbox default translates to.
- A missing file reads as `source: "default"` with the Xbox default. The first PUT or reset creates the file
  (and `dolphin-emu/` with mode 0700) from the embedded text, then applies the edit. The embedded text includes
  the known-good `Device` line, which is right for the streamed Sunshine pad.
- A file with no `[Wiimote1]` section gets the embedded `[Wiimote1]` block appended, then the edit.
- An existing file is never replaced by the embedded one, and `Device` in it is never touched.

## 4. API

Under the existing `/api` guard (Host allowlist, then `x-brisk-secret` or an allowed tailnet login). Nothing
new for auth, and no route skips the guard. All three routes are per machine. In the hub they go through
`/api/m/{id}/*` like `/api/games`; the web client's `route()` already scopes paths that start with
`/api/games`, so no client change is needed for that.

| Method and path | Body | Answers |
|---|---|---|
| `GET /api/games/{appid}/controls` | none | 200, the section 2 object |
| `PUT /api/games/{appid}/controls` | PUT body | 200, the section 2 object after the write |
| `POST /api/games/{appid}/controls/reset` | none (an empty body is fine; a body is ignored) | 200, the section 2 object after the write |

Why a `reset` route and not a client-side PUT of the default: the default also deletes the dead zone keys and
overwrites custom lines. The client could do the same through PUT, but hostd owning "what is the default"
keeps one copy of the table. It costs one small handler.

Rules for all three:

- `appid` is a `u32` path segment. It must be an emulator id (`emu::is_emu_id`), match an entry in
  `emulators.json`, and that entry's emulator must have a controls adapter (only `dolphin`). Otherwise
  **404** `{"error":"no controls for this game"}`. This covers a Steam appid, an unknown emulator id and an
  entry with `emulator: "mystery"`. (An unparsable `appid` is axum's 400.)
- `running` is true when any Dolphin process is running (use `emu::pids` over all Dolphin entries, not just
  this game: the config file is shared, and any running Dolphin owns it). `appliesOn` is always
  `"next-launch"` in v1 (Dolphin reads the file at start). The field exists so the UI never hardcodes it.
- The ini path is `{dolphin_dir}/WiimoteNew.ini`. `dolphin_dir` is a new `Config` field from env
  `BRISK_DOLPHIN_DIR`, default `${XDG_CONFIG_HOME:-~/.config}/dolphin-emu` (same as
  `scripts/setup-dolphin.sh`). Add it to `docs/env.md`. Tests point it at a temp dir. If the path is a
  symlink, resolve it with `canonicalize` first and replace the target, so a symlinked config survives.
- PUT body cap: 4 KiB (`DefaultBodyLimit::max(4 << 10)`); over it is **413**. A full body is under 700 bytes.
  GET and reset take no body worth reading; give reset the same cap.
- PUT validation, all before anything is written, first failure wins, **400** `{"error": "<short reason>"}`:
  JSON must parse into a struct with `deny_unknown_fields`; all 14 binding keys present, no others; each value
  is in the allowlist for that id (table in section 2) or `null`; `null` only for a control that is currently
  custom in the file; no pad name used twice; `deadZone` has exactly `ls` and `rs`, integers 0 to 50. `rev`,
  if present, must be a string of at most 32 chars. Error text never echoes the input back.
- Other codes: **409** `{"error":"changed elsewhere"}` when `rev` is present and no longer matches (the body
  carries the current object under `current`, so the UI can show it without another GET); **500**
  `{"error":"could not save controls"}` for an IO error or a non-UTF-8 file (nothing is changed). 405 for other
  methods comes from axum.
- Responses are the section 2 object read back from what is now on disk, never the request echoed.

### Atomic write and backup

- Under one process-wide `Mutex` for the read, edit, write sequence (so two requests cannot interleave).
- Write the new bytes to a temp file in the same directory (`.WiimoteNew.ini.{pid}.tmp`, mode 0600,
  `create_new`, fsync), then `rename` over the file, the same pattern as `notes.rs` and `prefs.rs`. A failed
  rename removes the temp file. The result is mode 0600; Dolphin runs as the same user.
- One-time backup: before the first write to an existing file, copy its bytes to
  `WiimoteNew.ini.before-brisk-edit` in the same directory with `create_new` and mode 0600. If that file
  already exists, do nothing (never overwrite: it holds the original). If the backup fails, the write is
  abandoned with 500. No backup when there was no file. (`setup-dolphin.sh` uses a different name,
  `.before-brisk`, so the two never collide.)

### Concurrency (two browsers)

`rev` is a hash of the file's bytes (hex of `std::hash::DefaultHasher::new()` over the content; `"none"` for
no file). The UI sends the `rev` it last saw on every normal save. If another browser saved in between, hostd
answers 409 with the current state, the UI replaces its view and shows "Another device changed the controls.
Showing the latest." The in-process mutex covers the check and the write together. Shortcut: `DefaultHasher`
is fine because `rev` is only compared inside one hostd run; if it ever has to survive restarts, use a real
digest.

A PUT without `rev` is last write wins. Only the restart flow and reset use that.

### If Dolphin is running

Important and easy to get wrong: Dolphin rewrites `WiimoteNew.ini` from its in-memory copy when it exits. So
an edit made while the game runs is overwritten by the old mapping on exit, even though the file was written.

- hostd still writes the file (the user's intent is saved and shown; `running: true` tells the UI).
- The UI shows the "Restart game to apply" bar. That flow is quit, then write again, then launch (below), so
  the exit rewrite cannot undo the edit.
- If the game is closed some other way, the old mapping comes back. The next GET shows the truth, and the UI
  just reloads its view on open and on focus.

### Restart game to apply (no new restart route)

Existing routes only. The web client does, in order:

1. `api.quit(appid)` (`POST /api/quit`). If it answers `still-running`, stop and show the Force quit action the
   way `state/quit.ts` does (same toast wording and Force quit call). 409 "not running" means it already closed:
   go on.
2. `GET /controls`, then `PUT /controls` with the draft the user made and no `rev`. This rewrites the file
   after Dolphin's exit rewrite. (If the quit was a force quit there was no rewrite and this is a harmless
   repeat.)
3. `api.launch(appid)` (`POST /api/launch`). hostd starts Dolphin and focuses it fullscreen (`emu::launch`
   and `focus`, already built).

Why no `/restart` route: the three steps need the browser's draft for step 2 and the existing routes already
do the process work. A server route would have to guess the draft or keep it, which adds state. If a step
fails the bar says which one ("Could not close the game", "Could not save the controls", "Could not start the
game") and offers Try again; the file is already saved from the earlier PUT unless step 2 failed.

## 5. UI spec (web)

Everything below follows the "option 1 / Pad diagram" mock-up (`f1`, `svgPad`, `HS`). Visual tokens come from
`tokens.css` and the existing panels (docs/ui-design.md section 4, docs/ui-notes.md), not from the mock-up's
own CSS.

### Where it lives

- The emulator game's detail panel (`modules/library/GameDetail.svelte`) gets a **Controls** section, a
  `<section class="sec">` with an `<h3>Controls</h3>`, placed above Screenshots (an emulator game has no Steam
  info, so the panel is otherwise short). Shown when `isEmulatorId(appid)` (>= 0xE0000000, a small helper next
  to `Game` in `state/api.ts`) **and** the first GET did not answer 404. A 404 hides the section; any other
  error shows the error state inside it.
- The Settings Controller tab (`ControllerTest.svelte`) stays as it is (live test, drift, range). Its intro
  gets one line: "To change what a game's buttons do, open an emulator game in the Library." No link needed.
- No change to Steam games.

### What is reused, what is extended

Reused as is:

- `lib/gamepad.ts`: `PadLike` and the standard-mapping indexes. The listening loop reads pads with
  `navigator.getGamepads()` the same way `ControllerTest.svelte` does (requestAnimationFrame while the section
  is open and the page has focus); it does not touch `GamepadSender`.
- `settings/controllertest.ts`: `padView` (button states, trigger values, stick vectors, `stickView` for the
  threshold) and `padChoices` (which pad is connected, for the "No pad seen" state and the pad chip). Cross
  module import of these pure helpers is fine; do not copy them.
- `state/api.ts`: `api.quit`, `api.launch`, `route()` machine scoping, `ApiError`, the `call` helper.
- `state/notes.ts` as the pattern for a store with an `api` client, a `writable`, and a serialised save.
- The toast helper and Force quit wording from `state/quit.ts`.

New (all under `modules/library/controls/`):

- `classic.ts`, pure: `CONTROLS` (id, label, group, kind button or stick or dpad), `PAD_NAMES`, `ACCEPTS`
  (the allowlist table), `XBOX_DEFAULT`, `PAD_BY_INDEX` (0 A, 1 B, 2 X, 3 Y, 4 LB, 5 RB, 6 LT, 7 RT, 8 View,
  9 Menu, 12 to 15 D-pad, 16 Guide; 10 and 11 map to nothing), and `bind(state, id, pad)` which returns the
  new state plus `swappedWith`.
- `listen.ts`, pure: a detector fed `PadView` frames that says which pad name was pressed (rules below).
- `controls.ts`: the store (`controls`, load, save queue, restart flow), typed to the section 2 shape.
- `ClassicDiagram.svelte`: the drawn Classic Controller with hotspots. The `HS` table from the mock-up is
  copied as data (viewBox `0 0 560 320`; rect or circle, position, size, tag position and anchor per control).
  Sizes of the tag text must be large enough to read on a phone; check at 360 px wide.
- `ControlsPanel.svelte`: the section (diagram, the side card, the Reset button, the apply bar, the status
  line).

Decision (my call): `PadDiagram.svelte` and `pad.svg.ts` are **not** reused for this diagram. They draw the
physical 8BitDo pad to scale as a display-only, `aria-hidden` live tester, with geometry in mm. The mock-up's
option 1 draws the **Classic Controller** you are binding (that is what each control means), with the Xbox
name as a tag beside each control. Making the 8BitDo drawing clickable would bind the wrong thing in the
user's head (you pick a Wii control, not a pad button). The small extension that is worth doing: nothing in
`PadDiagram`/`pad.svg.ts`. In the panel, a "pad chip" row (from the mock-up's `padChip()`) shows the connected
pad's name and the button being pressed now, using `padChoices` and `padView`; it can show the last pressed
pad name in a short flash.

### States

| State | What shows |
|---|---|
| loading | Section with a skeleton diagram (grey shapes), `aria-busy` |
| idle | Diagram with all tags; first control (`a`) selected; side card shows its binding |
| selected | One hotspot has the selected style; side card: control name, "Pad button" and the bound name, a "changed" chip when it differs from the Xbox default, a "Listen for a button" button |
| listening | The selected hotspot pulses, its tag reads "press...", side card shows "Press a button on your pad. Esc to cancel." and a Cancel button |
| saved | Status line (polite live region): "Saved to the machine. Used the next time the game starts." |
| restart-needed | Apply bar: "Saved to the machine. Dolphin reads the mapping at start, so restart the game to use it." and the primary button "Restart game to apply". Only when `running` is true |
| restarting | Bar: "Restarting the game..." with the button disabled |
| error | Inline message with Try again. Save failure: "Could not save the controls." The tag shows the old value again |
| conflict | "Another device changed the controls. Showing the latest." (409) |
| no pad seen | Pad chip: "No pad seen. Click a button on your pad, or connect one." Listening still starts, but waits, and the card says "Waiting for a pad." The diagram stays fully usable by mouse and keyboard for selecting; only binding needs a pad |
| custom | The tag reads "Custom" in the warn color; side card says "This control was set in Dolphin and brisk cannot show it. Pick a button to replace it." Stays untouched until then |

### Interaction rules

- **Select**: click a hotspot, or Tab to it and press Enter or Space. Selecting does not listen.
- **Listen**: the "Listen for a button" button, or Enter on the already selected hotspot. Starting listen needs
  a pad; with none, it waits (see no pad seen).
- **First new press wins**: when listening starts, take a baseline of the pad. Any button already held and any
  stick already off center is ignored until it has been released once (ignore a held button at start). The
  first button that goes from released to pressed is the answer. Triggers count as pressed above 0.5.
  The d-pad (indexes 12 to 15) answers as `D-pad`.
- **Stick**: a stick answers when pushed past 0.6 from center (and then it must come back below 0.3 before it
  can answer again, so one push is one answer). Left or right stick by which one moved.
- **Kind match**: a button control only takes button names, a stick control only `Left stick` or `Right stick`,
  the d-pad only `D-pad`. A press of the wrong kind is ignored and the card says "That is a stick. A takes a
  button." (or "That is a button. Left stick takes a stick.") and keeps listening.
- **Esc** cancels listening and returns focus to the hotspot. Cancel button does the same. Tab or a click
  elsewhere also cancels. Leaving the page, the pad disconnecting or the section closing cancels.
- **One listener**: starting a new listen cancels the old one.
- **After a bind**: listening ends, the selection stays on the control, the diagram updates, a save starts.

### Swap on conflict

A pad button drives only one control. Binding a name that another control holds swaps the two: the other
control gets the old binding of this one. Message in the status line: "Swapped with ZL (a pad button can only
drive one control)." Because the pairing is complete (section 2), the swap always has a partner. The one case
with no partner: the other control holds the name through custom-ness (cannot happen: custom is `null`, so no
name is held). The swap happens in `bind()`, one PUT carries both changes.

### Reset and dead zone

- **Reset to Xbox default**: a ghost button under the side card. It asks nothing (it is easy to undo by
  re-binding and the original file is backed up) but shows a status: "Reset to the Xbox default." Calls
  `POST .../reset`. It also clears custom controls.
- **Dead zone**: shown in the side card only when `ls` or `rs` is selected, as the mock-up: label "Dead
  zone", a range input 0 to 40 (step 1) with the value readout "12%". The API allows 0 to 50; the slider stops
  at 40 as in the mock-up (Toby's pick). It writes Dolphin's `Dead Zone` key for that stick (section 3).
  The label hints: "How far to push before Dolphin sees movement." The readout updates while dragging; the save
  fires on `change` (release), not on every `input`. The default is 0, as in the known-good file (the mock-up
  showed 12 as sample data; my call is to keep the known-good behavior).
- After Reset, `rev` comes from the response.

### Saving

Autosave, no Save button. One save in flight at a time; if the user changes more while one is running, the
latest full draft is sent when it finishes (the latest wins; same shape as the layout save in `prefs.ts`). The
`rev` of the last response goes into the next PUT. A failed save reverts the visible tags to the last
confirmed state and shows the error state.

### Keyboard and screen reader

- Each hotspot is a `<g role="button" tabindex="0">` with `aria-pressed` for selected and an `aria-label` like
  "A, bound to B. Press Enter to change." (from the mock-up). While listening the label is "A, listening. Press
  a button on your pad."
- The diagram `<svg>` has `role="group"` and `aria-label="Classic Controller"`. A visually hidden list of the
  14 controls is not added; the 14 hotspots in tab order are the list.
- Focus ring on the selected and focused hotspot uses the app focus ring token, never removed.
- A polite `role="status"` region announces: "Listening for A. Press a button on your pad.", "A is now bound to
  B.", the swap message, saved, and errors (errors use `role="alert"`).
- Escape works from anywhere inside the section while listening.
- No timing or motion carries information. The listening pulse stops under `prefers-reduced-motion` and the
  tag text "press..." remains.
- Contrast: tags and labels 4.5:1 or more on the panel (the docs/ui-notes.md contrast checks cover tokens).

### Strings (plain, no em dashes)

Section title: Controls. Intro: "These buttons are shared by every Dolphin game on this machine. Click a
control, then press the pad button you want." Pad chip: "Pad: {name}" / "No pad seen". Buttons: "Listen for a
button", "Cancel", "Reset to Xbox default", "Restart game to apply", "Try again". Status lines are in the
states table. Errors: "Could not load the controls.", "Could not save the controls.", "Could not close the
game", "Could not start the game". All lines pass `scripts/check-wording.sh` (it also scans `web/src` strings).

## 6. Tests

Follow the repo's layers: Rust unit tests inline, hostd integration tests in `crates/brisk-hostd/tests/`,
web vitest unit project (`npm test`), web browser project (`npm run test:browser`, `*.browser.test.ts`).

### Rust

Unit tests in a new `crates/brisk-hostd/src/controls.rs` (translation, parser, rewriter; no network):

- Round trip for every control and every pad name it accepts: write then read gives the same name, for each of
  the 14 controls (button controls x 11 names, sticks x 2, dp x 1), and the four stick keys and the mirrors
  hold the expected expressions.
- The Xbox default applied to an empty `[Wiimote1]` yields the exact key set of `deploy/dolphin/WiimoteNew.ini`
  (same expressions; order may differ). The embedded text equals the repo file (the `include_str!` source).
- Reading the repo file gives the Xbox default and `custom` all false.
- Unknown lines preserved: comments, other sections, `Device`, `IR/*`, `Nunchuk/*`, unknown keys and their
  order survive an edit byte for byte (compare with the changed lines masked).
- Custom detection: `Classic/Buttons/A = EAST | TL`, a stick with three axes right and one wrong, a duplicate
  pad name across two controls, an odd spelling; each reads custom and stays byte for byte on a PUT with
  `null`; a PUT with a name overwrites it.
- Duplicate key lines are all updated. Missing keys are appended inside the section. Missing section is added.
  CRLF files stay CRLF. Non-UTF-8 file: 500, file unchanged.
- Dead zone: read missing as 0, read `12.000000` as 12, write `12.000000`, reset removes the key.
- Injection: names with newlines, `\n[Wiimote2]`, `=`, backticks, `EAST\nDevice = x`, very long strings,
  unicode lookalikes, case variants (`a` vs `A`); control ids like `a\nb` or `../x`; none reach the file (400)
  and the file is unchanged.

Integration tests in `crates/brisk-hostd/tests/controls.rs`, same harness style as `tests/notes.rs`
(`Config::from_env`, temp dir for `data_dir` and `dolphin_dir`, `tower::ServiceExt::oneshot`):

- Auth: no secret gives 403 on GET, PUT and reset; the secret gives 200.
- 404: a Steam appid (730), an unknown emulator id, and an `emulators.json` entry with `emulator: "mystery"`.
- GET shapes: no file gives `source: "default"` and `rev: "none"`; the repo file gives the default and `file`.
- PUT happy path writes the file with mode 0600, response equals the next GET, and the file contains the
  mirrors.
- Atomic: after a PUT there is no `.tmp` left in the directory; a failing rename (make the target path a
  directory) gives 500 and removes the temp file, the original is intact.
- Backup once: the first PUT creates `.before-brisk-edit` equal to the original bytes; a second PUT does not
  change it; no backup when there was no file.
- Validation: unknown id, missing id, extra key, wrong value type, `null` for a non-custom control, a button
  name on `ls`, `D-pad` on `a`, duplicate names, dead zone 51 or -1 or 1.5, body over 4 KiB (413), bad JSON,
  unknown fields (all 400 unless noted), with the file unchanged each time.
- Concurrency: PUT with a stale `rev` gives 409 and `current`; two PUTs from two tasks at once both end in a
  valid file (one wins, no torn file).
- Running: with a fake proc dir showing a `dolphin-emu` process (the `fake_proc` pattern from `emu.rs`),
  `running` is true and the PUT still writes the file; `appliesOn` is `"next-launch"`.
- Reset: restores the default, removes dead zone keys, replaces a custom control, backs up once.
- Fixtures: `tests/fixtures/controls/get.json` parses into the response struct, and `put.json` is accepted by
  the PUT validator (contract test, keeps the fixture honest for B).

### Web

Vitest unit (`npm test`), pure logic:

- `classic.test.ts`: `bind` (plain bind, swap, swap message data, binding the same name is a no-op, kind
  match, custom control binding), `XBOX_DEFAULT` is a full valid pairing, `ACCEPTS` equals the doc table,
  `PAD_BY_INDEX`.
- `listen.test.ts`: first new press wins; a button held at start is ignored until released; two buttons in one
  frame (lowest index wins, stable); trigger threshold; d-pad answers `D-pad`; stick past 0.6 answers, needs
  return below 0.3; wrong kind is ignored and reports why; a disconnected pad cancels.
- `controls.test.ts` (the store with a mocked `api`): load, 404 hides, autosave serial queue (latest wins),
  `rev` carried, 409 replaces the view, failed save reverts, restart flow order (quit, get, put without
  `rev`, launch) and each failure point.
- A contract test that parses `crates/brisk-hostd/tests/fixtures/controls/get.json` and `put.json` with the
  web types (key set equals `CONTROLS`, values in `ACCEPTS`).

Browser tests (`*.browser.test.ts`, vitest browser mode with `vitest-browser-svelte`), the api mocked from the
same fixtures:

- `ControlsPanel.browser.test.ts`: renders 14 hotspots with tags from the fixture; click selects; Enter starts
  listening (tag reads "press..."); a faked pad press (stub `navigator.getGamepads`) binds, shows the tag and
  sends a PUT with the expected body; a conflicting press shows the swap message and both tags; Esc cancels and
  returns focus; "No pad seen" with no pad; the Reset button; the dead zone slider only on sticks and saves on
  change; the apply bar only when `running` is true and its three steps run in order; 404 hides the section;
  error and 409 states; a custom control reads "Custom".
- `GameDetail.browser.test.ts` gets a case: an emulator id shows Controls, a Steam id does not.
- A11y: roles, names and `aria-pressed` on hotspots; status region text.
- `Settings.browser.test.ts` (or `ControllerTest`) checks the pointer line.

## 7. Work split

Both tasks work against sections 2 to 4 and these fixtures, which exist now:

- `crates/brisk-hostd/tests/fixtures/controls/get.json`: a sample GET response (the Xbox default, file source,
  not running).
- `crates/brisk-hostd/tests/fixtures/controls/put.json`: a sample PUT body (Switch-style face buttons, dead
  zone 10, with a `rev`).

The web side reads them through a relative import. If the contract changes, change the fixtures first, then
both sides.

### Task A: hostd API and tests

Owner files: `crates/brisk-hostd/src/controls.rs` (new), `http.rs` (three routes, a `Mutex` state field),
`config.rs` (`dolphin_dir`), `lib.rs` (module), `docs/env.md` (`BRISK_DOLPHIN_DIR`), `tests/controls.rs` (new).
Do first: verify the dead zone format against a Dolphin-written file (section 3) and report the answer. Use
`emu::is_emu_id`, `emu::load`, `emu::spec` (add a `controls` adapter row only for `dolphin`), `emu::pids` for
`running`. No web files. Done when: `scripts/check.sh` passes (fmt, clippy, nextest, wording, the embed-feature
runs) and the section 6 Rust tests exist and pass.

### Task B: web UI and tests

Owner files: `web/src/modules/library/controls/*` (new), small edits in `GameDetail.svelte`,
`state/api.ts` (`controls`, `putControls`, `resetControls` client calls and types, `isEmulatorId`), and a line
in `ControllerTest.svelte`. May mock the API with the fixtures until A lands (a tiny fake fetch in tests, and
in dev a `?mockControls` is not added; tests only). No hostd files. Done when `npm --prefix web run ci` passes
(biome, svelte-check, vitest, build) and the browser tests pass, and `scripts/check-wording.sh` is clean.

A and B do not touch each other's files. The only shared edit is the fixtures, which this doc fixed.

### Task C: integration and deploy checklist (later, not part of A or B)

1. Merge A and B on `emulator-launch`; `scripts/check.sh` green.
2. Against a throwaway `BRISK_DOLPHIN_DIR`, run hostd and hit the three routes with curl and the secret.
3. Deploy to the machine with Dolphin (not before Toby says), via the usual `scripts/brisk-service.sh update`.
4. On the machine, check `WiimoteNew.ini` is unchanged after a GET, and that `.before-brisk-edit` appears
   after the first save.
5. In the real UI over the hub (`/api/m/{id}/...`): bind, swap, dead zone, Reset, with the real streamed pad.
6. Launch Mario Kart Wii, confirm the new mapping works after "Restart game to apply".
7. While the game runs: change a binding, restart through the bar, confirm the new mapping survived Dolphin's
   exit rewrite.
8. Close Dolphin by hand after an edit and note what the file says (confirms the exit-rewrite behavior).
9. Update `docs/ui-notes.md` (new section for the Controls panel and the hostd routes) and `docs/env.md`.

## 8. Decisions (my calls) and open questions

My calls, each can be vetoed:

1. File wins over the mock-up for the Xbox default: `zl:LB zr:RB l:LT r:RT` (the mock-up had them crossed).
2. Dead zone default is 0 (key absent), not the mock-up's sample 12; slider 0 to 40, API 0 to 50.
3. The diagram is a new Classic Controller drawing from the mock-up's `HS`, not a clickable `PadDiagram`.
   `PadDiagram` and `pad.svg.ts` are untouched.
4. Embed the known-good ini in hostd (`include_str!`) rather than read a path.
5. Edits while Dolphin runs are written at once, and "Restart game to apply" is quit, re-save, launch,
   because Dolphin rewrites the file on exit.
6. `rev` check for two browsers; PUT without `rev` is last write wins.
7. `custom` controls are `null`, kept untouched until the user picks a button.
8. A separate `POST .../controls/reset`; no restart route.
9. `running` means any Dolphin process, since the file is shared.
10. Autosave on each change; no Save button.
11. No Wii Remote 2 to 4 and no `Device` handling beyond "never touch it".

Open question only Toby can answer (not blocking; the default is chosen):

- Should Reset also clear the dead zone to 0, or keep a nonzero default such as the mock-up's 12? Chosen: 0,
  because the known-good file has no dead zone and it worked.
