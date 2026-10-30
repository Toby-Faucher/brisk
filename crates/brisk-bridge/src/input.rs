//! Browser keyboard, mouse and gamepads to Sunshine; rumble back to the browser.
//!
//! The page opens one WebTransport bidirectional stream and writes frames on it: `len u8`, then
//! `len` bytes of body, `kind u8` + fields (little-endian; docs/m2-notes.md "Input"). One stream
//! keeps key-down and key-up in order, which separate uni streams would not.
//!
//! [`InputQueue`] (sans-io) sits between that stream and the control session: it maps a body to
//! an [`InputEvent`], counts what it drops, coalesces mouse motion and scroll, and holds at most
//! [`MAX_PENDING`] events that may be dropped (motion, scroll, presses). Releases are never dropped,
//! and it knows what is held down, so [`InputQueue::release_all`] can let go of everything when
//! the stream or the bridge ends: no stuck keys on the host.
//!
//! Gamepads: the page sends each pad's whole state (latest wins); the queue merges stick and
//! trigger changes but keeps every button change, sets Sunshine's `activeGamepadMask`, and sends a
//! neutral state and a removal when the stream ends. Rumble from Sunshine goes back on the same
//! stream ([`write_rumble`]), as the newest motor values per pad.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use brisk_client::Error;
use brisk_proto::control::ControlMsg;
use brisk_proto::input::{InputEvent, MAX_MOUSE_DELTA};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Notify, watch};

use crate::wt;

/// `vk u8, down u8 (0/1), modifiers u8` (1 shift, 2 ctrl, 4 alt, 8 meta).
pub const KIND_KEY: u8 = 0x10;
/// `dx i16, dy i16`: relative motion.
pub const KIND_MOVE: u8 = 0x11;
/// `button u8 (1 L, 2 M, 3 R, 4 X1, 5 X2), down u8 (0/1)`.
pub const KIND_BUTTON: u8 = 0x12;
/// `amount i16`: vertical scroll, 120 = one notch, positive = up.
pub const KIND_SCROLL: u8 = 0x13;
/// No fields: "something is still held" (the page sends it every 250 ms while a key or button is
/// down). Never reaches Sunshine.
pub const KIND_HEARTBEAT: u8 = 0x14;
/// `slot u8 (0..MAX_PADS), buttons u16 (Moonlight buttonFlags), lt u8, rt u8, lx i16, ly i16,
/// rx i16, ry i16` (sticks up = positive): one pad's whole state. The page resends it every 100 ms
/// while anything is off neutral and every 250 ms otherwise; a repeat of the last state is dropped.
pub const KIND_GAMEPAD: u8 = 0x15;
/// `slot u8`: the pad is gone (neutral, then a removal to Sunshine).
pub const KIND_GAMEPAD_GONE: u8 = 0x16;
/// Bridge to page, on the same stream: `slot u8, low u16, high u16, left u16, right u16`, the
/// newest motor values Sunshine sent for that pad (0 = stop).
pub const KIND_RUMBLE: u8 = 0x20;
/// Pad slots the page may use (Sunshine has 16).
pub const MAX_PADS: u8 = 4;
/// Moonlight buttonFlags bits a pad may set (0x0800 is unused in the low word).
const PAD_BUTTONS: u16 = 0xF7FF;
/// Per slot `[low, high, left trigger, right trigger]` motor values.
pub type Rumble = [[u16; 4]; MAX_PADS as usize];
/// With something held, a stream silent this long (no frame, heartbeat included) is taken as a
/// lost page: everything is released.
pub const HOLD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);
/// Queued events that may be dropped (motion, scroll, presses); releases come on top.
pub const MAX_PENDING: usize = 32;

/// Counters for the input frames from the browser.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InputStats {
    /// Frames from the browser.
    pub received: u64,
    /// Unknown kind, wrong length or a bad value: dropped.
    pub bad: u64,
    /// Motion or scroll merged into the queued one before it.
    pub coalesced: u64,
    /// Dropped because the queue was full (motion, scroll or a press, never a release).
    pub dropped: u64,
    /// Handed to the control session.
    pub sent: u64,
    /// The control session took nothing for 1 s: motion and presses dropped, releases retried.
    pub timeouts: u64,
    /// Releases the bridge made up itself (stream closed, bridge stopping).
    pub released: u64,
    /// Heartbeat frames (not counted in `received`).
    pub heartbeats: u64,
    /// Streams silent for [`HOLD_TIMEOUT`] while something was held: released.
    pub held_timeouts: u64,
    /// Other control-session errors: that event dropped.
    pub errors: u64,
}

impl std::fmt::Display for InputStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "input\treceived {} sent {} bad {} coalesced {} dropped {} timeouts {} errors {} released {} held_timeouts {} heartbeats {}",
            self.received,
            self.sent,
            self.bad,
            self.coalesced,
            self.dropped,
            self.timeouts,
            self.errors,
            self.released,
            self.held_timeouts,
            self.heartbeats
        )
    }
}

/// One frame body to an event; `None` for anything Sunshine should not get.
pub fn parse(b: &[u8]) -> Option<InputEvent> {
    let flag = |x: u8| (x <= 1).then_some(x == 1);
    let i16_at = |i: usize| i16::from_le_bytes([b[i], b[i + 1]]);
    match (b.first()?, b.len()) {
        (&KIND_KEY, 4) if (1..=0xfe).contains(&b[1]) && b[3] & !0x0f == 0 => {
            Some(InputEvent::Key {
                vk: b[1],
                down: flag(b[2])?,
                modifiers: b[3],
            })
        }
        (&KIND_MOVE, 5) => Some(InputEvent::MouseMoveRel {
            dx: i16_at(1).into(),
            dy: i16_at(3).into(),
        }),
        (&KIND_BUTTON, 3) if (1..=5).contains(&b[1]) => Some(InputEvent::MouseButton {
            button: b[1],
            down: flag(b[2])?,
        }),
        (&KIND_SCROLL, 3) => Some(InputEvent::Scroll { amount: i16_at(1) }),
        (&KIND_GAMEPAD, 14)
            if b[1] < MAX_PADS && u16::from_le_bytes([b[2], b[3]]) & !PAD_BUTTONS == 0 =>
        {
            Some(InputEvent::Gamepad {
                index: b[1],
                active_mask: 1 << b[1],
                buttons: u16::from_le_bytes([b[2], b[3]]).into(),
                lt: b[4],
                rt: b[5],
                lx: i16_at(6),
                ly: i16_at(8),
                rx: i16_at(10),
                ry: i16_at(12),
            })
        }
        (&KIND_GAMEPAD_GONE, 2) if b[1] < MAX_PADS => Some(neutral(b[1], 0)),
        _ => None,
    }
}

/// Pad `index` at rest; with its bit clear in `active_mask` this is Sunshine's removal.
fn neutral(index: u8, active_mask: u16) -> InputEvent {
    InputEvent::Gamepad {
        index,
        active_mask,
        buttons: 0,
        lt: 0,
        rt: 0,
        lx: 0,
        ly: 0,
        rx: 0,
        ry: 0,
    }
}

fn is_neutral(e: &InputEvent) -> bool {
    match *e {
        InputEvent::Gamepad {
            index, active_mask, ..
        } => *e == neutral(index, active_mask),
        _ => false,
    }
}

/// Never dropped: key-ups, button-ups and pad removals.
fn is_release(e: &InputEvent) -> bool {
    match *e {
        InputEvent::Key { down, .. } | InputEvent::MouseButton { down, .. } => !down,
        InputEvent::Gamepad {
            index, active_mask, ..
        } => active_mask & (1 << index) == 0,
        _ => false,
    }
}

/// Which browser stream an input came from ([`read_stream`] numbers them from 1; 0 is a caller
/// that does not say). What a stream pressed is its own: when it ends or goes silent only its
/// keys, buttons and pads are released, so a reloaded page's new stream keeps what it holds.
pub type StreamId = u64;

/// Mouse buttons are 1..=5; slot 0 is unused so a button indexes its own slot.
const BUTTON_SLOTS: usize = 6;

/// Input events waiting for the host, with the bookkeeping that keeps held keys, buttons and pads releasable.
#[derive(Default)]
pub struct InputQueue {
    q: VecDeque<InputEvent>,
    /// Pressed (sent or queued, not yet released), by vk, with the stream that pressed it.
    keys: Vec<(u8, StreamId)>,
    /// Pressed buttons and their stream.
    buttons: [Option<StreamId>; BUTTON_SLOTS],
    /// Every key and button pressed this session, for [`Self::shutdown`].
    ever_keys: Vec<u8>,
    ever_buttons: [bool; BUTTON_SLOTS],
    /// Per attached pad: its last state queued (mask = its own bit) and the stream that sent it.
    pads: [Option<(InputEvent, StreamId)>; MAX_PADS as usize],
    /// Pads detached since [`Self::take_detached`] (their rumble must stop).
    detached: u16,
    /// The stream of the frame being handled.
    from: StreamId,
    /// Bit per pad attached this session, for [`Self::shutdown`].
    ever_pads: u16,
    /// What the queue has counted so far.
    pub stats: InputStats,
}

impl InputQueue {
    /// One frame body from the browser.
    pub fn on_message(&mut self, b: &[u8]) {
        self.on_message_from(0, b);
    }

    /// One frame body from browser stream `stream`.
    pub fn on_message_from(&mut self, stream: StreamId, b: &[u8]) {
        self.from = stream;
        if b == [KIND_HEARTBEAT] {
            self.stats.heartbeats += 1;
            return;
        }
        self.stats.received += 1;
        match parse(b) {
            Some(e) => self.push(e),
            None => self.stats.bad += 1,
        }
    }

    /// Queues one event, merging motion and scroll into the one before it where it can.
    pub fn push(&mut self, e: InputEvent) {
        match e {
            InputEvent::Key { vk, down, .. } => {
                // A repeat press, or a release of a key that is not down (its press was dropped).
                let at = self.keys.iter().position(|k| k.0 == vk);
                if let (Some(i), true) = (at, down) {
                    self.keys[i].1 = self.from; // a repeat: the newer stream holds it now
                    return;
                }
                if at.is_none() && !down {
                    return;
                }
                if !down {
                    self.keys.retain(|k| k.0 != vk);
                } else if !self.make_room() {
                    return;
                } else {
                    self.keys.push((vk, self.from));
                    if !self.ever_keys.contains(&vk) {
                        self.ever_keys.push(vk);
                    }
                }
            }
            InputEvent::MouseButton { button, down } => {
                let b = &mut self.buttons[button as usize];
                if b.is_some() == down {
                    if down {
                        *b = Some(self.from);
                    }
                    return;
                }
                if down && !self.make_room() {
                    return;
                }
                self.buttons[button as usize] = down.then_some(self.from);
                self.ever_buttons[button as usize] |= down;
            }
            InputEvent::MouseMoveRel { dx, dy } => {
                if let Some(InputEvent::MouseMoveRel { dx: x, dy: y }) = self.q.back_mut() {
                    let m = |a: i32, b: i32| (a + b).clamp(-MAX_MOUSE_DELTA, MAX_MOUSE_DELTA);
                    (*x, *y) = (m(*x, dx), m(*y, dy));
                    self.stats.coalesced += 1;
                    return;
                }
                if !self.make_room() {
                    return;
                }
            }
            InputEvent::Scroll { amount } => {
                if let Some(InputEvent::Scroll { amount: a }) = self.q.back_mut() {
                    *a = a.saturating_add(amount);
                    self.stats.coalesced += 1;
                    return;
                }
                if !self.make_room() {
                    return;
                }
            }
            InputEvent::Gamepad { .. } => return self.push_pad(e),
        }
        self.q.push_back(e);
    }

    /// Sunshine's `activeGamepadMask`: a bit per attached pad.
    fn mask(&self) -> u16 {
        (0..MAX_PADS)
            .filter(|&i| self.pads[i as usize].is_some())
            .fold(0, |m, i| m | 1 << i)
    }

    fn push_pad(&mut self, e: InputEvent) {
        let InputEvent::Gamepad {
            index, active_mask, ..
        } = e
        else {
            return;
        };
        if index >= MAX_PADS {
            return;
        }
        let bit = 1u16 << index;
        if active_mask & bit == 0 {
            return self.detach(index);
        }
        let own = with_mask(&e, bit);
        if let Some((p, by)) = &mut self.pads[index as usize]
            && *p == own
        {
            *by = self.from; // the page's resend of a state already queued or sent
            return;
        }
        let out = with_mask(&e, self.mask() | bit);
        // Sticks and triggers: latest wins over this pad's newest queued state, if that has the
        // same buttons (Sunshine reads only the pad's own mask bit). A button change is always its own packet, so no press is lost.
        let last = self
            .q
            .iter_mut()
            .rev()
            .find(|x| matches!(x, InputEvent::Gamepad { index: i, .. } if *i == index));
        if let Some(x) = last.filter(|x| !is_release(x) && buttons(x) == buttons(&out)) {
            *x = out;
            self.stats.coalesced += 1;
        } else if self.make_room() {
            self.q.push_back(out);
        } else {
            return; // dropped: the page's next resend brings it back
        }
        self.pads[index as usize] = Some((own, self.from));
        self.ever_pads |= bit;
    }

    /// Pad `index` is gone: neutral (if it was not) while still attached, then the removal.
    fn detach(&mut self, index: u8) {
        let Some((p, _)) = self.pads[index as usize].take() else {
            return;
        };
        if !is_neutral(&p) {
            self.q.push_back(neutral(index, self.mask() | 1 << index));
        }
        self.q.push_back(neutral(index, self.mask()));
        self.detached |= 1 << index;
    }

    /// Detaches every pad (after [`Self::release_all`], so each goes neutral first).
    pub fn detach_pads(&mut self) {
        for i in 0..MAX_PADS {
            self.detach(i);
        }
    }

    /// Stream `s` ended: releases what it holds and detaches the pads it last sent.
    pub fn end_stream(&mut self, s: StreamId) {
        self.release(Some(s));
        for i in 0..MAX_PADS {
            if self.pads[i as usize].as_ref().is_some_and(|p| p.1 == s) {
                self.detach(i);
            }
        }
    }

    /// Bits of the pads detached since the last call.
    pub fn take_detached(&mut self) -> u16 {
        std::mem::take(&mut self.detached)
    }

    /// The control session did not take `e` within 1 s (`Timeout`): a release goes back to the
    /// front, and so does a pad state unless the next queued state of that pad has the same buttons
    /// (then only analog values are lost, and the newer ones follow). So a press whose release is
    /// already queued still reaches Sunshine before it. Anything else is dropped; a dropped key or
    /// button press is no longer counted as down, so the page's next repeat of it is sent.
    pub fn timed_out(&mut self, e: InputEvent) {
        self.stats.timeouts += 1;
        let retry =
            match e {
                InputEvent::Gamepad { index, .. } => is_release(&e)
                    || self
                        .q
                        .iter()
                        .find(|x| matches!(x, InputEvent::Gamepad { index: i, .. } if *i == index))
                        .is_none_or(|x| buttons(x) != buttons(&e)),
                _ => is_release(&e),
            };
        if retry {
            return self.q.push_front(e);
        }
        // A dropped press is not down on the host: forget it, so the page's next repeat goes
        // through, unless a newer event for the same key/button is still queued.
        match e {
            InputEvent::Key { vk, down: true, .. } => {
                let newer = self
                    .q
                    .iter()
                    .any(|x| matches!(x, InputEvent::Key { vk: v, .. } if *v == vk));
                if !newer {
                    self.keys.retain(|k| k.0 != vk);
                }
            }
            InputEvent::MouseButton { button, down: true } => {
                let newer = self.q.iter().any(
                    |x| matches!(x, InputEvent::MouseButton { button: b, .. } if *b == button),
                );
                if !newer {
                    self.buttons[button as usize] = None;
                }
            }
            _ => {}
        }
    }

    /// Room for one more droppable event: evicts the oldest motion or scroll if full. False (and
    /// counted) when only presses and releases are queued: the new event is dropped instead.
    fn make_room(&mut self) -> bool {
        if self.q.iter().filter(|e| !is_release(e)).count() < MAX_PENDING {
            return true;
        }
        self.stats.dropped += 1;
        let old = self.q.iter().position(|e| {
            matches!(
                e,
                InputEvent::MouseMoveRel { .. } | InputEvent::Scroll { .. }
            )
        });
        old.map(|i| self.q.remove(i)).is_some()
    }

    /// Takes the oldest queued event.
    pub fn pop(&mut self) -> Option<InputEvent> {
        self.q.pop_front()
    }

    /// Number of queued events.
    pub fn len(&self) -> usize {
        self.q.len()
    }

    /// True when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    /// A key or button is down, or a pad is off neutral.
    pub fn held(&self) -> bool {
        self.held_by(None)
    }

    /// [`Self::held`], counting only what `only` (a stream) holds, or everything for `None`.
    pub fn held_by(&self, only: Option<StreamId>) -> bool {
        let mine = |s: StreamId| only.is_none_or(|o| o == s);
        self.keys.iter().any(|k| mine(k.1))
            || self.buttons.iter().flatten().any(|&s| mine(s))
            || self
                .pads
                .iter()
                .flatten()
                .any(|p| mine(p.1) && !is_neutral(&p.0))
    }

    /// Queues a release for every key (in vk order) and button that is down, and a neutral state
    /// for every pad off neutral (still attached: the stream may come back).
    pub fn release_all(&mut self) {
        self.release(None);
    }

    /// [`Self::release_all`] for what stream `only` holds (`None`: everything).
    pub fn release(&mut self, only: Option<StreamId>) {
        let mine = |s: StreamId| only.is_none_or(|o| o == s);
        let (mut keys, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.keys)
            .into_iter()
            .partition(|k| mine(k.1));
        self.keys = kept;
        keys.sort_unstable();
        for (vk, _) in keys {
            self.stats.released += 1;
            self.q.push_back(InputEvent::Key {
                vk,
                down: false,
                modifiers: 0,
            });
        }
        for button in 1..self.buttons.len() as u8 {
            if self.buttons[button as usize].is_some_and(mine) {
                self.buttons[button as usize] = None;
                self.stats.released += 1;
                self.q.push_back(InputEvent::MouseButton {
                    button,
                    down: false,
                });
            }
        }
        for i in 0..MAX_PADS {
            if let Some((p, s)) = &mut self.pads[i as usize] {
                if !mine(*s) || is_neutral(p) {
                    continue;
                }
                *p = neutral(i, 1 << i);
                self.stats.released += 1;
                self.q.push_back(neutral(i, self.mask()));
            }
        }
    }

    /// The bridge is stopping: a release for every key and button pressed this session (in vk
    /// order, then buttons), and an empty queue. A superset on purpose: a release the send loop
    /// already handed to `ControlHandle` may still sit unsent in its channel (up to 64 commands)
    /// when the control loop stops, so "released" is never known for sure. Releasing a key the
    /// host does not hold does nothing. Then a removal for every pad attached this session (not a
    /// neutral state: with its bit set that would attach a fresh pad); Sunshine frees the pad.
    pub fn shutdown(&mut self) -> Vec<InputEvent> {
        self.release_all();
        self.q.clear();
        let mut keys = std::mem::take(&mut self.ever_keys);
        keys.sort_unstable();
        let keys = keys.into_iter().map(|vk| InputEvent::Key {
            vk,
            down: false,
            modifiers: 0,
        });
        let buttons = (1..BUTTON_SLOTS as u8)
            .filter(|&b| std::mem::take(&mut self.ever_buttons[b as usize]))
            .map(|button| InputEvent::MouseButton {
                button,
                down: false,
            });
        let ever = std::mem::take(&mut self.ever_pads);
        self.pads = Default::default();
        let pads = (0..MAX_PADS)
            .filter(|i| ever & 1 << i != 0)
            .map(|i| neutral(i, 0));
        keys.chain(buttons.collect::<Vec<_>>())
            .chain(pads)
            .collect()
    }
}

fn with_mask(e: &InputEvent, m: u16) -> InputEvent {
    let mut e = e.clone();
    if let InputEvent::Gamepad { active_mask, .. } = &mut e {
        *active_mask = m;
    }
    e
}

fn buttons(e: &InputEvent) -> Option<u32> {
    match *e {
        InputEvent::Gamepad { buttons, .. } => Some(buttons),
        _ => None,
    }
}

/// The queue shared by the browser stream readers and the one send loop, and the rumble state
/// the control session feeds and the stream writers read.
#[derive(Default)]
pub struct Shared {
    q: Mutex<InputQueue>,
    wake: Notify,
    rumble: watch::Sender<Rumble>,
    streams: AtomicU64,
}

impl Shared {
    /// A `Rumble` or `RumbleTriggers` from Sunshine (its `controller` is our pad slot); anything
    /// else, or a slot the page cannot have, is ignored.
    pub fn rumble(&self, m: &ControlMsg) {
        let (c, at, v) = match *m {
            ControlMsg::Rumble {
                controller,
                low,
                high,
            } => (controller, 0, [low, high]),
            ControlMsg::RumbleTriggers {
                controller,
                left,
                right,
            } => (controller, 2, [left, right]),
            _ => return,
        };
        if c >= MAX_PADS as u16 {
            return;
        }
        self.rumble.send_if_modified(|r| {
            let s = &mut r[c as usize][at..at + 2];
            let changed = *s != v;
            s.copy_from_slice(&v);
            changed
        });
    }

    /// The latest rumble state from the host. Do not hold the guard across an `.await`.
    pub fn rumble_state(&self) -> watch::Ref<'_, Rumble> {
        self.rumble.borrow()
    }

    /// A receiver that wakes when the rumble state changes.
    pub fn rumble_rx(&self) -> watch::Receiver<Rumble> {
        self.rumble.subscribe()
    }

    /// Runs `f` on the queue under its lock.
    pub fn with<R>(&self, f: impl FnOnce(&mut InputQueue) -> R) -> R {
        f(&mut self.q.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// [`Self::with`], then wakes the send loop and stops the rumble of pads just detached
    /// (Sunshine sends no stop for a pad it freed).
    fn push(&self, f: impl FnOnce(&mut InputQueue)) {
        let gone = self.with(|q| {
            f(q);
            q.take_detached()
        });
        self.wake.notify_one();
        if gone != 0 {
            self.rumble.send_if_modified(|r| {
                let mut changed = false;
                for (i, v) in r.iter_mut().enumerate() {
                    if gone & 1 << i != 0 && *v != [0; 4] {
                        (*v, changed) = ([0; 4], true);
                    }
                }
                changed
            });
        }
    }
}

/// A body as one frame on the wire (bodies are at most 255 bytes).
pub fn frame(body: &[u8]) -> Vec<u8> {
    let mut f = vec![body.len() as u8];
    f.extend_from_slice(body);
    f
}

/// One [`KIND_RUMBLE`] frame.
pub fn rumble_frame(slot: u8, v: [u16; 4]) -> Vec<u8> {
    let mut b = vec![KIND_RUMBLE, slot];
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    frame(&b)
}

/// Writes a [`KIND_RUMBLE`] frame for each pad whose motors changed (on start: each that is not
/// still), until the write fails. Only the newest values are sent, so a slow page cannot back up.
pub async fn write_rumble<W: AsyncWrite + Unpin>(mut w: W, mut rx: watch::Receiver<Rumble>) {
    let mut last = Rumble::default();
    rx.mark_changed();
    while rx.changed().await.is_ok() {
        let now = *rx.borrow_and_update();
        for (i, (n, l)) in now.iter().zip(&last).enumerate() {
            if n != l && w.write_all(&rumble_frame(i as u8, *n)).await.is_err() {
                return;
            }
        }
        last = now;
    }
}

/// Reads frames until the stream ends or fails, then releases everything held down. While
/// something is held, a stream silent for [`HOLD_TIMEOUT`] (a lost network, a frozen or closed
/// laptop: the QUIC idle timeout alone would take 10 s while Sunshine repeats the key) also
/// releases everything. The timer runs only while something is held. At the end the pads it
/// last sent are detached and their rumble stopped. Both act only on what this stream holds: a
/// newer stream (a reloaded page) keeps its keys and pads.
pub async fn read_stream<R: AsyncRead + Unpin>(mut r: R, sh: &Shared) {
    let id = sh.streams.fetch_add(1, Ordering::Relaxed) + 1;
    let mut b = [0u8; 255];
    loop {
        let n = {
            // One frame read, kept across timer firings (a frame read is not cancel-safe).
            let f = read_frame(&mut r, &mut b);
            tokio::pin!(f);
            loop {
                let held = sh.with(|q| q.held_by(Some(id)));
                tokio::select! {
                    n = &mut f => break n,
                    _ = tokio::time::sleep(HOLD_TIMEOUT), if held => sh.push(|q| {
                        q.stats.held_timeouts += 1;
                        q.release(Some(id));
                    }),
                }
            }
        };
        let Some(n) = n else { break };
        sh.push(|q| q.on_message_from(id, &b[..n]));
    }
    sh.push(|q| q.end_stream(id));
}

async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, b: &mut [u8; 255]) -> Option<usize> {
    let n = r.read_u8().await.ok()? as usize;
    r.read_exact(&mut b[..n]).await.ok()?;
    Some(n)
}

/// Hands queued events to `send` (the control session) one by one, until it returns `Closed`.
/// `Timeout` (the session took nothing for 1 s): [`InputQueue::timed_out`] (releases and pad
/// button changes are retried, the rest dropped). Any other error drops the event (no retry, so
/// it cannot spin).
pub async fn send_loop<F, Fut>(sh: &Shared, mut send: F) -> brisk_client::Result<()>
where
    F: FnMut(InputEvent) -> Fut,
    Fut: Future<Output = brisk_client::Result<()>>,
{
    loop {
        let wake = sh.wake.notified();
        let Some(e) = sh.with(InputQueue::pop) else {
            wake.await;
            continue;
        };
        match send(e.clone()).await {
            Ok(()) => sh.with(|q| q.stats.sent += 1),
            Err(err @ Error::Closed(_)) => return Err(err),
            Err(err @ Error::Timeout(_)) => sh.with(|q| {
                tracing::warn!("input: {err}");
                q.timed_out(e);
            }),
            Err(err) => sh.with(|q| {
                tracing::warn!("input dropped: {err}");
                q.stats.errors += 1;
            }),
        }
    }
}

/// Reads every input stream the viewer opens until its connection ends; rumble goes back on
/// each.
pub async fn serve(s: Arc<wt::Session>, sh: Arc<Shared>) {
    while let Ok((send, mut recv)) = s.accept_bi().await {
        let sh = sh.clone();
        tokio::spawn(async move {
            if wt::is_webtransport_bi(&mut recv).await {
                let w = tokio::spawn(write_rumble(send, sh.rumble_rx()));
                read_stream(recv, &sh).await;
                w.abort();
            }
        });
    }
}
