//! Property tests for AudioRx (Sunshine audio datagrams to Opus packets in seq order).

use brisk_bridge::session::AudioRx;
use brisk_proto::audio::encode_parity;
use proptest::prelude::*;

const KEY: [u8; 16] = *b"0123456789abcdef";

fn rtp(pt: u8, seq: u16, body: &[u8]) -> Vec<u8> {
    let mut v = vec![0x80, pt];
    v.extend(seq.to_be_bytes());
    v.extend([0; 8]);
    v.extend(body);
    v
}

/// One 4+2 block as Sunshine sends it: 4 data datagrams, then 2 FEC datagrams.
fn block(base: u16, len: usize) -> Vec<Vec<u8>> {
    let mut shards: Vec<Vec<u8>> = (0..4u16)
        .map(|i| {
            (0..len)
                .map(|j| (j as u16 ^ base.wrapping_add(i)) as u8)
                .collect()
        })
        .collect();
    shards.extend([vec![0; len], vec![0; len]]);
    encode_parity(&mut shards).unwrap();
    let mut d: Vec<Vec<u8>> = (0..4u16)
        .map(|i| rtp(97, base.wrapping_add(i), &shards[i as usize]))
        .collect();
    for x in 0..2u16 {
        let mut body = vec![x as u8, 97];
        body.extend(base.to_be_bytes());
        body.extend([0; 8]);
        body.extend(&shards[4 + x as usize]);
        d.push(rtp(127, base.wrapping_add(4 + x), &body));
    }
    d
}

proptest! {
    /// Any datagrams at any times, plain or encrypted session: no panic, bounded output.
    #[test]
    fn audio_rx_takes_any_datagrams(
        ds in prop::collection::vec((prop::sample::select(vec![97u8, 127, 5]), any::<u16>(), prop::collection::vec(any::<u8>(), 0..40), 0u64..30_000_000), 0..80),
        enc in any::<bool>(),
    ) {
        let mut rx = AudioRx::new(if enc { brisk_proto::sdp::SS_ENC_AUDIO } else { 0 }, KEY, 7);
        let mut now = 0;
        for (pt, seq, body, dt) in ds {
            now += dt;
            prop_assert!(rx.push(&rtp(pt, seq, &body), now).len() <= 8);
            if rx.deadline().is_some_and(|t| now >= t) {
                let _ = rx.poll(now);
            }
        }
    }

    /// Blocks in order with any 2 datagrams of each lost: every packet comes out once, in order.
    #[test]
    fn audio_rx_is_gapless_and_ordered_when_fec_can_recover(
        start in any::<u16>(), nblocks in 1usize..8, len in 1usize..48,
        loss in prop::collection::vec(prop::sample::subsequence((0..6usize).collect::<Vec<_>>(), 0..=2), 8),
    ) {
        let base0 = start & !3;
        let mut rx = AudioRx::new(0, KEY, 7);
        let mut out = Vec::new();
        for (b, lost) in loss.iter().take(nblocks).enumerate() {
            let base = base0.wrapping_add(4 * b as u16);
            for (i, d) in block(base, len).iter().enumerate() {
                if !lost.contains(&i) {
                    out.extend(rx.push(d, b as u64 * 20_000_000).into_iter().map(|f| f.seq));
                }
            }
        }
        out.extend(rx.poll(u64::MAX / 2).into_iter().map(|f| f.seq));
        let want: Vec<u16> = (0..4 * nblocks as u16).map(|i| base0.wrapping_add(i)).collect();
        prop_assert_eq!(out, want);
        prop_assert_eq!(rx.stats.skipped, 0);
    }
}
