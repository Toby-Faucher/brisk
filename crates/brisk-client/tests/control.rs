//! Control session against a local fake Sunshine control server (127.0.0.1, ephemeral port).
//!
//! ENet on the fake side is rusty_enet (a Rust port of ENet 1.3.18), not our own code, so the
//! handshake/ACK/retransmit layer is checked against an independent implementation. The
//! control layer above it is written here from Sunshine stream.cpp, separately from
//! brisk_proto::control: `get_session` (connect-data match, else `enet_peer_disconnect_now`),
//! `IDX_ENCRYPTED` handler (length >= 24, tag then ciphertext, IV = seq LE + 'C','C', inner
//! type u16 LE, payload after the 4-byte V2 header), `encode_control` (host seq from 0, IV
//! 'H','C') and `control_server_t::send` (channel 0, reliable). Places where this can only
//! mirror our reading of stream.cpp are listed in fixtures/control/README.md.

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce, Tag};
use brisk_client::control::{AvPing, ControlSession};
use brisk_proto::input::{encode_input, InputEvent};
use rusty_enet as enet;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const KEY: [u8; 16] = *b"control-test-key";
const CONNECT_DATA: u32 = 1396329308;
/// X-SS-Ping-Payload from fixtures/rtsp/010 and 012 (Sunshine sends one value for audio and video).
const PING_PAYLOAD: &str = "FE073FEB9401CE42";
/// Stands in for Sunshine's `ping_timeout` (10 s, config.cpp:815), shortened for tests.
const FAKE_PING_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Default, Debug)]
struct Log {
    connects: Vec<u32>,
    rejected: u32,
    disconnects: u32,
    /// (ENet channel, inner type, payload after the V2 header)
    msgs: Vec<(u8, u16, Vec<u8>)>,
    unencrypted: u32,
    bad_tag: u32,
    /// Datagrams containing the ping payload, per socket [video, audio].
    av_pings: [u32; 2],
    /// Raw first ping seen per socket.
    first_ping: [Option<Vec<u8>>; 2],
    /// `recv_ping` timed out on a socket: session::stop, ENet peer disconnected.
    stopped: bool,
}

enum Cmd {
    /// (type, payload) sealed and sent reliable on channel 0.
    Send(u16, Vec<u8>),
    /// Raw ENet packet, no encryption.
    SendRaw(Vec<u8>),
    /// Termination 0x80030023 then `free_host`'s disconnect_now.
    Terminate,
}

/// UDP socket that drops the first `drop` received datagrams (to force retransmission).
struct Lossy {
    sock: UdpSocket,
    drop: usize,
}
impl enet::Socket for Lossy {
    type Address = SocketAddr;
    type Error = std::io::Error;
    fn init(&mut self, o: enet::SocketOptions) -> Result<(), Self::Error> {
        enet::Socket::init(&mut self.sock, o)
    }
    fn send(&mut self, a: SocketAddr, b: &[u8]) -> Result<usize, Self::Error> {
        enet::Socket::send(&mut self.sock, a, b)
    }
    fn receive(
        &mut self,
        buf: &mut [u8; enet::MTU_MAX],
    ) -> Result<Option<(SocketAddr, enet::PacketReceived)>, Self::Error> {
        let r = enet::Socket::receive(&mut self.sock, buf)?;
        if r.is_some() && self.drop > 0 {
            self.drop -= 1;
            return Ok(None);
        }
        Ok(r)
    }
}

fn iv(seq: u32, dir: u8) -> [u8; 12] {
    let mut iv = [0u8; 12];
    iv[..4].copy_from_slice(&seq.to_le_bytes());
    iv[10] = dir;
    iv[11] = b'C';
    iv
}

struct Fake {
    addr: SocketAddr,
    video: SocketAddr,
    audio: SocketAddr,
    log: Arc<Mutex<Log>>,
    tx: mpsc::Sender<Cmd>,
}

fn fake(drop: usize) -> Fake {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = sock.local_addr().unwrap();
    let av = [
        UdpSocket::bind("127.0.0.1:0").unwrap(),
        UdpSocket::bind("127.0.0.1:0").unwrap(),
    ];
    let (video, audio) = (av[0].local_addr().unwrap(), av[1].local_addr().unwrap());
    for s in &av {
        s.set_nonblocking(true).unwrap();
    }
    let log = Arc::new(Mutex::new(Log::default()));
    let (tx, rx) = mpsc::channel::<Cmd>();
    let l = log.clone();
    std::thread::spawn(move || {
        // net::host_create: 128 peers, channel limit 0 (= max), no bandwidth limits.
        let settings = enet::HostSettings {
            peer_limit: 128,
            channel_limit: 255,
            ..Default::default()
        };
        let mut host = enet::Host::new(Lossy { sock, drop }, settings).unwrap();
        let cipher = Aes128Gcm::new(&KEY.into());
        let mut peer: Option<enet::PeerID> = None;
        let mut seq: u32 = 0;
        // Sunshine starts videoThread/audioThread at PLAY (stream.cpp:2289-2293); the fake is
        // created right where PLAY would have completed.
        let started = Instant::now();
        let mut ping_peer: [Option<SocketAddr>; 2] = [None, None];
        let mut stopped = false;
        let mut buf = [0u8; 2048];
        while started.elapsed() < Duration::from_secs(30) {
            // recv_ping (stream.cpp:2092-2120): any datagram containing the payload is a ping and
            // its source becomes the media peer. Then send some media-like datagrams there.
            for (i, s) in av.iter().enumerate() {
                while let Ok((n, from)) = s.recv_from(&mut buf) {
                    let d = &buf[..n];
                    if !d.windows(16).any(|w| w == PING_PAYLOAD.as_bytes()) {
                        continue;
                    }
                    let mut log = l.lock().unwrap();
                    log.av_pings[i] += 1;
                    if ping_peer[i].is_none() && !stopped {
                        ping_peer[i] = Some(from);
                        log.first_ping[i] = Some(d.to_vec());
                        for k in 0..3 - i {
                            s.send_to(&[0x80 + k as u8; 100], from).unwrap();
                        }
                    }
                }
            }
            if !stopped
                && started.elapsed() >= FAKE_PING_TIMEOUT
                && ping_peer.iter().any(|p| p.is_none())
            {
                // "Initial Ping Timeout" -> fail_guard session::stop -> broadcast loop
                // enet_peer_disconnect_now (stream.cpp:1336-1349).
                stopped = true;
                l.lock().unwrap().stopped = true;
                if let Some(id) = peer.take() {
                    host.peer_mut(id).disconnect_now(0);
                }
            }
            while let Ok(c) = rx.try_recv() {
                let Some(id) = peer else { continue };
                let mut seal = |t: u16, p: &[u8]| {
                    let mut pt = t.to_le_bytes().to_vec();
                    pt.extend_from_slice(&(p.len() as u16).to_le_bytes());
                    pt.extend_from_slice(p);
                    let tag = cipher
                        .encrypt_in_place_detached(Nonce::from_slice(&iv(seq, b'H')), b"", &mut pt)
                        .unwrap();
                    let mut w = 1u16.to_le_bytes().to_vec();
                    w.extend_from_slice(&((pt.len() + 16 + 4) as u16).to_le_bytes());
                    w.extend_from_slice(&seq.to_le_bytes());
                    w.extend_from_slice(&tag);
                    w.extend_from_slice(&pt);
                    seq += 1;
                    w
                };
                let p = host.peer_mut(id);
                match c {
                    Cmd::Send(t, payload) => {
                        let w = seal(t, &payload);
                        p.send(0, &enet::Packet::reliable(w.as_slice())).unwrap();
                    }
                    Cmd::SendRaw(w) => {
                        p.send(0, &enet::Packet::reliable(w.as_slice())).unwrap();
                    }
                    Cmd::Terminate => {
                        let w = seal(0x0109, &0x8003_0023u32.to_be_bytes());
                        p.send(0, &enet::Packet::reliable(w.as_slice())).unwrap();
                        host.flush();
                        host.peer_mut(id).disconnect_now(0);
                        peer = None;
                    }
                }
            }
            loop {
                let ev = match host.service() {
                    Ok(Some(e)) => e.no_ref(),
                    _ => break,
                };
                let mut log = l.lock().unwrap();
                match ev {
                    enet::EventNoRef::Connect { peer: id, data } => {
                        log.connects.push(data);
                        if data == CONNECT_DATA && !stopped {
                            peer = Some(id);
                        } else {
                            log.rejected += 1;
                            host.peer_mut(id).disconnect_now(0);
                        }
                    }
                    enet::EventNoRef::Disconnect { .. } => {
                        log.disconnects += 1;
                        peer = None;
                    }
                    enet::EventNoRef::Receive {
                        channel_id, packet, ..
                    } => {
                        let d = packet.data();
                        if d.len() < 2 || u16::from_le_bytes([d[0], d[1]]) != 1 {
                            log.unencrypted += 1; // controlProtocolType 13 drops these
                            continue;
                        }
                        if d.len() < 8 {
                            continue;
                        }
                        let len = u16::from_le_bytes([d[2], d[3]]) as usize;
                        if len < 24 || d.len() < 4 + len {
                            continue; // "Control: Runt packet"
                        }
                        let s = u32::from_le_bytes(d[4..8].try_into().unwrap());
                        let mut pt = d[24..4 + len].to_vec();
                        let ok = cipher.decrypt_in_place_detached(
                            Nonce::from_slice(&iv(s, b'C')),
                            b"",
                            &mut pt,
                            Tag::from_slice(&d[8..24]),
                        );
                        if ok.is_err() {
                            log.bad_tag += 1;
                            continue;
                        }
                        let t = u16::from_le_bytes([pt[0], pt[1]]);
                        log.msgs.push((channel_id, t, pt[4..].to_vec()));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    Fake {
        addr,
        video,
        audio,
        log,
        tx,
    }
}

fn types(f: &Fake) -> Vec<(u8, u16)> {
    let l = f.log.lock().unwrap();
    l.msgs.iter().map(|(c, t, _)| (*c, *t)).collect()
}

async fn av(f: &Fake) -> Option<AvPing> {
    let ip = f.video.ip();
    Some(
        AvPing::new(ip, f.video.port(), f.audio.port(), PING_PAYLOAD)
            .await
            .unwrap(),
    )
}

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
