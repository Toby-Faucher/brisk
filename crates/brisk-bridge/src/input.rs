//! Browser keyboard and mouse to Sunshine.
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

use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};

use brisk_client::Error;
use brisk_proto::input::{InputEvent, MAX_MOUSE_DELTA};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::Notify;

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
/// With something held, a stream silent this long (no frame, heartbeat included) is taken as a
/// lost page: everything is released.
pub const HOLD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);
/// Queued events that may be dropped (motion, scroll, presses); releases come on top.
pub const MAX_PENDING: usize = 32;

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
        _ => None,
    }
}

fn is_release(e: &InputEvent) -> bool {
    matches!(
        e,
        InputEvent::Key { down: false, .. } | InputEvent::MouseButton { down: false, .. }
    )
}

#[derive(Default)]
pub struct InputQueue {
    q: VecDeque<InputEvent>,
    /// Pressed (sent or queued, not yet released), by vk.
    keys: Vec<u8>,
    buttons: [bool; 6],
    /// Every key and button pressed this session, for [`Self::shutdown`].
    ever_keys: Vec<u8>,
    ever_buttons: [bool; 6],
    pub stats: InputStats,
}

impl InputQueue {
    /// One frame body from the browser.
    pub fn on_message(&mut self, b: &[u8]) {
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

    pub fn push(&mut self, e: InputEvent) {
        match e {
            InputEvent::Key { vk, down, .. } => {
                // A repeat press, or a release of a key that is not down (its press was dropped).
                if self.keys.contains(&vk) == down {
                    return;
                }
                if !down {
                    self.keys.retain(|&k| k != vk);
                } else if !self.make_room() {
                    return;
                } else {
                    self.keys.push(vk);
                    if !self.ever_keys.contains(&vk) {
                        self.ever_keys.push(vk);
                    }
                }
            }
            InputEvent::MouseButton { button, down } => {
                let b = &mut self.buttons[button as usize];
                if *b == down {
                    return;
                }
                if down && !self.make_room() {
                    return;
                }
                self.buttons[button as usize] = down;
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
            InputEvent::Gamepad { .. } => return,
        }
        self.q.push_back(e);
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

    pub fn pop(&mut self) -> Option<InputEvent> {
        self.q.pop_front()
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    /// A key or button is down.
    pub fn held(&self) -> bool {
        !self.keys.is_empty() || self.buttons.contains(&true)
    }

    /// Queues a release for every key (in vk order) and button that is down.
    pub fn release_all(&mut self) {
        let mut keys = std::mem::take(&mut self.keys);
        keys.sort_unstable();
        for vk in keys {
            self.stats.released += 1;
            self.q.push_back(InputEvent::Key {
                vk,
                down: false,
                modifiers: 0,
            });
        }
        for button in 1..self.buttons.len() as u8 {
            if std::mem::take(&mut self.buttons[button as usize]) {
                self.stats.released += 1;
                self.q.push_back(InputEvent::MouseButton {
                    button,
                    down: false,
                });
            }
        }
    }

    /// The bridge is stopping: a release for every key and button pressed this session (in vk
    /// order, then buttons), and an empty queue. A superset on purpose: a release the send loop
    /// already handed to `ControlHandle` may still sit unsent in its channel (up to 64 commands)
    /// when the control loop stops, so "released" is never known for sure. Releasing a key the
    /// host does not hold does nothing.
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
        let buttons = (1..6u8)
            .filter(|&b| std::mem::take(&mut self.ever_buttons[b as usize]))
            .map(|button| InputEvent::MouseButton {
                button,
                down: false,
            });
        keys.chain(buttons.collect::<Vec<_>>()).collect()
    }
}

/// The queue shared by the browser stream readers and the one send loop.
#[derive(Default)]
pub struct Shared {
    q: Mutex<InputQueue>,
    wake: Notify,
}

impl Shared {
    pub fn with<R>(&self, f: impl FnOnce(&mut InputQueue) -> R) -> R {
        f(&mut self.q.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// [`Self::with`], then wakes the send loop.
    fn push(&self, f: impl FnOnce(&mut InputQueue)) {
        self.with(f);
        self.wake.notify_one();
    }
}

/// A body as one frame on the wire (bodies are at most 255 bytes).
pub fn frame(body: &[u8]) -> Vec<u8> {
    let mut f = vec![body.len() as u8];
    f.extend_from_slice(body);
    f
}

/// Reads frames until the stream ends or fails, then releases everything held down. While
/// something is held, a stream silent for [`HOLD_TIMEOUT`] (a lost network, a frozen or closed
/// laptop: the QUIC idle timeout alone would take 10 s while Sunshine repeats the key) also
/// releases everything. The timer runs only while something is held.
pub async fn read_stream<R: AsyncRead + Unpin>(mut r: R, sh: &Shared) {
    let mut b = [0u8; 255];
    loop {
        let n = {
            // One frame read, kept across timer firings (a frame read is not cancel-safe).
            let f = read_frame(&mut r, &mut b);
            tokio::pin!(f);
            loop {
                let held = sh.with(|q| q.held());
                tokio::select! {
                    n = &mut f => break n,
                    _ = tokio::time::sleep(HOLD_TIMEOUT), if held => sh.push(|q| {
                        q.stats.held_timeouts += 1;
                        q.release_all();
                    }),
                }
            }
        };
        let Some(n) = n else { break };
        sh.push(|q| q.on_message(&b[..n]));
    }
    sh.push(InputQueue::release_all);
}

async fn read_frame<R: AsyncRead + Unpin>(r: &mut R, b: &mut [u8; 255]) -> Option<usize> {
    let n = r.read_u8().await.ok()? as usize;
    r.read_exact(&mut b[..n]).await.ok()?;
    Some(n)
}

/// Hands queued events to `send` (the control session) one by one, until it returns `Closed`.
/// `Timeout` (the session took nothing for 1 s) drops that event, except a release, which goes
/// back to the front of the queue and is retried. Any other error drops the event (no retry, so
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
                q.stats.timeouts += 1;
                if is_release(&e) {
                    q.q.push_front(e);
                }
            }),
            Err(err) => sh.with(|q| {
                tracing::warn!("input dropped: {err}");
                q.stats.errors += 1;
            }),
        }
    }
}

/// Reads every input stream the viewer opens until its connection ends.
pub async fn serve(s: Arc<wt::Session>, sh: Arc<Shared>) {
    while let Ok((send, mut recv)) = s.accept_bi().await {
        let sh = sh.clone();
        tokio::spawn(async move {
            let _send = send; // kept open while reading
            if wt::is_webtransport_bi(&mut recv).await {
                read_stream(recv, &sh).await;
            }
        });
    }
}
