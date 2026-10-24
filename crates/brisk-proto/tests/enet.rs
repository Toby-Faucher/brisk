//! Vectors hand-built from moonlight-common-c `enet/include/enet/protocol.h` (packed, network order)
//! and `host.c`/`protocol.c` behaviour. Cross-implementation interop (rusty_enet as the host) lives
//! in brisk-client/tests/control.rs.
use brisk_proto::enet::*;

const CONNECT_DATA: u32 = 1396329308; // X-SS-Connect-Data in fixtures/rtsp/014
const CONNECT_ID: u32 = 0x1122_3344;

/// Server datagram: header (our peer id 0, session 1, sent time) + commands.
fn from_host(sent_time: Option<u16>, cmds: &[&[u8]]) -> Vec<u8> {
    let mut id: u16 = 1 << 12;
    if sent_time.is_some() {
        id |= 0x8000;
    }
    let mut d = id.to_be_bytes().to_vec();
    if let Some(t) = sent_time {
        d.extend_from_slice(&t.to_be_bytes());
    }
    for c in cmds {
        d.extend_from_slice(c);
    }
    d
}

fn verify_cmd(connect_id: u32, throttle_interval: u32) -> Vec<u8> {
    let mut c = vec![0x83, 0xFF, 0, 1];
    c.extend_from_slice(&7u16.to_be_bytes()); // host's peer index for us
    c.extend_from_slice(&[1, 2]); // incomingSessionID, outgoingSessionID
    for v in [900u32, 65536, 0x30, 0, 0, throttle_interval, 2, 2] {
        c.extend_from_slice(&v.to_be_bytes());
    }
    c.extend_from_slice(&connect_id.to_ne_bytes());
    c
}

fn reliable(channel: u8, seq: u16, data: &[u8]) -> Vec<u8> {
    let mut c = vec![0x86, channel];
    c.extend_from_slice(&seq.to_be_bytes());
    c.extend_from_slice(&(data.len() as u16).to_be_bytes());
    c.extend_from_slice(data);
    c
}

fn connected(now: u32) -> EnetPeer {
    let (mut p, _) = EnetPeer::connect(CONNECT_DATA, CONNECT_ID, now);
    let ev = p
        .handle_datagram(&from_host(Some(5), &[&verify_cmd(CONNECT_ID, 5000)]), now)
        .unwrap();
    assert_eq!(ev, vec![EnetEvent::Connected]);
    p
}

fn receives(ev: &[EnetEvent]) -> Vec<Vec<u8>> {
    ev.iter()
        .filter_map(|e| match e {
            EnetEvent::Receive { data, .. } => Some(data.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn enet_connect_encodes_known_header() {
    let (p, d) = EnetPeer::connect(CONNECT_DATA, CONNECT_ID, 0x0001_1234);
    let mut want = vec![
        0x8F, 0xFF, // peerID 0xFFF (none yet) | SENT_TIME
        0x12, 0x34, // sentTime = now & 0xFFFF
        0x82, 0xFF, 0x00, 0x01, // CONNECT|ACK, channel 0xFF, reliable seq 1
        0x00, 0x00, // outgoingPeerID: our incoming id 0
        0xFF, 0xFF, // incoming/outgoing session id
        0x00, 0x00, 0x03, 0x84, // mtu 900
        0x00, 0x01, 0x00, 0x00, // window 65536
        0x00, 0x00, 0x00, 0x30, // 48 channels
        0, 0, 0, 0, 0, 0, 0, 0, // bandwidth in/out: unlimited
        0x00, 0x00, 0x13, 0x88, // throttle interval 5000
        0x00, 0x00, 0x00, 0x02, // acceleration
        0x00, 0x00, 0x00, 0x02, // deceleration
    ];
    want.extend_from_slice(&CONNECT_ID.to_ne_bytes()); // opaque, host order
    want.extend_from_slice(&[0x53, 0x3A, 0x4B, 0x5C]); // 1396329308 big-endian
    assert_eq!(d, want);
    assert_eq!(p.state(), PeerState::Connecting);
}

#[test]
fn enet_verify_connect_then_ack_carries_session_and_peer_id() {
    let mut p = connected(100);
    let (out, _) = p.poll_timeout(100).unwrap();
    // ACK of VERIFY_CONNECT: header = host peer id 7 | our outgoing session 2 << 12, no sent time.
    assert_eq!(out, vec![vec![0x20, 0x07, 0x01, 0xFF, 0, 1, 0, 1, 0, 5]]);
    assert_eq!(p.in_flight(), 0, "VERIFY_CONNECT retires CONNECT");
}

#[test]
fn enet_verify_mismatch_refused() {
    let (mut p, _) = EnetPeer::connect(CONNECT_DATA, CONNECT_ID, 0);
    let r = p.handle_datagram(&from_host(Some(5), &[&verify_cmd(CONNECT_ID ^ 1, 5000)]), 1);
    assert_eq!(r.unwrap(), vec![]);
    assert_eq!(p.state(), PeerState::Disconnected);
    let (mut p, _) = EnetPeer::connect(CONNECT_DATA, CONNECT_ID, 0);
    p.handle_datagram(&from_host(Some(5), &[&verify_cmd(CONNECT_ID, 4000)]), 1)
        .unwrap();
    assert_eq!(p.state(), PeerState::Disconnected);
}

#[test]
fn enet_ack_for_reliable() {
    let mut p = connected(0);
    p.poll_timeout(0).unwrap();
    let ev = p
        .handle_datagram(&from_host(Some(0xABCD), &[&reliable(0, 1, b"hi")]), 10)
        .unwrap();
    assert_eq!(receives(&ev), vec![b"hi".to_vec()]);
    let (out, _) = p.poll_timeout(10).unwrap();
    assert_eq!(
        out,
        vec![vec![0x20, 0x07, 0x01, 0x00, 0, 1, 0, 1, 0xAB, 0xCD]]
    );
    // Without a sent-time header ENet does not ACK (and stops parsing).
    p.handle_datagram(&from_host(None, &[&reliable(0, 2, b"x")]), 11)
        .unwrap();
    assert_eq!(p.poll_timeout(11).unwrap().0, Vec::<Vec<u8>>::new());
}

#[test]
fn enet_send_reliable_and_unreliable_layout() {
    let mut p = connected(0);
    p.poll_timeout(0).unwrap();
    let d = p.send(3, true, b"abc", 0x10).unwrap();
    assert_eq!(
        d,
        vec![0xA0, 0x07, 0x00, 0x10, 0x86, 3, 0, 1, 0, 3, b'a', b'b', b'c']
    );
    let d = p.send(3, false, b"z", 0x11).unwrap();
    assert_eq!(d, vec![0x20, 0x07, 0x07, 3, 0, 1, 0, 1, 0, 1, b'z']);
    assert_eq!(p.send(0x30, true, b"", 0), Err(EnetError::BadChannel));
    assert_eq!(
        p.send(0, true, &[0u8; 900], 0),
        Err(EnetError::TooLarge),
        "no fragmentation"
    );
}

#[test]
fn enet_ack_retires_and_reports() {
    let mut p = connected(0);
    p.send(0, true, b"ping", 0).unwrap();
    let ack = [0x01, 0, 0, 1, 0, 1, 0, 0];
    let ev = p.handle_datagram(&from_host(None, &[&ack]), 20).unwrap();
    assert_eq!(ev, vec![EnetEvent::Acked { channel: 0, seq: 1 }]);
    assert_eq!(p.in_flight(), 0);
    assert_eq!(p.rtt(), 20, "first sample sets RTT");
}

#[test]
fn enet_sequence_wraps_at_16_bits() {
    let mut p = connected(0);
    let mut n = 0usize;
    for i in 1..=65537u32 {
        let seq = i as u16; // 65536 -> 0, 65537 -> 1
        let ev = p
            .handle_datagram(
                &from_host(Some(1), &[&reliable(0, seq, &i.to_le_bytes())]),
                1,
            )
            .unwrap();
        assert_eq!(receives(&ev), vec![i.to_le_bytes().to_vec()], "seq {seq}");
        n += 1;
        if i % 64 == 0 {
            p.poll_timeout(1).unwrap();
        }
    }
    assert_eq!(n, 65537);
    // seq 1 again is now a duplicate (delivered as 65537): dropped, still ACKed.
    let ev = p
        .handle_datagram(&from_host(Some(1), &[&reliable(0, 1, b"dup")]), 1)
        .unwrap();
    assert!(receives(&ev).is_empty());
}

#[test]
fn enet_reorders_reliable() {
    let mut p = connected(0);
    let ev = p
        .handle_datagram(&from_host(Some(1), &[&reliable(0, 2, b"b")]), 1)
        .unwrap();
    assert!(receives(&ev).is_empty());
    let ev = p
        .handle_datagram(&from_host(Some(1), &[&reliable(0, 1, b"a")]), 1)
        .unwrap();
    assert_eq!(receives(&ev), vec![b"a".to_vec(), b"b".to_vec()]);
    // Far beyond the reorder window: dropped and not ACKed.
    p.poll_timeout(1).unwrap();
    p.handle_datagram(&from_host(Some(1), &[&reliable(0, 500, b"x")]), 1)
        .unwrap();
    assert!(p.poll_timeout(1).unwrap().0.is_empty());
}

#[test]
fn enet_retransmits_then_times_out() {
    let mut p = connected(0);
    p.poll_timeout(0).unwrap();
    let first = p.send(0, true, b"x", 0).unwrap();
    let (out, next) = p.poll_timeout(100).unwrap();
    assert!(out.is_empty());
    assert!(next.unwrap() > 0);
    let (out, _) = p.poll_timeout(600).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0][4..], first[4..], "same command, new sent time");
    let mut t = 600;
    let r = loop {
        t += 100;
        match p.poll_timeout(t) {
            Ok(_) => assert!(t < 11_000),
            Err(e) => break e,
        }
    };
    assert_eq!(r, EnetError::Timeout);
    assert_eq!(p.state(), PeerState::Disconnected);
}

#[test]
fn enet_pings_when_idle() {
    let mut p = connected(0);
    let (out, next) = p.poll_timeout(0).unwrap();
    assert_eq!(out.len(), 1); // the VERIFY ack
    assert_eq!(next, Some(500));
    let (out, _) = p.poll_timeout(500).unwrap();
    assert_eq!(out, vec![vec![0xA0, 0x07, 0x01, 0xF4, 0x85, 0xFF, 0, 2]]);
}

#[test]
fn enet_disconnect_later_waits_for_acks() {
    let mut p = connected(0);
    p.poll_timeout(0).unwrap();
    p.send(0, true, b"x", 0).unwrap();
    assert!(p.disconnect(1).is_empty());
    assert_eq!(p.state(), PeerState::DisconnectLater);
    assert!(p.send(0, true, b"y", 1).is_err());
    let ack = [0x01, 0, 0, 1, 0, 1, 0, 0];
    p.handle_datagram(&from_host(None, &[&ack]), 5).unwrap();
    let (out, _) = p.poll_timeout(5).unwrap();
    // DISCONNECT|ACK on 0xFF, system seq 2, data 0.
    assert_eq!(
        out,
        vec![vec![0xA0, 0x07, 0, 5, 0x84, 0xFF, 0, 2, 0, 0, 0, 0]]
    );
    let ack = [0x01, 0xFF, 0, 2, 0, 2, 0, 5];
    let ev = p.handle_datagram(&from_host(None, &[&ack]), 6).unwrap();
    assert!(ev.contains(&EnetEvent::Disconnected { data: 0 }));
    assert_eq!(p.state(), PeerState::Disconnected);
}

#[test]
fn enet_host_disconnect_now() {
    let mut p = connected(0);
    // enet_peer_disconnect_now: DISCONNECT|UNSEQUENCED, no ACK expected.
    let c = [0x44, 0xFF, 0, 2, 0, 0, 0, 0];
    let ev = p.handle_datagram(&from_host(Some(1), &[&c]), 1).unwrap();
    assert_eq!(ev, vec![EnetEvent::Disconnected { data: 0 }]);
}

#[test]
fn enet_rejects_wrong_session_and_garbage_without_panic() {
    let mut p = connected(0);
    let mut d = from_host(Some(1), &[&reliable(0, 1, b"a")]);
    d[0] ^= 0x30; // session bits
    assert!(p.handle_datagram(&d, 1).is_err());
    assert!(p.handle_datagram(&[0u8; 5000], 1).is_err());
    let mut x: u32 = 0x9E37_79B9;
    for len in 0..300usize {
        let mut d: Vec<u8> = (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        if len >= 2 {
            d[0] = 0x90 | (d[0] & 0x80); // keep a valid peer id/session half the time
            d[1] = 0;
        }
        let _ = p.handle_datagram(&d, len as u32);
        let _ = p.poll_timeout(len as u32);
    }
}

#[test]
fn enet_disconnect_now_is_unsequenced() {
    let mut p = connected(0);
    p.poll_timeout(0).unwrap();
    assert_eq!(
        p.disconnect_now(9).unwrap(),
        vec![0x20, 0x07, 0x44, 0xFF, 0, 2, 0, 0, 0, 0]
    );
    assert_eq!(p.state(), PeerState::Disconnected);
    assert!(p.disconnect_now(9).is_none());
}
