//! Control session against a local fake Sunshine control server (127.0.0.1, ephemeral port);
//! the fake and what it mirrors are described in common/fake_host.rs.

mod common;

use brisk_client::control::{ControlSession, IDR_MIN_INTERVAL};
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
