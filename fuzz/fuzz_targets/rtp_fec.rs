//! Video datagrams (plain and SS_ENC_VIDEO) into the frame assembler, plus raw Reed-Solomon decode.
#![no_main]
#[path = "common.rs"]
mod common;

use brisk_proto::fec::{rs_decode, rs_encode};
use brisk_proto::rtp::{FrameAssembler, MAX_VIDEO_PACKET, VideoCipher, parse_video_packet};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|d: &[u8]| {
    let cipher = VideoCipher::new(&common::KEY);
    let mut asm = FrameAssembler::new();
    for c in common::chunks(d) {
        let _ = cipher.open(c);
        if let Ok(p) = parse_video_packet(c, 0) {
            assert!(p.payload.len() <= MAX_VIDEO_PACKET);
            for f in asm.push(p) {
                // At most 4 blocks; a block with no parity shards may hold up to 1023 data shards
                // (the header field is 10 bits and rtp.rs caps the total only when parity exists), one packet each.
                assert!(f.annexb.len() <= 4 * 1023 * MAX_VIDEO_PACKET);
            }
        }
    }
    let _ = asm.take_dropped();

    // Reed-Solomon on fuzz-chosen shapes: ds, ps, shard size, loss bitmap, then shard bytes.
    if d.len() >= 4 {
        let (ds, ps, bs) = (
            d[0] as usize % 40,
            d[1] as usize % 12,
            d[2] as usize % 64 + 1,
        );
        let loss = d[3] as u64 | (u64::from(d.get(4).copied().unwrap_or(0)) << 8);
        let mut src = d[4..].iter().copied().cycle();
        let mut shards: Vec<Vec<u8>> = (0..ds + ps)
            .map(|_| (0..bs).map(|_| src.next().unwrap_or(0)).collect())
            .collect();
        if rs_encode(ds, ps, &mut shards).is_ok() {
            let mut have: Vec<Option<Vec<u8>>> = shards.iter().cloned().map(Some).collect();
            for (i, s) in have.iter_mut().enumerate() {
                if i < 16 && loss & (1 << i) != 0 {
                    *s = None;
                }
            }
            let lost = have.iter().filter(|s| s.is_none()).count();
            match rs_decode(ds, ps, &mut have) {
                Ok(()) => {
                    for i in 0..ds {
                        assert_eq!(have[i].as_ref(), Some(&shards[i]), "wrong recovery");
                    }
                }
                Err(_) => assert!(lost > ps, "recoverable loss pattern failed"),
            }
        }
    }
});
