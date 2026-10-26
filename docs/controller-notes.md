# Controller notes (M3)

Gamepad input from the browser to Sunshine, and rumble back. Code: `web/src/lib/gamepad.ts`
(page), `crates/brisk-bridge/src/input.rs` (bridge), `ControlSession::on_feedback`
(`crates/brisk-client/src/control.rs`). Tests: `web/src/lib/gamepad.test.ts`,
`crates/brisk-bridge/tests/gamepad.rs`.

## What Sunshine needs (checked in ~/build/sunshine-brisk, 7.1.431 + libvirtualhid)

- **No arrival packet is needed.** A multi-controller packet (`0x0C`) whose
  `activeGamepadMask` has the pad's own bit set allocates the pad if it is not allocated yet
  (input.cpp `passthrough(PNV_MULTI_CONTROLLER_PACKET)`, "still supported for legacy clients").
  A packet with that bit clear frees it and is not applied as a state. Only the pad's own bit is
  read; the bridge still sends the full mask, as moonlight does. `SS_CONTROLLER_ARRIVAL` (type,
  capabilities, supported buttons) only feeds the profile choice below; Brisk does not send it.
- **gcmap does not gate anything.** nvhttp.cpp reads `gcmap` from the launch query and process.cpp
  only exports it to the app as `SUNSHINE_CLIENT_GCMAP`. Brisk now sends `gcmap=1` (pad 0), as
  moonlight sends its attached-pad mask; pads attach on their packets either way.
- **Virtual pad type.** `~/.config/sunshine/sunshine.conf` on bawn has no `gamepad` line, so it is
  the default `auto`. With `auto` and no arrival packet (type unknown, no capabilities)
  virtualhid_input.cpp `profile_for_metadata` picks **Xbox Series** (`xseries`). An arrival with
  `LI_CTYPE_XBOX` would pick the same. An **Xbox One** pad needs `gamepad = xone` in
  sunshine.conf (not changed by this work: see "For Toby").
- **Controller input on.** `controller` defaults to true (config.cpp); no setting needed.
- **No deadzone in Sunshine.** `normalize_axis` scales i16 straight to -1..1 for the virtual pad.
  The page adds none either: the virtual pad behaves like a real one and games apply their own.
- **Rumble.** The virtual pad's force feedback becomes `gamepad_feedback_msg_t::make_rumble(
  client_relative_index, low, high)`, sent as control message `0x010b`
  (`0xC0FFEE`, controller u16, low u16, high u16, LE) only when the values change; trigger motors
  as `0x5500` (controller, left, right). `client_relative_index` is the `controllerNumber` the
  client used, i.e. Brisk's slot. Sunshine drops feedback until the control peer is connected.
- Back held alone emulating Home: `back_button_timeout` defaults to -1 ms (off).

## Wire format (page <-> bridge)

Same WebTransport bidirectional stream as keys and mouse (docs/m2-notes.md "Input"): frames
`len u8` + body, body = `kind u8` + fields, little-endian.

Page to bridge:

| kind | body | meaning |
|---|---|---|
| `0x15` gamepad | `slot u8 (0..3), buttons u16, lt u8, rt u8, lx i16, ly i16, rx i16, ry i16` | one pad's whole state; sticks up = positive |
| `0x16` gamepad gone | `slot u8` | the pad was unplugged / the page is going |

`buttons` are Moonlight `buttonFlags` (A 0x1000, B 0x2000, X 0x4000, Y 0x8000, up/down/left/right
0x0001/0x0002/0x0004/0x0008, Start 0x0010, Back 0x0020, L3 0x0040, R3 0x0080, LB 0x0100,
RB 0x0200, Home 0x0400). Any other bit (0x0800), a slot over 3 or a wrong length: `bad`.

Bridge to page (the bridge now writes on this stream; before M3 it never did):

| kind | body | meaning |
|---|---|---|
| `0x20` rumble | `slot u8, low u16, high u16, left u16, right u16` | newest motor values for that pad, 0 = stop |

Rumble rides the input stream rather than a datagram: audio datagrams carry no kind byte, and a
"stop" must never be lost. The bridge keeps only the newest values per slot (a `watch`), so a slow
page cannot back the control loop up; each stream gets the current values when it opens.

## Page (`gamepad.ts`)

- Polls `navigator.getGamepads()` every 4 ms (`POLL_MS`) plus once per animation frame, only while
  `document.hasFocus()` and visible (the Gamepad API only updates for a focused page). Only
  `mapping === "standard"` pads; up to 4, each in the lowest free slot.
- Mapping: buttons by the standard index table above (6/7 are the analog triggers, 0..1 ->
  0..255); axes 0/1 left stick, 2/3 right stick, -1..1 -> i16 (negative x 32768, positive x
  32767), Y negated.
- Sends a pad's state on any change, again every 100 ms while anything is off neutral and every
  250 ms idle. The stream is reliable, so the resends are belt and braces (and keep the bridge's
  1 s held-input watchdog quiet); the bridge drops exact repeats.
- Release: window `blur` and a hidden tab send neutral for every pad off neutral (still
  attached; polling resumes with the real state on focus). `pagehide`, `gamepaddisconnected`, the
  stream stopping (`Input.release`) and the transport closing send neutral (if needed) and
  `0x16` gone.
- Rumble: `vibrationActuator.playEffect("dual-rumble", {duration: 1000, startDelay: 0,
  strongMagnitude: low/65535, weakMagnitude: high/65535})`; with trigger values and an actuator
  listing `trigger-rumble` (Chrome on Windows), `trigger-rumble` with `leftTrigger`/`rightTrigger`
  too. Replayed every 500 ms while on, so a long rumble lasts and a lost page stops within 1 s.
  All zero, blur, gone: `reset()`.
- UI: a small controller icon in the ghost bar while a pad is attached; its tooltip says
  "<name> · slot N" (name = `Gamepad.id` without Chrome's "(STANDARD GAMEPAD Vendor ...)" tail).

## Bridge (`input.rs`)

- `0x15` -> `InputEvent::Gamepad` with the bridge's `activeGamepadMask` (bit per attached slot).
  `0x16` -> neutral (if the pad was not) with its bit set, then the removal (bit clear).
- Queue: a state identical to the pad's last queued one is dropped. Stick/trigger changes replace
  that pad's newest queued state if it has the same buttons (`coalesced`, latest wins); a button
  change is always its own packet, so a press-and-release inside one send window both reach
  Sunshine. Pad states count against the 32 droppable events; a state dropped there is not
  remembered, so the page's next resend brings it back. Removals are never dropped.
- Send loop: a pad state that times out (`Timeout`) is retried like a key-up (it may carry a
  button release) unless a newer state of that pad is already queued.
- Held: a pad off neutral counts as held, so a stream silent for 1 s (`HOLD_TIMEOUT`) sends it
  neutral (still attached; the next state from the page goes through).
- Stream end (tab closed, reload, lost connection): neutral for pads off neutral, then a removal
  per pad; the rumble state is cleared (Sunshine sends no stop for a pad it freed).
- Shutdown: after the key and button releases, a removal (bit clear) for every slot attached this
  session. Not a neutral with the bit set: on a pad Sunshine already freed that would attach a
  fresh one for a moment.
- Rumble: `ControlSession::on_feedback` hands each `Rumble`/`RumbleTriggers` to
  `Shared::rumble`; slots over 3 are ignored. Each input stream has a writer task
  (`write_rumble`) that sends a `0x20` frame per slot whose values changed.

## M3 gate: "5% controller input loss causes no stuck inputs"

Queue-level test `five_percent_loss_leaves_nothing_stuck`: 60 s of 4 ms polls over two pads with 5%
of the page's frames dropped before the queue (as if they were datagrams), the page resending every
100 ms; after the player lets go, Sunshine's view of both pads is neutral. In the real path page to
bridge is a QUIC stream and bridge to Sunshine is reliable ENet, so loss shows up as delay, not as
lost presses. A netem run on the real path is still to do [U].

## [U] for the live test

- Chrome's `navigator.getGamepads()` refresh rate for the 8BitDo 2.4G receiver on Toby's laptop
  (Chrome samples its gamepad backend itself; the 4 ms poll only bounds the page's part).
- That the virtual Xbox Series pad appears on bawn (`libvirtualhid`, uinput, user in `input`),
  that Steam/games see it, and that buttons, sticks (Y direction) and triggers land right.
- Rumble end to end: that a game's rumble reaches Sunshine's feedback queue for slot 0, and that
  the 8BitDo vibrates through `dual-rumble` over the 2.4G receiver; that replaying the effect every
  500 ms is seamless (no felt gaps).
- Reload: the old stream's end detaches every pad, possibly just after the new page attached its
  pad; the next resend (<= 250 ms) attaches it again, so a game may see one quick reconnect.
- Input-to-photon latency for controller input (M3 gate's first half): not measured here.
