//! Audio datagrams (data + FEC) through AudioFec, the CBC opener and the bridge's AudioRx reorder.
#![no_main]
#[path = "common.rs"]
mod common;

use brisk_bridge::session::AudioRx;
use brisk_proto::audio::{AudioCipher, AudioFec, MAX_DATAGRAM, parse_audio_payload};
use brisk_proto::sdp::SS_ENC_AUDIO;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|d: &[u8]| {
    let cipher = AudioCipher::new(&common::KEY, 7);
    let mut fec = AudioFec::new(5);
    let mut rx = [
        AudioRx::new(0, common::KEY, 7),
        AudioRx::new(SS_ENC_AUDIO, common::KEY, 7),
    ];
    let mut now = 0u64;
    for c in common::chunks(d) {
        now += c.first().copied().unwrap_or(0) as u64 * 1_000_000;
        if let Ok(p) = parse_audio_payload(c) {
            let _ = cipher.open(&p);
        }
        if let Ok(ps) = fec.push(c) {
            assert!(ps.len() <= 4);
            for p in ps {
                assert!(p.payload.len() <= MAX_DATAGRAM);
            }
        }
        for r in &mut rx {
            let out = r.push(c, now);
            assert!(out.len() <= 8);
            let _ = r.poll(now);
        }
    }
});
