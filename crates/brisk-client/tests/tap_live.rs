//! Tap listener against a local fake Sunshine tap (a real AF_UNIX SOCK_SEQPACKET peer in this
//! process, writing messages as the patch's `brisk_tap_send` does).

use brisk_client::tap::{TapListener, monotonic_ns};
use brisk_proto::sdp::Codec;
use socket2::{Domain, SockAddr, Socket, Type};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

/// 44-byte little-endian header (research/m2-tap-design.md section 2) + payload.
fn msg(
    flags: u8,
    connect_data: u32,
    idx: u64,
    capture: u64,
    dropped: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut m = b"BTAP".to_vec();
    m.extend([1, flags]);
    m.extend(44u16.to_le_bytes());
    m.extend(connect_data.to_le_bytes());
    m.extend(0u32.to_le_bytes()); // H.264
    m.extend(idx.to_le_bytes());
    m.extend(capture.to_le_bytes());
    m.extend(monotonic_ns().to_le_bytes());
    m.extend(dropped.to_le_bytes());
    m.extend(payload);
    m
}

/// What Sunshine does: connect, then one `send` per frame.
fn peer(path: &Path) -> Socket {
    let s = Socket::new(Domain::UNIX, Type::SEQPACKET, None).unwrap();
    s.connect(&SockAddr::unix(path).unwrap()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    s
}

/// The listener closed this peer: a read sees EOF (0), or the connection was reset.
fn closed(s: &Socket) -> bool {
    let mut b = [std::mem::MaybeUninit::uninit(); 8];
    matches!(s.recv(&mut b), Ok(0) | Err(_)) && s.send(b"x").is_err()
}

const CD: u32 = 0x5348_4c5c;
const ANNEXB: &[u8] = &[0, 0, 0, 1, 0x67, 0x42, 0, 0x28, 0, 0, 0, 1, 0x65, 0x88];

#[tokio::test]
async fn listener_delivers_frames_of_the_matching_session() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("brisk/tap.sock");
    let l = TapListener::bind(&path).unwrap();
    let mode = std::fs::metadata(t.path().join("brisk"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    let p = peer(&path);
    p.send(&msg(0b1001, CD, 41, 1000, 2, ANNEXB)).unwrap();
    p.send(&msg(0, CD, 42, 2000, 2, &ANNEXB[11..])).unwrap();
    let mut c = l.accept(CD, Codec::H264).await.unwrap();
    let f = c.recv().await.unwrap();
    assert_eq!((f.idx, f.idr, f.capture_ns, f.dropped), (41, true, 1000, 2));
    assert_eq!(&f.annexb[..], ANNEXB);
    assert!(f.tap_ns > 0 && f.tap_ns <= monotonic_ns());
    let f = c.recv().await.unwrap();
    assert_eq!((f.idx, f.idr, &f.annexb[..]), (42, false, &ANNEXB[11..]));
    drop(l);
    assert!(!path.exists(), "socket file removed with the listener");
}

/// A connection of another session (stale connect data) is closed; the right one is returned.
#[tokio::test]
async fn listener_skips_other_sessions() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    let l = TapListener::bind(&path).unwrap();
    let stale = peer(&path);
    stale.send(&msg(1, CD + 1, 1, 1, 0, ANNEXB)).unwrap();
    let ours = peer(&path);
    ours.send(&msg(1, CD, 7, 1, 0, ANNEXB)).unwrap();
    let mut c = l.accept(CD, Codec::H264).await.unwrap();
    assert_eq!(c.recv().await.unwrap().idx, 7);
    assert!(closed(&stale));
}

#[tokio::test]
async fn listener_rejects_foreign_uid() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    // Pretend we run as another user, so this process's own peer is the foreign one.
    let uid = rustix::process::getuid().as_raw();
    let l = TapListener::bind(&path).unwrap().expect_uid(uid + 1);
    let p = peer(&path);
    p.send(&msg(1, CD, 1, 1, 0, ANNEXB)).unwrap();
    let r = tokio::time::timeout(Duration::from_millis(300), l.accept(CD, Codec::H264)).await;
    assert!(r.is_err(), "a foreign peer must not be accepted");
    assert!(closed(&p));
}

#[tokio::test]
async fn tap_conn_reports_eof_as_closed() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    let l = TapListener::bind(&path).unwrap();
    let p = peer(&path);
    p.send(&msg(1, CD, 1, 1, 0, ANNEXB)).unwrap();
    let mut c = l.accept(CD, Codec::H264).await.unwrap();
    c.recv().await.unwrap();
    drop(p);
    let r = tokio::time::timeout(Duration::from_secs(2), c.recv())
        .await
        .unwrap();
    assert!(matches!(r, Err(brisk_client::Error::Closed(_))), "{r:?}");
}

/// A message that is not a tap frame is an error, not a panic or a silent skip.
#[tokio::test]
async fn tap_conn_rejects_garbage() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    let l = TapListener::bind(&path).unwrap();
    let p = peer(&path);
    p.send(&msg(1, CD, 1, 1, 0, ANNEXB)).unwrap();
    p.send(b"BTAQ not a tap message at all, but long enough to have a header")
        .unwrap();
    let mut c = l.accept(CD, Codec::H264).await.unwrap();
    c.recv().await.unwrap();
    let r = c.recv().await;
    assert!(matches!(r, Err(brisk_client::Error::Tap(_))), "{r:?}");
}

#[test]
fn tap_path_prefers_brisk_tap_sock() {
    use brisk_client::tap::tap_path_from;
    let xdg = |n: &str| (n == "XDG_RUNTIME_DIR").then(|| "/run/user/4242".into());
    assert_eq!(
        tap_path_from(xdg).unwrap(),
        Path::new("/run/user/4242/brisk/tap.sock")
    );
    let both = |n: &str| match n {
        "BRISK_TAP_SOCK" => Some("/tmp/x.sock".into()),
        _ => xdg(n),
    };
    assert_eq!(tap_path_from(both).unwrap(), Path::new("/tmp/x.sock"));
    assert_eq!(tap_path_from(|_| None), None);
}

/// `recv - tap_ns` on loopback: 3000 frames of 40 KB (every 60th 300 KB, an IDR), 1 ms apart.
#[tokio::test(flavor = "current_thread")]
#[ignore = "benchmark: cargo test --release -p brisk-client --test tap_live -- --ignored --nocapture"]
async fn bench_recv_latency() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    let l = TapListener::bind(&path).unwrap();
    let p = peer(&path);
    p.set_send_buffer_size(4 << 20).unwrap();
    let n = 3000;
    let sender = std::thread::spawn(move || {
        for i in 0..n {
            let size = if i % 60 == 0 { 300_000 } else { 40_000 };
            let mut m = msg((i % 60 == 0) as u8, CD, i, 0, 0, &vec![0x55; size]);
            m[32..40].copy_from_slice(&monotonic_ns().to_le_bytes());
            p.send(&m).unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        p
    });
    let mut c = l.accept(CD, Codec::H264).await.unwrap();
    let mut us = Vec::new();
    for _ in 0..n {
        let f = c.recv().await.unwrap();
        us.push((monotonic_ns() - f.tap_ns) as f64 / 1000.0);
    }
    drop(sender.join().unwrap());
    let q = brisk_client::stream::percentiles(&us, &[50, 99]).unwrap();
    println!(
        "recv - tap_ns over {n} frames: p50 {:.1} us p99 {:.1} us",
        q[0], q[1]
    );
}

fn frame(
    idx: u64,
    idr: bool,
    capture_ns: u64,
    dropped: u32,
    annexb: &'static [u8],
) -> brisk_client::tap::TapFrame {
    brisk_client::tap::TapFrame {
        idx,
        idr,
        capture_ns,
        tap_ns: capture_ns + 1_000_000,
        dropped,
        annexb: bytes::Bytes::from_static(annexb),
    }
}

const SPS_PPS_IDR: &[u8] = &[
    0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x28, 0xda, 0, 0, 0, 1, 0x68, 0xce, 0, 0, 1, 0x65, 0x88,
];

/// 600 frames at 60 fps (10 s) with 5 dropped by the tap (idx 100-104, counted in
/// dropped_total) and a recovery IDR: the gate passes. A gap the tap did not count, or any RTP
/// video datagram, fails it.
#[test]
fn tap_rx_gate_counts_drops_idr_and_latency() {
    use brisk_client::control::Stats;
    use brisk_client::tap::TapRx;
    let ns = |i: u64| 1_000_000_000 + i * 16_666_667;
    let mut rx = TapRx::new();
    for i in (0..600).filter(|i| !(100..105).contains(i)) {
        if i == 350 {
            rx.idr_requested(ns(i));
        }
        let idr = matches!(i, 0 | 105 | 400);
        let f = frame(
            i,
            idr,
            ns(i),
            if i >= 105 { 5 } else { 0 },
            if idr {
                SPS_PPS_IDR
            } else {
                &[0, 0, 1, 0x41, 0x9a]
            },
        );
        rx.on_frame(&f, ns(i) + 3_000_000);
    }
    let s = rx.summary(10.0, &Stats::default());
    assert!(s.passed(), "{s}");
    assert_eq!(
        (s.frames, s.idr_frames, s.idx_gaps, s.dropped),
        (595, 3, 5, 5)
    );
    assert_eq!(s.recv_capture_ms.as_deref(), Some(&[3.0, 3.0][..]));
    assert_eq!(s.host_ms.as_deref(), Some(&[1.0, 1.0][..]));
    let ms = s.idr_response_ms[0].unwrap();
    assert!((ms - 836.3).abs() < 0.1, "{ms}"); // 50 frames + the 3 ms receive delay
    assert_eq!(
        s.first_sps.as_deref(),
        Some(&[0x67, 0x42, 0xc0, 0x28, 0xda][..])
    );
    let row = |s: &brisk_client::tap::TapSummary, k: &str| {
        s.to_string()
            .lines()
            .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
    };
    assert_eq!(row(&s, "rtp_video_datagrams").as_deref(), Some("0"), "{s}");
    assert_eq!(row(&s, "first_sps").as_deref(), Some("6742c028da"), "{s}");

    let mut st = Stats::default();
    st.media_rx_packets[0] = 1;
    let s = rx.summary(10.0, &st);
    assert!(
        s.failures.iter().any(|f| f.contains("rtp_video_datagrams")),
        "{s}"
    );

    let mut rx = TapRx::new();
    for i in (0..600).filter(|&i| i != 50) {
        rx.on_frame(&frame(i, i == 0, ns(i), 0, SPS_PPS_IDR), ns(i));
    }
    let s = rx.summary(10.0, &Stats::default());
    assert!(s.failures.iter().any(|f| f.contains("idx")), "{s}");
}

/// A header longer than 256 bytes (header_len 300, future fields) is accepted and skipped.
#[tokio::test]
async fn listener_accepts_header_len_300() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    let l = TapListener::bind(&path).unwrap();
    let mut m = msg(1, CD, 9, 1, 0, ANNEXB);
    m[6..8].copy_from_slice(&300u16.to_le_bytes());
    m.splice(44..44, std::iter::repeat_n(0xEE, 256));
    let p = peer(&path);
    p.send(&m).unwrap();
    let r = tokio::time::timeout(Duration::from_secs(2), l.accept(CD, Codec::H264)).await;
    let mut c = r.expect("accept hung").expect("header_len 300 refused");
    let f = c.recv().await.unwrap();
    assert_eq!((f.idx, &f.annexb[..]), (9, ANNEXB));
}

/// A same-uid peer whose first message is not a tap frame is closed like a stale session; the
/// accept goes on to the next connection.
#[tokio::test]
async fn listener_skips_garbage_first_message() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    let l = TapListener::bind(&path).unwrap();
    let junk = peer(&path);
    junk.send(b"not a tap message, but long enough to look like a header")
        .unwrap();
    let ours = peer(&path);
    ours.send(&msg(1, CD, 5, 1, 0, ANNEXB)).unwrap();
    let r = tokio::time::timeout(Duration::from_secs(2), l.accept(CD, Codec::H264)).await;
    let mut c = r.expect("accept hung").expect("garbage aborted the accept");
    assert_eq!(c.recv().await.unwrap().idx, 5);
    assert!(closed(&junk));
}

/// HEVC (VPS 0x40, SPS 0x42, PPS 0x44, IDR_W_RADL 0x26) with the tap header's codec field = 1.
const HEVC: &[u8] = &[
    0, 0, 0, 1, 0x40, 1, 0, 0, 0, 1, 0x42, 1, 0, 0, 0, 1, 0x44, 1, 0, 0, 0, 1, 0x26, 1,
];

fn with_codec(mut m: Vec<u8>, codec: u32) -> Vec<u8> {
    m[12..16].copy_from_slice(&codec.to_le_bytes());
    m
}

#[tokio::test]
async fn listener_accepts_hevc_when_the_session_asked_for_it() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    let l = TapListener::bind(&path).unwrap();
    let p = peer(&path);
    p.send(&with_codec(msg(1, CD, 3, 1, 0, HEVC), 1)).unwrap();
    let r = tokio::time::timeout(Duration::from_secs(2), l.accept(CD, Codec::Hevc)).await;
    let mut c = r.expect("accept hung").expect("HEVC refused");
    let f = c.recv().await.unwrap();
    assert_eq!((f.idx, f.idr, &f.annexb[..]), (3, true, HEVC));
}

/// The tap's codec must be the one the session announced; anything else is a clear error.
#[tokio::test]
async fn listener_rejects_a_codec_mismatch() {
    for (sent, want, msg_has) in [
        (
            0u32,
            Codec::Hevc,
            "codec 0 (H.264), but the session asked for HEVC",
        ),
        (
            1,
            Codec::H264,
            "codec 1 (HEVC), but the session asked for H.264",
        ),
        (2, Codec::H264, "codec 2 (AV1)"),
    ] {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("tap.sock");
        let l = TapListener::bind(&path).unwrap();
        let p = peer(&path);
        p.send(&with_codec(msg(1, CD, 3, 1, 0, ANNEXB), sent))
            .unwrap();
        let r = tokio::time::timeout(Duration::from_secs(2), l.accept(CD, want)).await;
        let e = r.expect("accept hung").err().expect("mismatch accepted");
        assert!(e.to_string().contains(msg_has), "{e}");
    }
}
