//! Browser input messages to Moonlight input events: the mapping, the bounded queue between the
//! browser's stream and the control session, release-all, and the send loop's error handling.

mod common;

use std::sync::Arc;

use brisk_bridge::input::{
    HOLD_TIMEOUT, InputQueue, KIND_BUTTON, KIND_HEARTBEAT, KIND_KEY, KIND_MOVE, KIND_SCROLL,
    MAX_PENDING, Shared, frame, read_stream, send_loop, serve,
};
use brisk_client::Error;
use brisk_proto::input::{InputEvent, encode_input};
use common::*;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::time::{sleep, timeout};

fn key(vk: u8, down: bool, mods: u8) -> Vec<u8> {
    vec![KIND_KEY, vk, down as u8, mods]
}
fn mv(dx: i16, dy: i16) -> Vec<u8> {
    let mut b = vec![KIND_MOVE];
    b.extend_from_slice(&dx.to_le_bytes());
    b.extend_from_slice(&dy.to_le_bytes());
    b
}
fn button(b: u8, down: bool) -> Vec<u8> {
    vec![KIND_BUTTON, b, down as u8]
}
fn drain(q: &mut InputQueue) -> Vec<InputEvent> {
    std::iter::from_fn(|| q.pop()).collect()
}
const fn k(vk: u8, down: bool) -> InputEvent {
    InputEvent::Key {
        vk,
        down,
        modifiers: 0,
    }
}

#[test]
fn maps_keydown_to_vk() {
    let mut q = InputQueue::default();
    q.on_message(&key(0x41, true, 1)); // KeyA with Shift held
    let ev = q.pop().unwrap();
    assert_eq!(
        ev,
        InputEvent::Key {
            vk: 0x41,
            down: true,
            modifiers: 1
        }
    );
    // On the wire to Sunshine: keyAction 3 (down), keyCode 0x8041, as Moonlight sends for `a`.
    let p = &encode_input(&ev).unwrap()[0];
    assert_eq!(u32::from_le_bytes(p[4..8].try_into().unwrap()), 3);
    assert_eq!(&p[9..11], &[0x41, 0x80]);
    q.on_message(&key(0x41, true, 1)); // a repeat: Sunshine repeats held keys itself
    q.on_message(&key(0x41, false, 0));
    assert_eq!(q.pop(), Some(k(0x41, false)));
    q.on_message(&key(0x42, false, 0)); // never pressed: nothing to release
    assert_eq!(q.pop(), None);
    assert_eq!(q.stats.received, 4);
    assert_eq!(q.stats.bad, 0);
}

#[test]
fn maps_relative_mouse_buttons_and_scroll() {
    let mut q = InputQueue::default();
    q.on_message(&mv(-5, 3));
    assert_eq!(q.pop(), Some(InputEvent::MouseMoveRel { dx: -5, dy: 3 }));
    q.on_message(&button(3, true));
    assert_eq!(
        q.pop(),
        Some(InputEvent::MouseButton {
            button: 3,
            down: true
        })
    );
    q.on_message(&[KIND_SCROLL, 0x88, 0xff]); // -120: one notch down
    assert_eq!(q.pop(), Some(InputEvent::Scroll { amount: -120 }));
}

#[test]
fn maps_relative_mouse() {
    let mut q = InputQueue::default();
    q.on_message(&mv(5, -5));
    q.on_message(&mv(2, 1)); // still queued: coalesced into one move
    assert_eq!(q.pop(), Some(InputEvent::MouseMoveRel { dx: 7, dy: -4 }));
    assert_eq!(q.pop(), None);
    assert_eq!(q.stats.coalesced, 1);
    // Not across a click: order is kept.
    q.on_message(&mv(1, 0));
    q.on_message(&button(1, true));
    q.on_message(&mv(1, 0));
    assert_eq!(drain(&mut q).len(), 3);
}

#[test]
fn unknown_kind_is_counted_not_fatal() {
    let mut q = InputQueue::default();
    for bad in [
        &[0x7f, 1, 2][..],           // unknown kind
        &[][..],                     // empty
        &[KIND_KEY, 0x41, 1][..],    // short
        &[KIND_MOVE, 1, 0, 1][..],   // short
        &key(0x00, true, 0)[..],     // vk 0
        &key(0xff, true, 0)[..],     // vk 0xff
        &key(0x41, true, 0x10)[..],  // modifier bit Sunshine does not know
        &[KIND_KEY, 0x41, 2, 0][..], // down neither 0 nor 1
        &button(6, true)[..],        // button out of 1..=5
        &button(0, true)[..],
    ] {
        q.on_message(bad);
    }
    assert_eq!(q.stats.bad, 10);
    assert_eq!(q.pop(), None, "nothing bad reaches Sunshine");
    q.on_message(&key(0x42, true, 0));
    assert_eq!(q.pop(), Some(k(0x42, true)), "still working after bad ones");
}

#[tokio::test]
async fn releases_all_keys_on_stream_close() {
    let sh = Shared::default();
    let mut wire = Vec::new();
    for m in [
        key(0xA0, true, 1),
        key(0x41, true, 1),
        key(0x41, false, 1),
        button(1, true),
        vec![0xEE], // unknown kind inside a frame: skipped, the stream goes on
        key(0x42, true, 1),
    ] {
        wire.extend_from_slice(&frame(&m));
    }
    wire.push(9); // a frame cut off by the close
    read_stream(&wire[..], &sh).await;
    let got = sh.with(|q| {
        assert_eq!(q.stats.bad, 1);
        drain(q)
    });
    assert_eq!(got.len(), 8, "{got:?}");
    // Keys in vk order, then buttons.
    assert_eq!(
        got[5..],
        vec![
            k(0x42, false),
            k(0xA0, false),
            InputEvent::MouseButton {
                button: 1,
                down: false
            },
        ]
    );
    assert_eq!(sh.with(|q| q.stats.released), 3);
    // Nothing is down any more: a second release-all adds nothing.
    assert!(sh.with(|q| {
        q.release_all();
        q.pop().is_none()
    }));
}

#[test]
fn overflow_drops_moves_and_downs_but_keeps_key_ups() {
    let mut q = InputQueue::default();
    q.on_message(&key(0x41, true, 0));
    q.on_message(&button(1, true));
    for i in 0..(MAX_PENDING * 2) {
        // Alternate kinds so nothing coalesces.
        q.on_message(&mv(1, 0));
        q.on_message(&[KIND_SCROLL, 120, 0]);
        q.on_message(&key(0x50 + (i % 40) as u8, true, 0)); // more keys than fit
    }
    q.on_message(&key(0x41, false, 0));
    q.on_message(&button(1, false));
    let got = drain(&mut q);
    assert!(got.len() <= MAX_PENDING + 2, "{} queued", got.len());
    assert!(q.stats.dropped > 0);
    assert_eq!(
        &got[got.len() - 2..],
        &[
            k(0x41, false),
            InputEvent::MouseButton {
                button: 1,
                down: false
            }
        ]
    );
    // A down that was dropped is not down: its key-up is redundant and nothing stays stuck.
    q.release_all();
    let ups = drain(&mut q);
    assert!(
        ups.iter()
            .all(|e| matches!(e, InputEvent::Key { down: false, .. })),
        "{ups:?}"
    );
    // Every key whose last queued event was a press gets its release.
    let mut held = std::collections::BTreeMap::new();
    for e in &got {
        if let InputEvent::Key { vk, down, .. } = e {
            held.insert(*vk, *down);
        }
    }
    let held: Vec<_> = held
        .into_iter()
        .filter(|h| h.1)
        .map(|h| k(h.0, false))
        .collect();
    assert!(held.len() > 20, "{} keys pressed", held.len());
    assert_eq!(ups, held, "every press released once, no extra releases");
}

#[tokio::test]
async fn timeout_is_not_fatal_and_keeps_releases() {
    let sh = Shared::default();
    sh.with(|q| {
        q.on_message(&mv(5, 0));
        q.on_message(&key(0x41, true, 0));
        q.on_message(&key(0x41, false, 0));
    });
    let mut calls = Vec::new();
    let r = send_loop(&sh, |ev| {
        calls.push(ev.clone());
        let n = calls.len();
        async move {
            match n {
                // move and key-down time out: dropped; the key-up times out once: retried.
                1..=3 => Err(Error::Timeout("not drained".into())),
                4 => Ok(()),
                _ => Err(Error::Closed("control session")),
            }
        }
    });
    let r = timeout(SECS2, async {
        sh.with(|q| q.on_message(&key(0x42, true, 0))); // sent after the retry: ends the loop
        r.await
    })
    .await
    .unwrap();
    assert!(matches!(r, Err(Error::Closed(_))), "{r:?}");
    assert_eq!(
        calls,
        vec![
            InputEvent::MouseMoveRel { dx: 5, dy: 0 },
            k(0x41, true),
            k(0x41, false),
            k(0x41, false),
            k(0x42, true),
        ]
    );
    let st = sh.with(|q| q.stats);
    assert_eq!((st.timeouts, st.sent), (3, 1));
}

#[test]
fn shutdown_returns_pending_and_held_releases() {
    let mut q = InputQueue::default();
    q.on_message(&key(0x41, true, 0));
    q.on_message(&key(0xA0, true, 0));
    q.on_message(&mv(3, 3));
    assert_eq!(q.pop(), Some(k(0x41, true))); // sent; 0xA0 down and the move still queued
    assert_eq!(q.shutdown(), vec![k(0x41, false), k(0xA0, false)]);
    assert_eq!(q.pop(), None);
}

#[test]
fn shutdown_also_releases_keys_whose_release_was_already_handed_on() {
    // A release popped by the send loop may still sit unsent in ControlHandle's channel when the
    // bridge stops; shutdown re-sends a release for every key and button ever pressed.
    let mut q = InputQueue::default();
    q.on_message(&key(0x42, true, 0));
    q.on_message(&button(2, true));
    q.on_message(&key(0x42, false, 0));
    q.on_message(&button(2, false));
    assert_eq!(drain(&mut q).len(), 4);
    assert_eq!(
        q.shutdown(),
        vec![
            k(0x42, false),
            InputEvent::MouseButton {
                button: 2,
                down: false
            }
        ]
    );
}

#[tokio::test]
async fn a_non_timeout_error_drops_a_release_once_without_spinning() {
    let sh = Shared::default();
    sh.with(|q| {
        q.on_message(&key(0x41, true, 0));
        q.on_message(&key(0x41, false, 0));
    });
    let mut calls = Vec::new();
    let r = send_loop(&sh, |e| {
        calls.push(e);
        let n = calls.len();
        async move {
            match n {
                1 => Ok(()),
                2 => Err(Error::Setup("bad input".into())), // not a Timeout: no retry
                _ => Err(Error::Closed("control session")),
            }
        }
    });
    sh.with(|q| q.on_message(&key(0x43, true, 0))); // ends the loop on the 3rd call
    let r = timeout(SECS2, r).await.expect("no spin");
    assert!(matches!(r, Err(Error::Closed(_))));
    assert_eq!(
        calls,
        vec![k(0x41, true), k(0x41, false), k(0x43, true)],
        "the release was tried once"
    );
    let st = sh.with(|q| q.stats);
    assert_eq!((st.timeouts, st.errors, st.sent), (0, 1, 1));
}

/// A read_stream over an in-memory pipe, on tokio's paused clock.
fn piped() -> (
    Arc<Shared>,
    tokio::io::DuplexStream,
    tokio::task::JoinHandle<()>,
) {
    let sh = Arc::new(Shared::default());
    let (tx, rx) = tokio::io::duplex(4096);
    let t = tokio::spawn({
        let sh = sh.clone();
        async move { read_stream(rx, &sh).await }
    });
    (sh, tx, t)
}
const BEAT: Duration = Duration::from_millis(250);

#[tokio::test(start_paused = true)]
async fn held_key_with_heartbeats_stays_down() {
    let (sh, mut tx, _t) = piped();
    tx.write_all(&frame(&key(0x41, true, 0))).await.unwrap();
    for _ in 0..20 {
        sleep(BEAT).await; // 5 s in all
        tx.write_all(&frame(&[KIND_HEARTBEAT])).await.unwrap();
    }
    sleep(BEAT / 2).await;
    sh.with(|q| {
        assert_eq!(drain(q), vec![k(0x41, true)]);
        assert_eq!(
            (q.stats.released, q.stats.held_timeouts, q.stats.bad),
            (0, 0, 0)
        );
        assert_eq!(q.stats.received, 1, "heartbeats are not input");
        assert_eq!(q.stats.heartbeats, 20);
    });
}

#[tokio::test(start_paused = true)]
async fn held_key_and_silence_releases_once() {
    let (sh, mut tx, _t) = piped();
    tx.write_all(&frame(&key(0x41, true, 0))).await.unwrap();
    tx.write_all(&frame(&button(1, true))).await.unwrap();
    sleep(Duration::from_millis(750)).await; // the gate asks for about 1 s
    assert_eq!(
        sh.with(|q| q.stats.held_timeouts),
        0,
        "not before the timeout"
    );
    sleep(BEAT * 2).await;
    assert_eq!(sh.with(|q| q.stats.held_timeouts), 1, "released by 1.25 s");
    sleep(HOLD_TIMEOUT * 5).await; // and never again
    sh.with(|q| {
        assert_eq!(
            drain(q),
            vec![
                k(0x41, true),
                InputEvent::MouseButton {
                    button: 1,
                    down: true
                },
                k(0x41, false),
                InputEvent::MouseButton {
                    button: 1,
                    down: false
                },
            ]
        );
        assert_eq!((q.stats.held_timeouts, q.stats.released), (1, 2));
    });
    // Heartbeats after the release do not bring the keys back.
    for _ in 0..8 {
        tx.write_all(&frame(&[KIND_HEARTBEAT])).await.unwrap();
        sleep(BEAT).await;
    }
    sh.with(|q| {
        assert_eq!(q.pop(), None);
        q.release_all();
        assert_eq!(q.pop(), None, "nothing is held");
    });
}

#[tokio::test(start_paused = true)]
async fn nothing_held_and_silence_does_nothing() {
    let (sh, mut tx, t) = piped();
    tx.write_all(&frame(&key(0x41, true, 0))).await.unwrap();
    tx.write_all(&frame(&key(0x41, false, 0))).await.unwrap();
    tx.write_all(&frame(&mv(5, 0))).await.unwrap();
    let start = tokio::time::Instant::now();
    sleep(HOLD_TIMEOUT * 10).await;
    assert_eq!(start.elapsed(), HOLD_TIMEOUT * 10);
    assert_eq!(
        sh.with(|q| (q.stats.held_timeouts, q.stats.released)),
        (0, 0)
    );
    assert_eq!(sh.with(|q| q.len()), 3);
    // held_timeouts counts every firing of the reader's timer, which is only armed while held.
    assert!(!t.is_finished());
}

#[tokio::test]
async fn browser_bidi_stream_feeds_the_queue_and_releases_on_close() {
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let sh = Arc::new(Shared::default());
    tokio::spawn(serve(Arc::new(s), sh.clone()));
    let (mut tx, _rx) = c.open_bi().await.unwrap();
    tx.write_all(&[0x40, 0x41, 0x00]).await.unwrap(); // WEBTRANSPORT_STREAM, session id 0
    tx.write_all(&frame(&key(0x41, true, 0))).await.unwrap();
    let wait = |n: usize| {
        let sh = sh.clone();
        async move {
            while sh.with(|q| q.len()) < n {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    };
    timeout(SECS2, wait(1)).await.unwrap();
    c.close(0u32.into(), b"tab closed");
    timeout(SECS2, wait(2)).await.unwrap();
    let got = sh.with(drain);
    assert_eq!(got, vec![k(0x41, true), k(0x41, false)]);
}

/// A press dropped on a send timeout must not stay marked as down: the page's repeat of it
/// (key auto-repeat, held W) goes through, and the release afterwards is sent once.
#[test]
fn press_dropped_on_timeout_is_forwarded_on_the_next_repeat() {
    let mut q = InputQueue::default();
    q.on_message(&key(0x57, true, 0));
    let e = q.pop().unwrap();
    q.timed_out(e);
    assert!(q.is_empty(), "a press is not retried");
    q.on_message(&key(0x57, true, 0));
    assert_eq!(drain(&mut q), vec![k(0x57, true)]);
    q.on_message(&key(0x57, false, 0));
    assert_eq!(drain(&mut q), vec![k(0x57, false)]);
    assert!(!q.held());

    q.on_message(&button(1, true));
    let e = q.pop().unwrap();
    q.timed_out(e);
    q.on_message(&button(1, true));
    let down = InputEvent::MouseButton {
        button: 1,
        down: true,
    };
    assert_eq!(drain(&mut q), vec![down]);
    q.on_message(&button(1, false));
    assert_eq!(drain(&mut q).len(), 1);
    assert!(!q.held());
}

/// A dropped press whose key was released and pressed again meanwhile: the newer press is still
/// queued, so the key stays held (its release must not be ignored, or the key would stick).
#[test]
fn press_dropped_on_timeout_keeps_a_newer_queued_press() {
    let mut q = InputQueue::default();
    q.on_message(&key(0x57, true, 0));
    let e = q.pop().unwrap();
    q.on_message(&key(0x57, false, 0));
    q.on_message(&key(0x57, true, 0));
    q.timed_out(e);
    q.on_message(&key(0x57, false, 0));
    assert_eq!(
        drain(&mut q),
        vec![k(0x57, false), k(0x57, true), k(0x57, false)]
    );
    assert!(!q.held());
}
