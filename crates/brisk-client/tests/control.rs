//! Control session against a local fake Sunshine control server (127.0.0.1, ephemeral port);
//! the fake and what it mirrors are described in common/fake_host.rs.

mod common;

use brisk_client::control::{ControlSession, IDR_MIN_INTERVAL, INPUT_DEMO_GAP};
use brisk_proto::input::{encode_input, InputEvent};
use common::fake_host::*;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

async fn settle(c: &mut ControlSession, ms: u64) {
    c.run_until(Instant::now() + Duration::from_millis(ms))
        .await
        .unwrap();
}

#[tokio::test]
async fn control_session_against_fake_sunshine() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 0xC0DE, av(&f).await)
        .await
        .unwrap();
    settle(&mut c, 450).await;
    {
        let l = f.log.lock().unwrap();
        assert_eq!(l.connects, vec![CONNECT_DATA]);
        assert_eq!((l.unencrypted, l.bad_tag), (0, 0));
    }
    let t = types(&f);
    // startControlStream order: index-0 (IDR on Gen7Enc) then Start B, both on GENERIC.
    assert_eq!(t[..2], [(0, 0x0302), (0, 0x0307)]);
    let pings = t.iter().filter(|m| **m == (0, 0x0200)).count();
    assert!(pings >= 3, "{t:?}");
    assert!(c.stats.pings_acked >= 2, "{:?}", c.stats);

    let key = InputEvent::Key {
        vk: 0x41,
        down: true,
        modifiers: 0,
    };
    c.send_input(&key).await.unwrap();
    c.request_idr().await.unwrap();
    f.tx.send(Cmd::Send(0x010b, {
        let mut p = 0xC0FFEEu32.to_le_bytes().to_vec();
        p.extend_from_slice(&[1, 0, 0x34, 0x12, 0xFF, 0xFF]);
        p
    }))
    .unwrap();
    settle(&mut c, 200).await;
    assert_eq!(c.stats.last_rumble, Some((1, 0x1234, 0xFFFF)));
    assert_eq!(c.stats.host_msgs.get(&0x010b), Some(&1));
    {
        let l = f.log.lock().unwrap();
        let input: Vec<_> = l.msgs.iter().filter(|m| m.1 == 0x0206).collect();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0].0, 2, "keyboard channel");
        assert_eq!(input[0].2, encode_input(&key).unwrap()[0]);
        // On-demand requestIdrFrame uses CTRL_CHANNEL_URGENT (ControlStream.c:1524-1527).
        let idr: Vec<_> = l
            .msgs
            .iter()
            .filter(|m| m.1 == 0x0302)
            .map(|m| m.0)
            .collect();
        assert_eq!(idr, [0, 1]);
    }
    let stats = c.disconnect().await.unwrap();
    assert!(!stats.host_disconnected);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        f.log.lock().unwrap().disconnects,
        1,
        "graceful DISCONNECT acked"
    );
}

/// Sunshine tears the session down when no video/audio ping arrives within ping_timeout.
#[tokio::test]
async fn control_survives_ping_timeout() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 3, av(&f).await)
        .await
        .unwrap();
    c.run_until(Instant::now() + FAKE_PING_TIMEOUT + Duration::from_secs(1))
        .await
        .unwrap();
    let l = f.log.lock().unwrap();
    assert!(!l.stopped, "fake stopped the session: no A/V ping");
    assert!(c.is_open() && !c.stats.host_disconnected, "{:?}", c.stats);
    // 2.5 s at 500 ms: about 5 pings per socket; SS_PING = payload ASCII + seq BE from 1.
    assert!(l.av_pings.iter().all(|&n| n >= 4), "{:?}", l.av_pings);
    let mut want = PING_PAYLOAD.as_bytes().to_vec();
    want.extend_from_slice(&1u32.to_be_bytes());
    assert_eq!(l.first_ping, [Some(want.clone()), Some(want)]);
    // The fake answered 3 video and 2 audio datagrams of 100 bytes to the ping source.
    assert_eq!(c.stats.media_rx_packets, [3, 2]);
    assert_eq!(c.stats.media_rx_bytes, [300, 200]);
    assert!(c.stats.av_pings_sent.iter().all(|&n| n >= 4));
}

/// Without A/V pings the fake ends the session at its ping timeout, as Sunshine would at 10 s.
#[tokio::test]
async fn control_without_av_pings_is_torn_down() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 4, None)
        .await
        .unwrap();
    c.run_until(Instant::now() + FAKE_PING_TIMEOUT + Duration::from_secs(1))
        .await
        .unwrap();
    assert!(f.log.lock().unwrap().stopped);
    assert!(!c.is_open() && c.stats.host_disconnected);
}

#[tokio::test]
async fn control_retransmits_connect() {
    let f = fake(2); // first CONNECT and its first retransmission are lost
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 7, av(&f).await)
        .await
        .unwrap();
    settle(&mut c, 300).await;
    assert_eq!(f.log.lock().unwrap().connects, vec![CONNECT_DATA]);
    assert!(types(&f).contains(&(0, 0x0307)));
    c.disconnect().await.unwrap();
}

#[tokio::test]
async fn control_wrong_connect_data_is_rejected() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA + 1, KEY, 9, av(&f).await)
        .await
        .unwrap();
    settle(&mut c, 500).await;
    assert!(!c.is_open());
    assert!(c.stats.host_disconnected);
    assert_eq!(f.log.lock().unwrap().rejected, 1);
    c.disconnect().await.unwrap();
}

#[tokio::test]
async fn control_host_termination() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 11, av(&f).await)
        .await
        .unwrap();
    settle(&mut c, 150).await;
    f.tx.send(Cmd::SendRaw(vec![0x00, 0x02, 0, 0])).unwrap(); // unencrypted: dropped
    f.tx.send(Cmd::Terminate).unwrap();
    let t0 = Instant::now();
    c.run_until(t0 + Duration::from_secs(3)).await.unwrap();
    assert!(t0.elapsed() < Duration::from_secs(2), "ends on termination");
    assert_eq!(c.stats.termination, Some(0x8003_0023));
    assert_eq!(c.stats.bad_msgs, 1);
    let st = c.disconnect().await.unwrap();
    assert_eq!(st.host_msgs.get(&0x0109), Some(&1));
}

#[tokio::test]
async fn control_connect_times_out_without_host() {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap(); // silent: never answers
    let t0 = Instant::now();
    let r = ControlSession::connect(sock.local_addr().unwrap(), CONNECT_DATA, KEY, 1, None).await;
    assert!(r.is_err());
    let e = t0.elapsed();
    assert!(
        e >= Duration::from_secs(9) && e < Duration::from_secs(12),
        "{e:?}"
    );
}

fn ports(f: &Fake, enc_flags: u32) -> brisk_client::rtsp::StreamPorts {
    brisk_client::rtsp::StreamPorts {
        audio: f.audio.port(),
        video: f.video.port(),
        control: f.addr.port(),
        session_id: "S".into(),
        ping_payload: Some(PING_PAYLOAD.into()),
        connect_data: Some(CONNECT_DATA),
        enc_flags,
        key: brisk_client::rtsp::LaunchKey {
            rikey: KEY,
            rikeyid: 77,
        },
    }
}

/// `SS_ENC_VIDEO` + `SS_ENC_AUDIO`: every datagram is decrypted before parsing.
#[tokio::test]
async fn stream_decrypts_video_and_audio() {
    use brisk_proto::sdp::{SS_ENC_AUDIO, SS_ENC_CONTROL_V2, SS_ENC_VIDEO};
    let f = fake_with(Cfg {
        media: Some(Media {
            video_key: Some(KEY),
            audio_key: Some((KEY, 77)),
            ..Media::default()
        }),
        ..Cfg::default()
    });
    let enc = SS_ENC_CONTROL_V2 | SS_ENC_VIDEO | SS_ENC_AUDIO;
    let s = brisk_client::stream::stream(
        "127.0.0.1",
        &ports(&f, enc),
        5,
        Duration::from_secs(2),
        None,
        false,
    )
    .await
    .unwrap();
    assert_eq!(s.video_parse_errors + s.audio_parse_errors, 0, "{s}");
    assert!(s.frames >= 114 && s.idr_frames >= 3, "{s}");
    assert!(s.audio_ok >= 350, "{s}");
    assert_eq!(s.idr_response_ms.len(), 2);
    // Host encrypts audio under another key: payloads fail the padding check, the gate fails.
    let f = fake_with(Cfg {
        media: Some(Media {
            audio_key: Some((*b"another-rikey-16", 77)),
            ..Media::default()
        }),
        ..Cfg::default()
    });
    let e = brisk_client::stream::stream(
        "127.0.0.1",
        &ports(&f, SS_ENC_CONTROL_V2 | SS_ENC_AUDIO),
        6,
        Duration::from_secs(1),
        None,
        false,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(e.contains("parse_errors"), "{e}");
}

/// Runs a control session with media fed to a [`StreamRx`] for `ms`; returns the session stats
/// and the number of IDR requests (0x0302) the fake host decoded.
async fn run_with_loss(lose_frame: fn(u32) -> bool, interval: Duration, ms: u64) -> (u32, usize) {
    let f = fake_with(Cfg {
        media: Some(Media {
            lose_frame,
            ..Media::default()
        }),
        ..Cfg::default()
    });
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 21, av(&f).await)
        .await
        .unwrap();
    c.idr_min_interval = interval;
    let mut rx = brisk_client::stream::StreamRx::new(0, KEY, 0, None).unwrap();
    c.run_until_with(
        Instant::now() + Duration::from_millis(ms),
        &mut |i, d, at| rx.on_datagram(i, d, at),
    )
    .await
    .unwrap();
    let st = c.disconnect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let idr = types(&f).iter().filter(|m| m.1 == 0x0302).count();
    (st.idr_on_loss, idr)
}

/// One frame lost beyond FEC: exactly one IDR request on top of the opening one.
#[tokio::test]
async fn lost_frame_requests_one_idr() {
    let (on_loss, idr) = run_with_loss(|i| i == 30, IDR_MIN_INTERVAL, 1500).await;
    assert_eq!((on_loss, idr), (1, 2));
}

/// Frames keep getting lost from frame 20 (~0.33 s) on: requests keep coming, rate-limited.
#[tokio::test]
async fn continuing_loss_requests_are_rate_limited() {
    let ms = 2000;
    let gap = Duration::from_millis(300);
    let (on_loss, idr) = run_with_loss(|i| i >= 20 && i % 2 == 0, gap, ms).await;
    // ~1.67 s of losses every 33 ms: one request per 300 ms, not one per lost frame (~50).
    assert!((3..=7).contains(&on_loss), "{on_loss}");
    assert_eq!(idr, 1 + on_loss as usize);
}

/// Input packets as the fake decoded them: (ENet channel, plaintext packet).
fn inputs(f: &Fake) -> Vec<(u8, Vec<u8>)> {
    let l = f.log.lock().unwrap();
    l.msgs
        .iter()
        .filter(|m| m.1 == 0x0206)
        .map(|m| (m.0, m.2.clone()))
        .collect()
}

/// `--input-demo` bytes, written out by hand from moonlight Input.h (as tests/input.rs in
/// brisk-proto), not with `encode_input`: size u32 BE (= len - 4), magic u32 LE, body.
#[rustfmt::skip]
fn input_demo_wire() -> Vec<(u8, Vec<u8>)> {
    let pad = |mask: u8| vec![
        0, 0, 0, 30, 0x0C, 0, 0, 0, // NV_MULTI_CONTROLLER_PACKET
        0x1A, 0, 0, 0, mask, 0, 0x14, 0, // headerB, controller 0, activeGamepadMask, midB
        0, 0, 0, 0, // buttonFlags 0, lt 0, rt 0
        0, 0, 0, 0, 0, 0, 0, 0, // lx ly rx ry 0
        0x9C, 0, 0, 0, 0x55, 0, // tailA, buttonFlags2 0, tailB
    ];
    vec![
        (3, vec![0, 0, 0, 8, 7, 0, 0, 0, 0, 5, 0, 0]), // mouse rel dx +5 BE
        (3, vec![0, 0, 0, 8, 7, 0, 0, 0, 0xFF, 0xFB, 0, 0]), // dx -5
        (2, vec![0, 0, 0, 10, 3, 0, 0, 0, 0, 0x10, 0x80, 0, 0, 0]), // VK_SHIFT|0x8000 LE down
        (2, vec![0, 0, 0, 10, 4, 0, 0, 0, 0, 0x10, 0x80, 0, 0, 0]), // up
        (0x10, pad(1)),
        (0x10, pad(0)),
    ]
}

#[tokio::test]
async fn input_demo_reaches_the_host_as_expected_bytes() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 13, av(&f).await)
        .await
        .unwrap();
    let t0 = Instant::now();
    assert_eq!(c.input_demo(&mut |_, _, _| false).await.unwrap(), 6);
    assert!(t0.elapsed() >= INPUT_DEMO_GAP * 6);
    settle(&mut c, 200).await;
    assert_eq!(inputs(&f), input_demo_wire());
    assert_eq!(c.stats.inputs_sent, 6);
    c.disconnect().await.unwrap();
}

/// `stream --input-demo`: the demo runs inside the gated stream, which still passes.
#[tokio::test]
async fn stream_input_demo_passes_gate() {
    let f = fake_with(Cfg {
        media: Some(Media::default()),
        ..Cfg::default()
    });
    let s = brisk_client::stream::stream(
        "127.0.0.1",
        &ports(&f, 0),
        8,
        Duration::from_secs(2),
        None,
        true,
    )
    .await
    .unwrap();
    assert!(s.passed() && s.control.inputs_sent == 6, "{s}");
    assert!(s.to_string().contains("inputs_sent        6"), "{s}");
    assert_eq!(inputs(&f), input_demo_wire());
}

/// A scheduled IDR request also covers a loss still waiting on the rate limit: no extra request.
#[tokio::test]
async fn scheduled_idr_clears_pending_loss_request() {
    let f = fake_with(Cfg {
        media: Some(Media {
            lose_frame: |i| i == 30,
            ..Media::default()
        }),
        ..Cfg::default()
    });
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 23, av(&f).await)
        .await
        .unwrap();
    c.idr_min_interval = Duration::from_secs(60); // the loss request stays pending
    let mut rx = brisk_client::stream::StreamRx::new(0, KEY, 0, None).unwrap();
    let mut sink = |i: usize, d: &[u8], at: Instant| rx.on_datagram(i, d, at);
    c.run_until_with(Instant::now() + Duration::from_millis(1200), &mut sink)
        .await
        .unwrap();
    c.request_idr().await.unwrap();
    c.idr_min_interval = Duration::ZERO;
    c.run_until_with(Instant::now() + Duration::from_millis(300), &mut sink)
        .await
        .unwrap();
    assert_eq!((c.stats.idr_on_loss, c.stats.idr_sent), (0, 2));
    c.disconnect().await.unwrap();
}

/// A [`ControlHandle`] used from another task drives the running session: the input and the
/// IDR request reach the host; once the session is gone the handle says so.
#[tokio::test]
async fn control_handle_sends_input_and_idr_from_another_task() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 31, av(&f).await)
        .await
        .unwrap();
    let h = c.handle();
    let key = InputEvent::Key {
        vk: 0x41,
        down: true,
        modifiers: 0,
    };
    let driver = async {
        h.clone().send_input(key.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        h.request_idr().await.unwrap();
    };
    let (r, ()) = tokio::join!(
        c.run_until(Instant::now() + Duration::from_millis(500)),
        driver
    );
    r.unwrap();
    assert_eq!(
        inputs(&f),
        vec![(2, encode_input(&key).unwrap()[0].clone())]
    );
    let idr: Vec<u8> = types(&f)
        .iter()
        .filter(|m| m.1 == 0x0302)
        .map(|m| m.0)
        .collect();
    assert_eq!(idr, [0, 1], "opening IDR, then the handle's on URGENT");
    assert_eq!((c.stats.idr_sent, c.stats.inputs_sent), (2, 1));
    c.disconnect().await.unwrap();
    assert!(matches!(
        h.request_idr().await,
        Err(brisk_client::Error::Closed(_))
    ));
}

/// Handle IDR requests every 10 ms for 500 ms are merged into at most one per 100 ms, and the
/// last one is still sent (merged, not dropped).
#[tokio::test]
async fn control_handle_idr_is_rate_limited_to_one_per_100ms() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 33, av(&f).await)
        .await
        .unwrap();
    let h = c.handle();
    let driver = async {
        for _ in 0..50 {
            h.request_idr().await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    let (r, ()) = tokio::join!(
        c.run_until(Instant::now() + Duration::from_millis(800)),
        driver
    );
    r.unwrap();
    let sent = c.stats.idr_sent - 1; // minus the opening request
    assert!((4..=7).contains(&sent), "{sent}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        types(&f).iter().filter(|m| m.1 == 0x0302).count(),
        1 + sent as usize
    );
    c.disconnect().await.unwrap();
}

/// Tap mode end to end against the fake: the fake connects to our listener on its first video
/// ping (as the patched Sunshine does), frames come over the tap, the IDR requests made through
/// the control handle are answered, no RTP video arrives, and the tap gate passes.
#[tokio::test]
async fn stream_tap_receives_frames_from_the_tap() {
    use brisk_client::stream::{stream_tap, Outcome};
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("tap.sock");
    let l = brisk_client::tap::TapListener::bind(&path).unwrap();
    let f = fake_with(Cfg {
        media: Some(Media::default()),
        tap: Some(path),
        ..Cfg::default()
    });
    let rec = t.path().join("rec");
    let o = stream_tap(
        "127.0.0.1",
        &ports(&f, 0),
        41,
        Duration::from_secs(3),
        Some(&rec),
        &l,
    )
    .await
    .unwrap();
    let Outcome::Tap(s) = o else {
        panic!("fell back to RTP: {o:?}")
    };
    assert!(s.passed(), "{s}");
    assert!(s.frames >= 171 && s.idr_frames >= 3, "{s}");
    assert_eq!(
        (s.rtp_video_datagrams, s.idx_gaps, s.dropped),
        (0, 0, 0),
        "{s}"
    );
    assert_eq!(s.idr_response_ms.len(), 2, "{s}");
    let fp = s.host_ms.as_ref().unwrap();
    assert!((fp[0] - 2.0).abs() < 0.01, "{s}");
    assert_eq!(
        f.log.lock().unwrap().video_sent,
        0,
        "no RTP video from the fake"
    );
    // --record-samples writes the first 60 tap messages, as received.
    let n = std::fs::read_dir(rec.join("tap")).unwrap().count();
    assert_eq!(n, 60);
    let m = std::fs::read(rec.join("tap/000.bin")).unwrap();
    assert_eq!(
        brisk_proto::tap::parse_tap(&m).unwrap().0.connect_data,
        CONNECT_DATA
    );
}

/// The tap connect fails (nobody listens where the fake connects): RTP video wins the race and
/// the session runs the normal RTP path and gate.
#[tokio::test]
async fn stream_tap_falls_back_to_rtp() {
    use brisk_client::stream::{stream_tap, Outcome};
    let t = tempfile::tempdir().unwrap();
    let l = brisk_client::tap::TapListener::bind(&t.path().join("tap.sock")).unwrap();
    let f = fake_with(Cfg {
        media: Some(Media::default()),
        tap: Some(t.path().join("elsewhere.sock")),
        ..Cfg::default()
    });
    let o = stream_tap(
        "127.0.0.1",
        &ports(&f, 0),
        43,
        Duration::from_secs(2),
        None,
        &l,
    )
    .await
    .unwrap();
    let Outcome::Rtp(s) = o else {
        panic!("tap mode without a tap: {o:?}")
    };
    assert!(s.passed() && s.frames >= 114, "{s}");
}

/// The host ends the session while a handle is held and the session is kept (not dropped, not
/// disconnected): handle calls fail with `Closed` at once instead of being queued as Ok.
#[tokio::test]
async fn control_handle_reports_closed_after_host_ends_session() {
    let f = fake(0);
    let mut c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 35, av(&f).await)
        .await
        .unwrap();
    let h = c.handle();
    settle(&mut c, 150).await;
    f.tx.send(Cmd::Terminate).unwrap();
    c.run_until(Instant::now() + Duration::from_secs(3))
        .await
        .unwrap();
    assert!(!c.is_open());
    let quick = Duration::from_millis(500);
    let r = tokio::time::timeout(quick, h.request_idr())
        .await
        .expect("request_idr hung");
    assert!(matches!(r, Err(brisk_client::Error::Closed(_))), "{r:?}");
    let key = InputEvent::Key {
        vk: 0x41,
        down: true,
        modifiers: 0,
    };
    let r = tokio::time::timeout(quick, h.send_input(key))
        .await
        .expect("send_input hung");
    assert!(matches!(r, Err(brisk_client::Error::Closed(_))), "{r:?}");
    drop(c);
}

/// A session that is held but never run does not drain commands: once the queue is full a handle
/// call gives up with a typed error after a bounded wait instead of blocking forever.
#[tokio::test]
async fn control_handle_does_not_hang_on_an_idle_session() {
    let f = fake(0);
    let c = ControlSession::connect(f.addr, CONNECT_DATA, KEY, 37, av(&f).await)
        .await
        .unwrap();
    let h = c.handle();
    let mut results = Vec::new();
    for _ in 0..66 {
        let r = tokio::time::timeout(Duration::from_secs(5), h.request_idr())
            .await
            .expect("request_idr hung");
        results.push(r);
    }
    assert!(results[..64].iter().all(|r| r.is_ok()), "{results:?}");
    assert!(
        results[64..]
            .iter()
            .all(|r| matches!(r, Err(brisk_client::Error::Timeout(_)))),
        "{results:?}"
    );
    drop(c);
}
