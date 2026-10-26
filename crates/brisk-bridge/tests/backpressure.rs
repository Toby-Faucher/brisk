//! The bridge's browser-leg drop policy ([`Pacer`]), sans-io: frames go out one uni stream each;
//! more than [`MAX_BEHIND`] unacked streams means the browser is behind, so the old streams are
//! reset and frames are dropped until the next IDR, which is requested at most every 100 ms.

use brisk_bridge::session::{Pacer, IDR_EVERY_NS, IDR_MAX_GAP_NS, MAX_BEHIND, REFRESH_EVERY_NS};

const MS: u64 = 1_000_000;

#[test]
fn drops_until_idr_when_more_than_3_frames_behind() {
    let mut p = Pacer::new();
    assert!(p.on_frame(0, true, 0).send);
    for i in 1..=3 {
        assert!(p.on_frame(i, false, i * 16 * MS).send, "frame {i}");
    }
    // 4 unacked: frame 4 finds the browser more than 3 behind.
    let d = p.on_frame(4, false, 64 * MS);
    assert!(!d.send);
    assert!(d.request_idr);
    for i in 5..=6 {
        assert!(
            !p.on_frame(i, false, i * 16 * MS).send,
            "frame {i} is not an IDR"
        );
    }
    assert!(p.on_frame(7, true, 112 * MS).send, "the IDR goes out");
    assert!(
        p.on_frame(8, false, 128 * MS).send,
        "and so does what follows it"
    );
}

#[test]
fn exactly_3_behind_is_not_overload() {
    let mut p = Pacer::new();
    for i in 0..=3 {
        assert!(p.on_frame(i, i == 0, i * 16 * MS).send);
    }
    p.on_done(0, true);
    let d = p.on_frame(4, false, 64 * MS);
    assert!(d.send && d.reset.is_empty() && !d.request_idr);
}

#[test]
fn resets_stale_streams() {
    let mut p = Pacer::new();
    for i in 0..=3 {
        p.on_frame(i, i == 0, i * 16 * MS);
    }
    p.on_done(1, true);
    p.on_done(2, true);
    for i in 4..=5 {
        assert!(p.on_frame(i, false, i * 16 * MS).send);
    }
    // In flight: 0, 3, 4, 5. Frame 6 resets exactly those, oldest first.
    let d = p.on_frame(6, false, 96 * MS);
    assert_eq!(d.reset, vec![0, 3, 4, 5]);
    assert_eq!(p.in_flight(), 0);
    // A late completion of a reset stream changes nothing.
    p.on_done(3, false);
    assert_eq!(p.in_flight(), 0);
    let d = p.on_frame(7, true, 112 * MS);
    assert!(d.send && d.reset.is_empty());
    assert_eq!(p.in_flight(), 1);
    assert_eq!(p.stats.reset, 4);
}

#[test]
fn idr_requested_at_most_every_100ms() {
    let mut p = Pacer::new();
    // No key frame yet: the first frame asks for one.
    let d = p.on_frame(0, false, 0);
    assert!(!d.send && d.request_idr);
    for (i, t) in [16, 50, 99].into_iter().enumerate() {
        assert!(
            !p.on_frame(i as u64 + 1, false, t * MS).request_idr,
            "{t} ms"
        );
    }
    assert!(
        p.on_frame(4, false, IDR_EVERY_NS).request_idr,
        "100 ms later"
    );
    assert!(p.on_frame(5, true, 110 * MS).send);
    p.on_done(5, true); // the browser is keeping up again
                        // The browser asks too: same limiter.
    assert!(!p.want_key(150 * MS));
    assert!(
        !p.on_frame(6, false, 160 * MS).send,
        "dropping after want_key"
    );
    assert!(p.want_key(200 * MS));
    assert_eq!(p.stats.idr_requests, 3);
}

#[test]
fn queue_is_bounded() {
    let mut p = Pacer::new();
    let (mut sent, mut reset) = (0u64, 0u64);
    // 10k frames, an IDR every 60, and a browser that never acknowledges anything.
    for i in 0..10_000u64 {
        let d = p.on_frame(i, i % 60 == 0, i * 16 * MS);
        sent += d.send as u64;
        reset += d.reset.len() as u64;
        assert!(
            p.in_flight() <= MAX_BEHIND + 1,
            "frame {i}: {}",
            p.in_flight()
        );
        assert!(d.reset.len() <= MAX_BEHIND + 1);
    }
    assert_eq!(
        sent,
        reset + p.in_flight() as u64,
        "every sent stream is acked, reset or in flight"
    );
    assert!(sent > 0 && p.stats.dropped > 9_000);
}

#[test]
fn idr_requests_back_off_while_the_browser_acks_nothing() {
    // A tab that died without closing: every IDR is sent into a connection that never acks it.
    let mut p = Pacer::new();
    let mut at = vec![];
    for i in 0..600u64 {
        let t = i * 16 * MS; // ~9.6 s
        if p.on_frame(i, i % 7 == 3, t).request_idr {
            at.push(t / MS);
        }
    }
    let gaps: Vec<u64> = at.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(
        gaps.windows(2).all(|g| g[1] >= g[0]),
        "gaps only grow: {gaps:?}"
    );
    assert!(gaps[0] >= IDR_EVERY_NS / MS && *gaps.last().unwrap() >= IDR_MAX_GAP_NS / MS);
    // Requests go out on dropped frames, so a gap can overrun by the few frames after an IDR.
    assert!(
        gaps.iter().all(|&g| g <= IDR_MAX_GAP_NS / MS + 100),
        "{gaps:?}"
    );
    // One acknowledged frame and the next request may come 100 ms after the last.
    let mut p = Pacer::new();
    assert!(p.on_frame(0, false, 0).request_idr);
    assert!(p.on_frame(1, false, 100 * MS).request_idr);
    assert!(
        !p.on_frame(2, false, 200 * MS).request_idr,
        "backed off to 200 ms"
    );
    assert!(p.on_frame(3, true, 210 * MS).send);
    assert!(p.on_frame(4, false, 226 * MS).send);
    p.on_done(3, false); // reset or failed: not an ack
    assert!(!p.want_key(240 * MS), "still backed off");
    p.on_done(4, true);
    assert!(p.want_key(250 * MS), "an ack resets the gap to 100 ms");
}

#[test]
fn refresh_key_requests_an_idr_without_dropping_anything() {
    let mut p = Pacer::new();
    assert!(p.on_frame(0, true, 0).send);
    p.on_done(0, true);
    assert!(p.refresh_key(10 * MS), "a refresh asks for an IDR");
    // The decoder is fine: deltas keep flowing while the IDR is on its way.
    for i in 1..=3 {
        let d = p.on_frame(i, false, (10 + i * 16) * MS);
        assert!(d.send && !d.request_idr, "frame {i}");
        p.on_done(i, true);
    }
    assert_eq!(p.stats.dropped, 0);
    assert_eq!(p.stats.refreshes, 1);
    assert_eq!(p.stats.idr_requests, 0, "the want-key counter is untouched");
    // Rate limit: at most one per REFRESH_EVERY_NS, independent of the want-key limiter.
    assert!(!p.refresh_key(10 * MS + REFRESH_EVERY_NS - 1));
    assert!(p.refresh_key(10 * MS + REFRESH_EVERY_NS));
    assert!(
        p.want_key(10 * MS + REFRESH_EVERY_NS),
        "want-key has its own limiter"
    );
    assert!(
        !p.on_frame(4, false, 2_000 * MS).send,
        "want-key still drops until the IDR"
    );
    assert!(REFRESH_EVERY_NS >= 1_000 * MS);
}
