//! Control stream: ENet datagrams from the host into a client peer, and Gen7Enc control envelopes.
#![no_main]
#[path = "common.rs"]
mod common;

use brisk_proto::control::{Control, ControlMsg};
use brisk_proto::enet::{EnetEvent, EnetPeer, MAX_IN_FLIGHT};
use libfuzzer_sys::fuzz_target;

const CONNECT_ID: u32 = 0x1122_3344;

/// VERIFY_CONNECT from the host for our CONNECT (peer id 0, session 1, sent time 5).
fn verify() -> Vec<u8> {
    let mut d = vec![0x90, 0x00, 0, 5, 0x83, 0xFF, 0, 1, 0, 7, 1, 2];
    for v in [900u32, 65536, 0x30, 0, 0, 5000, 2, 2] {
        d.extend_from_slice(&v.to_be_bytes());
    }
    d.extend_from_slice(&CONNECT_ID.to_ne_bytes());
    d
}

fuzz_target!(|d: &[u8]| {
    let (mut raw, _) = EnetPeer::connect(1, CONNECT_ID, 0);
    let (mut up, _) = EnetPeer::connect(1, CONNECT_ID, 0);
    assert_eq!(
        up.handle_datagram(&verify(), 0),
        Ok(vec![EnetEvent::Connected])
    );
    let mut ctl = Control::new(common::KEY);
    let mut now = 0u32;
    for c in common::chunks(d) {
        now = now.wrapping_add(c.first().copied().unwrap_or(0) as u32 * 40);
        for p in [&mut raw, &mut up] {
            if let Ok(evs) = p.handle_datagram(c, now) {
                for e in evs {
                    if let EnetEvent::Receive { data, .. } = e {
                        let _ = ctl.decode(&data);
                    }
                }
            }
            let _ = p.poll_timeout(now);
            let _ = p.send(0, true, c.get(..64).unwrap_or(c), now);
            assert!(p.in_flight() <= MAX_IN_FLIGHT);
        }
        let _ = ctl.decode(c);
        // The same bytes as a sealed host message: reaches ControlMsg::parse.
        if c.len() >= 2 {
            let t = u16::from_le_bytes([c[0], c[1]]);
            let mut host = Control::host(common::KEY);
            if let Ok(f) = host.encode(&ControlMsg::Unknown(t, c[2..].to_vec())) {
                let m = Control::new(common::KEY)
                    .decode(&f)
                    .expect("own message opens");
                let _ = (m.kind(), m.route());
            }
        }
    }
});
