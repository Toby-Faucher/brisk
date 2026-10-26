//! Browser gamepad frames to Moonlight multi-controller packets, the queue's latest-state-wins
//! rules, release on close/silence/shutdown, and rumble back to the page.

mod common;

use std::sync::Arc;

use brisk_bridge::input::{
    HOLD_TIMEOUT, InputQueue, KIND_GAMEPAD, KIND_GAMEPAD_GONE, KIND_HEARTBEAT, KIND_KEY,
    KIND_RUMBLE, MAX_PADS, MAX_PENDING, Shared, frame, read_stream, rumble_frame, send_loop, serve,
    write_rumble,
};
use brisk_client::Error;
use brisk_proto::control::ControlMsg;
use brisk_proto::input::{InputEvent, encode_input};
use common::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{sleep, timeout};

#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct St {
    buttons: u16,
    lt: u8,
    rt: u8,
    lx: i16,
    ly: i16,
    rx: i16,
    ry: i16,
}

fn body(slot: u8, s: St) -> Vec<u8> {
    let mut b = vec![KIND_GAMEPAD, slot];
    b.extend_from_slice(&s.buttons.to_le_bytes());
    b.extend_from_slice(&[s.lt, s.rt]);
    for v in [s.lx, s.ly, s.rx, s.ry] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}
fn ev(index: u8, mask: u16, s: St) -> InputEvent {
    InputEvent::Gamepad {
        index,
        active_mask: mask,
        buttons: s.buttons as u32,
        lt: s.lt,
        rt: s.rt,
        lx: s.lx,
        ly: s.ly,
        rx: s.rx,
        ry: s.ry,
    }
}
const N: St = St {
    buttons: 0,
    lt: 0,
    rt: 0,
    lx: 0,
    ly: 0,
    rx: 0,
    ry: 0,
};
const A: St = St {
    buttons: 0x1000,
    ..N
};
fn drain(q: &mut InputQueue) -> Vec<InputEvent> {
    std::iter::from_fn(|| q.pop()).collect()
}

#[test]
fn parses_full_state_into_a_multi_controller_packet() {
    let mut q = InputQueue::default();
    let s = St {
        buttons: 0x1000 | 0x0001 | 0x0400,
        lt: 7,
        rt: 255,
        lx: -32768,
        ly: 32767,
        rx: 1,
        ry: -2,
    };
    q.on_message(&body(1, s));
    let e = q.pop().unwrap();
    assert_eq!(e, ev(1, 0b10, s));
    // On the wire to Sunshine: magic 0x0C, controllerNumber 1, activeGamepadMask 0b10.
    let p = &encode_input(&e).unwrap()[0];
    assert_eq!(u32::from_le_bytes(p[4..8].try_into().unwrap()), 0x0C);
    assert_eq!(&p[10..14], &[1, 0, 2, 0]);
    assert_eq!(q.stats.bad, 0);
}

#[test]
fn malformed_and_out_of_range_frames_are_counted_bad() {
    let mut q = InputQueue::default();
    let good = body(0, A);
    for b in [
        good[..13].to_vec(),              // short
        [good.clone(), vec![0]].concat(), // long
        body(MAX_PADS, A),                // slot 4: only 0..4
        body(0xFF, A),
        body(
            0,
            St {
                buttons: 0x0800,
                ..N
            },
        ), // not a Moonlight button bit
        vec![KIND_GAMEPAD_GONE, MAX_PADS],
        vec![KIND_GAMEPAD_GONE],
        vec![KIND_GAMEPAD_GONE, 0, 0],
    ] {
        q.on_message(&b);
    }
    assert_eq!(q.stats.bad, 8);
    assert!(q.is_empty());
    assert!(!q.held());
}

#[test]
fn latest_analog_state_wins_per_controller_but_button_changes_are_kept() {
    let mut q = InputQueue::default();
    let stick = |lx| St { lx, ..A };
    q.on_message(&body(0, stick(100)));
    q.on_message(&body(1, stick(5)));
    q.on_message(&body(0, stick(200))); // same buttons: replaces pad 0's queued state
    q.on_message(&body(0, stick(300)));
    q.on_message(&body(0, St { lx: 300, ..N })); // A released: its own event
    q.on_message(&body(0, St { lx: 400, ..N }));
    let got = drain(&mut q);
    assert_eq!(
        got,
        vec![
            ev(0, 0b11, stick(300)),
            ev(1, 0b11, stick(5)),
            ev(0, 0b11, St { lx: 400, ..N }),
        ],
        "the merged state carries the newest mask"
    );
    // A press and release inside one send window: both reach Sunshine.
    q.on_message(&body(0, A));
    q.on_message(&body(0, N));
    assert_eq!(drain(&mut q), vec![ev(0, 0b11, A), ev(0, 0b11, N)]);
    assert_eq!(q.stats.coalesced, 3);
}

#[test]
fn a_resent_identical_state_is_not_sent_again() {
    let mut q = InputQueue::default();
    q.on_message(&body(0, A));
    assert_eq!(q.pop(), Some(ev(0, 1, A)));
    for _ in 0..5 {
        q.on_message(&body(0, A)); // the page's 100 ms redundancy
    }
    assert!(q.is_empty());
    assert_eq!(q.stats.received, 6);
}

#[test]
fn gone_sends_neutral_then_removal() {
    let mut q = InputQueue::default();
    q.on_message(&body(2, A));
    q.on_message(&body(0, N));
    q.on_message(&[KIND_GAMEPAD_GONE, 2]);
    q.on_message(&[KIND_GAMEPAD_GONE, 2]); // already gone: nothing
    q.on_message(&[KIND_GAMEPAD_GONE, 3]); // never attached: nothing
    assert_eq!(
        drain(&mut q),
        vec![
            ev(2, 0b100, A),
            ev(0, 0b101, N),
            ev(2, 0b101, N), // released while still attached
            ev(2, 0b001, N), // removal: pad 2's bit clear (Sunshine frees it)
        ]
    );
    assert!(!q.held());
}

#[tokio::test]
async fn stream_close_releases_and_detaches_every_pad_and_stops_rumble() {
    let sh = Shared::default();
    sh.rumble(&ControlMsg::Rumble {
        controller: 0,
        low: 9,
        high: 9,
    });
    let mut wire = Vec::new();
    for m in [body(0, A), body(1, N)] {
        wire.extend_from_slice(&frame(&m));
    }
    read_stream(&wire[..], &sh).await;
    assert_eq!(
        sh.with(drain),
        vec![
            ev(0, 0b01, A),
            ev(1, 0b11, N),
            ev(0, 0b11, N),
            ev(0, 0b10, N), // pad 0 removed
            ev(1, 0b00, N), // pad 1 removed
        ]
    );
    assert_eq!(sh.with(|q| q.stats.released), 1);
    assert_eq!(
        *sh.rumble_state(),
        [[0u16; 4]; 4],
        "no stale rumble for the next viewer"
    );
}

#[tokio::test(start_paused = true)]
async fn held_pad_and_silence_goes_neutral_but_stays_attached() {
    let sh = Arc::new(Shared::default());
    let (mut tx, rx) = tokio::io::duplex(4096);
    let _t = tokio::spawn({
        let sh = sh.clone();
        async move { read_stream(rx, &sh).await }
    });
    tx.write_all(&frame(&body(0, A))).await.unwrap();
    for _ in 0..8 {
        sleep(Duration::from_millis(100)).await; // the page's redundant resend
        tx.write_all(&frame(&body(0, A))).await.unwrap();
    }
    assert_eq!(sh.with(|q| q.stats.held_timeouts), 0);
    sleep(HOLD_TIMEOUT + Duration::from_millis(50)).await;
    assert_eq!(sh.with(drain), vec![ev(0, 1, A), ev(0, 1, N)]);
    assert_eq!(sh.with(|q| q.stats.held_timeouts), 1);
    // Idle keepalives on a neutral pad arm nothing.
    for _ in 0..8 {
        tx.write_all(&frame(&[KIND_HEARTBEAT])).await.unwrap();
        sleep(Duration::from_millis(250)).await;
    }
    sh.with(|q| {
        assert!(q.is_empty());
        assert_eq!(q.stats.held_timeouts, 1);
    });
    // The stream is back: the next state is sent (not taken for a resend of the old one).
    tx.write_all(&frame(&body(0, A))).await.unwrap();
    sleep(Duration::from_millis(10)).await;
    assert_eq!(sh.with(drain), vec![ev(0, 1, A)]);
}

#[test]
fn shutdown_removes_every_pad_ever_attached() {
    let mut q = InputQueue::default();
    q.on_message(&body(0, A));
    q.on_message(&body(3, N));
    q.on_message(&[KIND_GAMEPAD_GONE, 3]);
    drain(&mut q);
    // Removal only: a neutral with the bit set would attach a fresh pad on Sunshine for a moment.
    assert_eq!(q.shutdown(), vec![ev(0, 0, N), ev(3, 0, N)]);
    assert!(q.shutdown().is_empty());
}

#[tokio::test]
async fn timed_out_pad_state_is_retried_like_a_key_up() {
    let sh = Shared::default();
    sh.with(|q| {
        q.on_message(&body(0, A));
        q.on_message(&body(1, A));
        q.on_message(&body(0, N)); // A's release, already queued behind it
    });
    let mut calls = Vec::new();
    let r = send_loop(&sh, |e| {
        calls.push(e);
        let n = calls.len();
        async move {
            match n {
                1 => Err(Error::Timeout("not drained".into())),
                2..=4 => Ok(()),
                _ => Err(Error::Closed("control session")),
            }
        }
    });
    sh.with(|q| q.on_message(&body(2, A))); // ends the loop on the 5th call
    timeout(SECS2, r).await.unwrap().unwrap_err();
    assert_eq!(
        calls,
        vec![
            ev(0, 1, A),
            ev(0, 1, A),
            ev(1, 3, A),
            ev(0, 3, N),
            ev(2, 7, A)
        ],
        "the press is retried although its release is queued: both reach Sunshine"
    );
    assert_eq!(sh.with(|q| q.stats.timeouts), 1);
}

#[test]
fn timed_out_analog_only_state_gives_way_to_a_newer_one() {
    let mut q = InputQueue::default();
    q.on_message(&body(0, St { lx: 100, ..A }));
    let e = q.pop().unwrap();
    q.on_message(&body(0, St { lx: 200, ..A }));
    q.timed_out(e);
    assert_eq!(drain(&mut q), vec![ev(0, 1, St { lx: 200, ..A })]);
}

#[test]
fn a_pad_release_dropped_by_a_full_queue_comes_back_with_the_resend() {
    let mut q = InputQueue::default();
    q.on_message(&body(0, A));
    assert_eq!(q.pop(), Some(ev(0, 1, A)));
    for vk in 0..MAX_PENDING as u8 {
        q.on_message(&[KIND_KEY, 0x50 + vk, 1, 0]); // 32 presses: full
    }
    q.on_message(&body(0, N)); // A released: no room
    assert_eq!(q.stats.dropped, 1);
    let got = drain(&mut q);
    assert!(!got.contains(&ev(0, 1, N)));
    q.on_message(&body(0, N)); // the page's resend 100 ms later
    assert_eq!(drain(&mut q), vec![ev(0, 1, N)], "not taken for a repeat");
}

/// Sunshine's view of pads 0 and 1, and how often pad 0's A changed there.
#[derive(Default)]
struct Host {
    pads: [St; 2],
    a_edges: u32,
}
impl Host {
    fn take(&mut self, e: &InputEvent) {
        let InputEvent::Gamepad {
            index,
            active_mask,
            buttons,
            lt,
            rt,
            lx,
            ly,
            rx,
            ry,
        } = *e
        else {
            return;
        };
        let new = if active_mask & (1 << index) == 0 {
            N
        } else {
            St {
                buttons: buttons as u16,
                lt,
                rt,
                lx,
                ly,
                rx,
                ry,
            }
        };
        let old = &mut self.pads[index as usize];
        if index == 0 && (old.buttons ^ new.buttons) & 0x1000 != 0 {
            self.a_edges += 1;
        }
        *old = new;
    }

    /// The send loop's turn: each event reaches Sunshine, or (`timeout_pct` of the time) the
    /// control session does not take it in time, and the loop waits for the next turn.
    fn drain(&mut self, q: &mut InputQueue, r: &mut Rng, timeout_pct: u64) {
        while let Some(e) = q.pop() {
            if r.next() % 100 < timeout_pct {
                q.timed_out(e);
                return;
            }
            self.take(&e);
        }
    }
}

/// Deterministic xorshift, so the pattern is the same every run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn every_a_press_and_release_reaches_sunshine_despite_merging_and_timeouts() {
    let mut q = InputQueue::default();
    let mut r = Rng(0x2545F4914F6CDD1D);
    let mut host = Host::default();
    let mut page_edges = 0;
    for i in 0..500i16 {
        // A tap inside one send window, with stick motion before, between and after.
        q.on_message(&body(0, St { lx: i, ..N }));
        q.on_message(&body(0, St { lx: i, ..A }));
        q.on_message(&body(0, St { lx: i + 1, ..A }));
        q.on_message(&body(0, St { lx: i + 2, ..N }));
        q.on_message(&body(0, St { lx: i + 3, ..N }));
        page_edges += 2;
        host.drain(&mut q, &mut r, 20);
    }
    for _ in 0..50 {
        host.drain(&mut q, &mut r, 20);
    }
    assert!(
        q.stats.timeouts > 100,
        "timeouts happened ({})",
        q.stats.timeouts
    );
    assert!(q.stats.coalesced > 100);
    assert_eq!(host.a_edges, page_edges);
}

/// The M3 gate at the queue level, through the paths that really drop pad states: 5% of sends
/// time out (`timed_out`), and key bursts fill the queue so pad states are dropped there
/// (`make_room`). The page resends every 100 ms; once the player lets go, nothing stays pressed.
#[test]
fn five_percent_loss_leaves_nothing_stuck() {
    let mut q = InputQueue::default();
    let mut r = Rng(0x9E3779B97F4A7C15);
    let mut page = [N; 2];
    let mut host = Host::default();
    // 4 ms polls for 60 s; buttons change now and then, sticks often.
    for tick in 0..15_000u32 {
        let slot = (r.next() % 2) as usize;
        let mut changed = false;
        if r.next().is_multiple_of(20) {
            page[slot].buttons ^= [0x1000u16, 0x2000, 0x0001, 0x0100][(r.next() % 4) as usize];
            changed = true;
        }
        if r.next().is_multiple_of(3) {
            page[slot].lx = (r.next() % 65536) as u16 as i16;
            page[slot].rt = r.next() as u8;
            changed = true;
        }
        // Now and then someone mashes the keyboard: 40 presses fill the queue for a moment.
        let burst = tick % 500 == 7;
        if burst {
            for vk in 0..40 {
                q.on_message(&[KIND_KEY, 0x50 + vk, 1, 0]);
            }
        }
        let resend = tick.is_multiple_of(25); // 100 ms
        for (s, st) in page.iter().enumerate() {
            if (changed && s == slot) || resend {
                q.on_message(&body(s as u8, *st));
            }
        }
        if tick % 500 == 9 {
            for vk in 0..40 {
                q.on_message(&[KIND_KEY, 0x50 + vk, 0, 0]);
            }
        }
        if tick % 2 == 0 && !burst {
            host.drain(&mut q, &mut r, 5); // the send loop drains about every 8 ms
        }
    }
    // The player lets go of everything; the page keeps resending for 300 ms.
    page = [N; 2];
    for tick in 0..75u32 {
        for (s, st) in page.iter().enumerate() {
            if tick.is_multiple_of(25) {
                q.on_message(&body(s as u8, *st));
            }
        }
        host.drain(&mut q, &mut r, 5);
    }
    for _ in 0..20 {
        host.drain(&mut q, &mut r, 5);
    }
    assert!(q.stats.timeouts > 100, "timeouts ({})", q.stats.timeouts);
    assert!(
        q.stats.dropped > 10,
        "queue-full drops ({})",
        q.stats.dropped
    );
    assert_eq!(host.pads, [N; 2], "nothing stuck");
}

#[tokio::test(start_paused = true)]
async fn an_old_stream_ending_leaves_the_new_streams_keys_and_pads() {
    let sh = Arc::new(Shared::default());
    let open = || {
        let (tx, rx) = tokio::io::duplex(4096);
        let sh = sh.clone();
        tokio::spawn(async move { read_stream(rx, &sh).await });
        tx
    };
    let tick = || sleep(Duration::from_millis(10));
    let key = |vk: u8, down: bool| frame(&[KIND_KEY, vk, down as u8, 0]);
    let k = |vk: u8, down: bool| InputEvent::Key {
        vk,
        down,
        modifiers: 0,
    };
    let mut old = open();
    old.write_all(&key(0x41, true)).await.unwrap();
    old.write_all(&frame(&body(0, A))).await.unwrap();
    tick().await;
    // The page reloads: its new stream takes pad 0 over and attaches pad 1.
    let mut new = open();
    new.write_all(&key(0x42, true)).await.unwrap();
    new.write_all(&frame(&body(0, A))).await.unwrap();
    new.write_all(&frame(&body(1, A))).await.unwrap();
    tick().await;
    assert_eq!(
        sh.with(drain),
        vec![k(0x41, true), ev(0, 1, A), k(0x42, true), ev(1, 3, A)]
    );
    sh.rumble(&ControlMsg::Rumble {
        controller: 0,
        low: 5,
        high: 5,
    });
    // The old stream goes silent; the new one keeps its heartbeats going.
    for _ in 0..6 {
        sleep(Duration::from_millis(250)).await;
        new.write_all(&frame(&[KIND_HEARTBEAT])).await.unwrap();
    }
    assert_eq!(
        sh.with(drain),
        vec![k(0x41, false)],
        "only the old stream's key"
    );
    assert_eq!(sh.with(|q| q.stats.held_timeouts), 1);
    drop(old);
    tick().await;
    assert_eq!(sh.with(drain), vec![], "pad 0 is the new stream's now");
    assert_eq!(sh.rumble_state()[0], [5, 5, 0, 0], "its rumble goes on");
    drop(new);
    tick().await;
    assert_eq!(
        sh.with(drain),
        vec![
            k(0x42, false),
            ev(0, 3, N),
            ev(1, 3, N),
            ev(0, 2, N),
            ev(1, 0, N)
        ]
    );
    assert_eq!(sh.rumble_state()[0], [0; 4]);
}

#[test]
fn rumble_frames_are_len_kind_slot_and_four_u16() {
    assert_eq!(
        rumble_frame(1, [0x1234, 0xFFFF, 0, 2]),
        vec![10, KIND_RUMBLE, 1, 0x34, 0x12, 0xFF, 0xFF, 0, 0, 2, 0]
    );
}

#[tokio::test]
async fn rumble_from_sunshine_reaches_the_page_per_slot() {
    let sh = Arc::new(Shared::default());
    let (w, mut r) = tokio::io::duplex(4096);
    tokio::spawn(write_rumble(w, sh.rumble_rx()));
    sh.rumble(&ControlMsg::Rumble {
        controller: 1,
        low: 0x1234,
        high: 0xFFFF,
    });
    let mut b = vec![0u8; 11];
    timeout(SECS2, r.read_exact(&mut b)).await.unwrap().unwrap();
    assert_eq!(b, rumble_frame(1, [0x1234, 0xFFFF, 0, 0]));
    sh.rumble(&ControlMsg::RumbleTriggers {
        controller: 1,
        left: 5,
        right: 6,
    });
    sh.rumble(&ControlMsg::Rumble {
        controller: MAX_PADS as u16, // not a slot the page has: ignored
        low: 1,
        high: 1,
    });
    timeout(SECS2, r.read_exact(&mut b)).await.unwrap().unwrap();
    assert_eq!(b, rumble_frame(1, [0x1234, 0xFFFF, 5, 6]));
    sh.rumble(&ControlMsg::Rumble {
        controller: 1,
        low: 0,
        high: 0,
    });
    timeout(SECS2, r.read_exact(&mut b)).await.unwrap().unwrap();
    assert_eq!(b, rumble_frame(1, [0, 0, 5, 6]), "a stop goes through too");
}

#[tokio::test]
async fn rumble_goes_back_on_the_browser_input_stream() {
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let sh = Arc::new(Shared::default());
    tokio::spawn(serve(Arc::new(s), sh.clone()));
    let (mut tx, mut rx) = c.open_bi().await.unwrap();
    tx.write_all(&[0x40, 0x41, 0x00]).await.unwrap(); // WEBTRANSPORT_STREAM, session id 0
    tx.write_all(&frame(&body(0, A))).await.unwrap();
    timeout(SECS2, async {
        while sh.with(|q| q.len()) < 1 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    sh.rumble(&ControlMsg::Rumble {
        controller: 0,
        low: 0x8000,
        high: 0x4000,
    });
    let mut b = vec![0u8; 11];
    timeout(SECS2, rx.read_exact(&mut b))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(b, rumble_frame(0, [0x8000, 0x4000, 0, 0]));
}
