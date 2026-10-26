//! Control-stream vectors from Sunshine stream.cpp (`encode_control`, `IDX_ENCRYPTED` handler,
//! `control_*_t` structs) and moonlight ControlStream.c. Not yet checked against live bytes.
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use brisk_proto::control::*;
use brisk_proto::input::InputEvent;

const KEY: [u8; 16] = *b"0123456789abcdef";

#[test]
fn control_gcm_roundtrip() {
    let mut c = Control::new(KEY);
    let mut h = Control::host(KEY);
    for m in [ControlMsg::IdrRequest, ControlMsg::StartB, ControlMsg::Ping] {
        assert_eq!(h.decode(&c.encode(&m).unwrap()).unwrap(), m);
    }
    for m in [
        ControlMsg::Rumble {
            controller: 1,
            low: 0x1234,
            high: 0xFFFF,
        },
        ControlMsg::Termination(TERMINATION_GRACEFUL),
        ControlMsg::HdrMode { enabled: false },
    ] {
        assert_eq!(c.decode(&h.encode(&m).unwrap()).unwrap(), m);
    }
}

#[test]
fn control_wire_layout_matches_stream_cpp() {
    let mut c = Control::new(KEY);
    c.encode(&ControlMsg::Ping).unwrap(); // seq 0
    let w = c.encode(&ControlMsg::IdrRequest).unwrap(); // seq 1
    // 0x0001 LE, length = seq(4) + tag(16) + V2 header(4) + payload(2) = 26, seq 1 LE.
    assert_eq!(w[..8], [0x01, 0x00, 26, 0x00, 1, 0, 0, 0]);
    assert_eq!(w.len(), 4 + 26);
    // Independent decrypt: IV = seq LE, zeros, 'C','C'; tag then ciphertext.
    let mut iv = [0u8; 12];
    iv[0] = 1;
    iv[10] = b'C';
    iv[11] = b'C';
    let mut pt = w[24..].to_vec();
    Aes128Gcm::new(&KEY.into())
        .decrypt_in_place_detached(
            Nonce::from_slice(&iv),
            b"",
            &mut pt,
            aes_gcm::Tag::from_slice(&w[8..24]),
        )
        .unwrap();
    assert_eq!(pt, [0x02, 0x03, 0x02, 0x00, 0x00, 0x00]); // 0x0302, len 2, {0,0}
}

#[test]
fn control_decodes_sunshine_rumble_and_termination() {
    // Build what stream.cpp sends with an independent sealer: IV seq LE + 'H','C'.
    let seal = |seq: u32, pt: &[u8]| {
        let mut iv = [0u8; 12];
        iv[..4].copy_from_slice(&seq.to_le_bytes());
        iv[10] = b'H';
        iv[11] = b'C';
        let mut ct = pt.to_vec();
        let tag = Aes128Gcm::new(&KEY.into())
            .encrypt_in_place_detached(Nonce::from_slice(&iv), b"", &mut ct)
            .unwrap();
        let mut w = vec![1, 0];
        w.extend_from_slice(&((4 + 16 + ct.len()) as u16).to_le_bytes());
        w.extend_from_slice(&seq.to_le_bytes());
        w.extend_from_slice(&tag);
        w.extend_from_slice(&ct);
        w
    };
    let mut c = Control::new(KEY);
    // control_rumble_t: type 0x010b, len 10, useless 0xC0FFEE, id 2, low 0x0100, high 0x0200.
    let rumble = [
        0x0b, 0x01, 10, 0, 0xEE, 0xFF, 0xC0, 0, 2, 0, 0x00, 0x01, 0x00, 0x02,
    ];
    assert_eq!(
        c.decode(&seal(0, &rumble)).unwrap(),
        ControlMsg::Rumble {
            controller: 2,
            low: 0x100,
            high: 0x200
        }
    );
    // control_terminate_t: type 0x0109, len 4, ec big-endian.
    let term = [0x09, 0x01, 4, 0, 0x80, 0x03, 0x00, 0x23];
    assert_eq!(
        c.decode(&seal(1, &term)).unwrap(),
        ControlMsg::Termination(0x8003_0023)
    );
    assert_eq!(c.decode(&seal(1, &term)), Err(ControlError::Replay));
}

#[test]
fn control_rejects_tampered_tag() {
    let mut c = Control::new(KEY);
    let mut h = Control::host(KEY);
    let mut w = c.encode(&ControlMsg::StartB).unwrap();
    w[10] ^= 1;
    assert_eq!(h.decode(&w), Err(ControlError::BadTag));
    let mut w = c.encode(&ControlMsg::StartB).unwrap();
    *w.last_mut().unwrap() ^= 1;
    assert_eq!(h.decode(&w), Err(ControlError::BadTag));
    // Wrong direction: a client message does not open as host-originated.
    let w = c.encode(&ControlMsg::StartB).unwrap();
    assert_eq!(Control::new(KEY).decode(&w), Err(ControlError::BadTag));
}

#[test]
#[allow(non_snake_case)]
fn control_unknown_type_is_Unknown_not_panic() {
    let mut c = Control::new(KEY);
    let mut h = Control::host(KEY);
    let m = ControlMsg::Unknown(0x5504, vec![1, 2, 3]);
    assert_eq!(c.decode(&h.encode(&m).unwrap()).unwrap(), m);
    // Short known types fall back to Unknown rather than indexing out of range.
    let m = ControlMsg::Unknown(ptype::RUMBLE, vec![1]);
    assert_eq!(c.decode(&h.encode(&m).unwrap()).unwrap(), m);
    for d in [
        &[][..],
        &[1],
        &[1, 0],
        &[1, 0, 24, 0],
        &[1, 0, 23, 0, 0, 0, 0, 0],
        &[2, 0, 0, 0],
    ] {
        assert!(c.decode(d).is_err());
    }
    let mut x = 7u32;
    for n in 0..200 {
        let mut d: Vec<u8> = (0..n)
            .map(|_| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                (x >> 16) as u8
            })
            .collect();
        if n >= 4 {
            d[0] = 1;
            d[1] = 0;
            d[2] = (n - 4) as u8;
            d[3] = 0;
        }
        let _ = c.decode(&d);
    }
}

#[test]
fn control_payload_over_64k_rejected() {
    let mut c = Control::new(KEY);
    let mut big = vec![1u8, 0, 0xFF, 0xFF];
    big.resize(70_000, 0);
    assert_eq!(c.decode(&big), Err(ControlError::TooLarge));
    assert_eq!(
        c.encode(&ControlMsg::Unknown(0x1234, vec![0; 252])),
        Err(ControlError::TooLarge)
    );
}

#[test]
fn control_input_routes_like_moonlight() {
    let key = ControlMsg::input(&InputEvent::Key {
        vk: 0x41,
        down: true,
        modifiers: 0,
    })
    .unwrap();
    assert_eq!(key[0].route(), (channel::KEYBOARD, true));
    let pad = ControlMsg::input(&InputEvent::Gamepad {
        index: 3,
        active_mask: 8,
        buttons: 0,
        lt: 0,
        rt: 0,
        lx: 0,
        ly: 0,
        rx: 0,
        ry: 0,
    })
    .unwrap();
    assert_eq!(pad[0].route(), (0x13, true));
    let m = ControlMsg::input(&InputEvent::MouseButton {
        button: 1,
        down: true,
    })
    .unwrap();
    assert_eq!(m[0].route(), (channel::MOUSE, true));
    assert_eq!(ControlMsg::IdrRequest.route(), (channel::URGENT, true));
    assert_eq!(ControlMsg::Ping.route(), (channel::GENERIC, true));
    // Sunshine strips type+len and passes the NV_INPUT packet through unchanged.
    let mut c = Control::new(KEY);
    let mut h = Control::host(KEY);
    let ControlMsg::Input { packet, .. } = &m[0] else {
        unreachable!()
    };
    match h.decode(&c.encode(&m[0]).unwrap()).unwrap() {
        ControlMsg::Input { packet: p, .. } => assert_eq!(&p, packet),
        o => panic!("{o:?}"),
    }
}

#[test]
fn rtsp_setup_control_fixture_gives_port_and_connect_data() {
    let raw = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/rtsp/014-rtsp-setup-control-resp.bin"
    ))
    .unwrap();
    let r = brisk_proto::rtsp::parse_response(&raw).unwrap();
    assert_eq!(r.server_port(), Some(47999));
    // Decimal u32 (rtsp.cpp std::to_string(control_connect_data)); ENet CONNECT `data`.
    let cd: u32 = r.header("X-SS-Connect-Data").unwrap().parse().unwrap();
    assert_eq!(cd, 1396329308);
    let (_, d) = brisk_proto::enet::EnetPeer::connect(cd, 1, 0);
    assert_eq!(d[48..52], cd.to_be_bytes());
}

/// Fuzz find (fuzz/ target `input`): `GAMEPAD_BASE + index` overflowed u8 (a debug panic) before
/// encode_input could reject the index.
#[test]
fn control_input_gamepad_index_out_of_range_errors_not_panics() {
    let ev = InputEvent::Gamepad {
        index: 0xf0,
        active_mask: 0,
        buttons: 0,
        lt: 0,
        rt: 0,
        lx: 0,
        ly: 0,
        rx: 0,
        ry: 0,
    };
    assert_eq!(
        ControlMsg::input(&ev),
        Err(brisk_proto::input::InputError::GamepadIndex(0xf0))
    );
}
