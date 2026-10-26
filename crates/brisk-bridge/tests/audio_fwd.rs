//! Audio forward: the recorded Sunshine audio datagrams (fixtures/audio: AES-CBC data packets and
//! RS 4+2 parity) through [`AudioRx`] and out as WebTransport datagrams to a quinn client playing
//! the browser: `{seq u16, ts_48k u32}` little-endian, then the Opus packet as Sunshine encoded it.

mod common;

use std::time::Duration;

use brisk_bridge::session::{
    AUDIO_HEADER_LEN, AudioRx, AudioStats, REORDER_HOLD_NS, audio_datagram, send_audio,
};
use brisk_bridge::wt::AUDIO_SEND_BUFFER;
use brisk_proto::audio::{OpusFrame, PT_AUDIO, PT_FEC, encode_parity, parse_audio_packet};
use brisk_proto::sdp::SS_ENC_AUDIO;
use common::*;
use tokio::time::timeout;

/// Recorded datagrams in arrival order, and the session's rikey / rikeyid.
fn fixtures() -> (Vec<Vec<u8>>, [u8; 16], u32) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/audio");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect();
    files.sort();
    let meta = std::fs::read_to_string(dir.join("rikey.hex")).unwrap();
    let (k, id) = meta.trim().split_once(' ').unwrap();
    let key: [u8; 16] = (0..16)
        .map(|i| u8::from_str_radix(&k[2 * i..2 * i + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let raw = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
    (raw, key, u32::from_str_radix(id, 16).unwrap())
}

fn is_fec(d: &[u8]) -> bool {
    d[1] & 0x7f == PT_FEC
}

/// The browser's view of one datagram: (seq, ts_48k, opus). Quarter stream id 0 first.
fn parse(d: &[u8]) -> (u16, u32, Vec<u8>) {
    assert_eq!(d[0], 0, "HTTP datagram quarter stream id of session 0");
    let d = &d[1..];
    (
        u16::from_le_bytes([d[0], d[1]]),
        u32::from_le_bytes(d[2..6].try_into().unwrap()),
        d[AUDIO_HEADER_LEN..].to_vec(),
    )
}

#[tokio::test]
async fn forwards_decrypted_opus_unchanged() {
    let (raw, key, id) = fixtures();
    let want: Vec<OpusFrame> = raw
        .iter()
        .filter(|d| !is_fec(d))
        .map(|d| parse_audio_packet(d, &key, id).unwrap())
        .collect();
    assert!(want.len() >= 40, "only {} data packets", want.len());
    // Erase one data packet whose block's parity was recorded: it must come out by FEC, intact.
    let erased = want
        .iter()
        .map(|f| f.seq)
        .find(|&s| {
            let base = s - s % 4;
            s % 4 == 1
                && raw
                    .iter()
                    .filter(|d| is_fec(d) && u16::from_be_bytes([d[14], d[15]]) == base)
                    .count()
                    > 0
        })
        .expect("a block with its parity in the recording");
    let mut rx = AudioRx::new(SS_ENC_AUDIO, key, id);
    let mut got: Vec<OpusFrame> = Vec::new();
    for d in &raw {
        if !is_fec(d) && u16::from_be_bytes([d[2], d[3]]) == erased {
            continue;
        }
        got.extend(rx.push(d, 0));
    }
    assert_eq!(rx.stats.errors, 0);
    assert_eq!(rx.stats.packets, want.len() as u64);
    assert_eq!(rx.stats.recovered, 1, "the erased packet came back by FEC");
    // Not sorted: the recovered packet must come out in its place, in strict seq order.
    assert_eq!(
        got.iter().map(|f| f.seq).collect::<Vec<_>>(),
        want.iter().map(|f| f.seq).collect::<Vec<_>>(),
        "seq order"
    );
    assert_eq!(got, want, "every Opus packet, decrypted, bytes unchanged");

    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let mut st = AudioStats::default();
    for f in &got {
        send_audio(&s, f, &mut st);
        let d = timeout(SECS2, c.read_datagram()).await.unwrap().unwrap();
        let (seq, ts48, opus) = parse(&d);
        assert_eq!(seq, f.seq);
        assert_eq!(
            ts48,
            f.ts.wrapping_mul(48),
            "Sunshine's RTP audio clock is ms"
        );
        assert_eq!(opus, f.data);
    }
    assert_eq!(st.sent, got.len() as u64);
    assert_eq!((st.evicted, st.too_large), (0, 0));
}

#[tokio::test]
async fn datagram_fits_mtu() {
    let (raw, key, id) = fixtures();
    let biggest = raw
        .iter()
        .filter(|d| !is_fec(d))
        .map(|d| parse_audio_packet(d, &key, id).unwrap().data.len())
        .max()
        .unwrap();
    // Sunshine's largest stereo bitrate is 512 kbit/s CBR: 320 bytes per 5 ms packet.
    assert!(biggest >= 300, "fixture Opus packets are {biggest} bytes");
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    // Right after the handshake the path MTU is quinn's initial 1200 (the QUIC minimum).
    let max = s
        .max_datagram_payload()
        .expect("the client allows datagrams");
    assert!(
        max >= AUDIO_HEADER_LEN + biggest,
        "max datagram payload {max} < {}",
        AUDIO_HEADER_LEN + biggest
    );
    // Sunshine's MAX_AUDIO_PACKET_SIZE (1400) does not fit: counted and skipped, not fatal.
    let mut st = AudioStats::default();
    let mk = |seq, n| OpusFrame {
        seq,
        ts: 5 * seq as u32,
        data: vec![0x5a; n],
    };
    send_audio(&s, &mk(1, 1400), &mut st);
    send_audio(&s, &mk(2, max - AUDIO_HEADER_LEN), &mut st);
    assert_eq!((st.too_large, st.sent), (1, 1));
    let d = timeout(SECS2, c.read_datagram()).await.unwrap().unwrap();
    assert_eq!(parse(&d).0, 2);
    assert_eq!(d.len(), 1 + max);
}

#[tokio::test]
async fn full_send_buffer_drops_oldest() {
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let mut st = AudioStats::default();
    let n = 50u16;
    // No await in between: the connection cannot send, so the bounded send buffer fills.
    for seq in 0..n {
        let f = OpusFrame {
            seq,
            ts: 5 * seq as u32,
            data: vec![seq as u8; 320],
        };
        send_audio(&s, &f, &mut st);
    }
    let mut seqs = Vec::new();
    while let Ok(Ok(d)) = timeout(Duration::from_millis(300), c.read_datagram()).await {
        seqs.push(parse(&d).0);
    }
    // 4 KiB holds 11 of these (quinn counts 32 bytes of overhead per datagram).
    let fits = AUDIO_SEND_BUFFER / (1 + AUDIO_HEADER_LEN + 320 + 32);
    assert_eq!(fits, 11);
    assert_eq!(
        seqs,
        (n - fits as u16..n).collect::<Vec<_>>(),
        "the newest survive"
    );
    assert_eq!(st.sent, n as u64);
    assert_eq!(
        st.evicted,
        (n as usize - fits) as u64,
        "one oldest per send"
    );
}

#[test]
fn header_is_seq_then_ts48k_little_endian() {
    let f = OpusFrame {
        seq: 0x0102,
        ts: 0x0100_0005,
        data: vec![9, 8, 7],
    };
    let d = audio_datagram(&f);
    let ts48 = 0x0100_0005u32.wrapping_mul(48).to_le_bytes();
    assert_eq!(&d[..2], &[0x02, 0x01]);
    assert_eq!(&d[2..6], &ts48);
    assert_eq!(&d[6..], &[9, 8, 7]);
}

#[tokio::test]
async fn a_send_that_just_fits_evicts_nothing() {
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let mut st = AudioStats::default();
    // 1 + 6 + 473 bytes + 32 of quinn overhead = 512: exactly 8 fill the 4 KiB buffer.
    let opus = 512 - 32 - 1 - AUDIO_HEADER_LEN;
    assert_eq!(AUDIO_SEND_BUFFER, 8 * 512);
    for seq in 0..9u16 {
        let f = OpusFrame {
            seq,
            ts: 0,
            data: vec![0; opus],
        };
        send_audio(&s, &f, &mut st);
        assert_eq!(st.evicted, (seq == 8) as u64, "after seq {seq}");
    }
    let mut seqs = Vec::new();
    while let Ok(Ok(d)) = timeout(Duration::from_millis(300), c.read_datagram()).await {
        seqs.push(parse(&d).0);
    }
    assert_eq!(seqs, (1..9).collect::<Vec<_>>());
}

// Synthetic plain (unencrypted) blocks: 4 data packets then 2 parity packets, as Sunshine sends
// them; the payload of seq s is 40 bytes of (s as u8).
const MS: u64 = 1_000_000;

fn rtp(pt: u8, seq: u16, ts: u32) -> Vec<u8> {
    let mut h = vec![0x80, pt];
    h.extend_from_slice(&seq.to_be_bytes());
    h.extend_from_slice(&ts.to_be_bytes());
    h.extend_from_slice(&[0; 4]);
    h
}

/// [data base..base+3, fec 0, fec 1].
fn block(base: u16) -> Vec<Vec<u8>> {
    let mut shards: Vec<Vec<u8>> = (0..4)
        .map(|i| vec![base.wrapping_add(i) as u8; 40])
        .chain([vec![0; 40], vec![0; 40]])
        .collect();
    encode_parity(&mut shards).unwrap();
    let ts = |s: u16| s as u32 * 5;
    let mut out: Vec<Vec<u8>> = (0..4u16)
        .map(|i| {
            let s = base.wrapping_add(i);
            let mut d = rtp(PT_AUDIO, s, ts(s));
            d.extend_from_slice(&shards[i as usize]);
            d
        })
        .collect();
    for k in 0..2u16 {
        let mut d = rtp(PT_FEC, base.wrapping_add(4 + k), ts(base));
        d.extend_from_slice(&[k as u8, PT_AUDIO]);
        d.extend_from_slice(&base.to_be_bytes());
        d.extend_from_slice(&ts(base).to_be_bytes());
        d.extend_from_slice(&[0; 4]);
        d.extend_from_slice(&shards[4 + k as usize]);
        out.push(d);
    }
    out
}

fn seqs(fs: &[OpusFrame]) -> Vec<u16> {
    fs.iter().map(|f| f.seq).collect()
}

#[tokio::test]
async fn a_recovered_packet_reaches_the_browser_in_its_place() {
    let b = block(8);
    let mut rx = AudioRx::new(0, [0; 16], 0);
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let mut st = AudioStats::default();
    // Slot 1 (seq 9) is lost; its parity comes after slots 2 and 3, as Sunshine sends it.
    let mut t = 0;
    for d in [&b[0], &b[2], &b[3], &b[4]] {
        t += MS;
        for f in rx.push(d, t) {
            send_audio(&s, &f, &mut st);
        }
    }
    let mut got = Vec::new();
    for _ in 0..4 {
        let d = timeout(SECS2, c.read_datagram()).await.unwrap().unwrap();
        let (seq, _, opus) = parse(&d);
        assert_eq!(opus, vec![seq as u8; 40]);
        got.push(seq);
    }
    assert_eq!(
        got,
        vec![8, 9, 10, 11],
        "strict seq order, 9 rebuilt in place"
    );
    assert_eq!(rx.stats.recovered, 1);
    assert_eq!(rx.stats.reordered, 2, "10 and 11 waited for 9");
    assert_eq!((rx.stats.skipped, rx.stats.lost), (0, 0));
    assert_eq!(rx.deadline(), None);
}

#[test]
fn without_loss_every_packet_goes_out_at_once() {
    let mut rx = AudioRx::new(0, [0; 16], 0);
    for base in (0..64u16).step_by(4) {
        let b = block(base);
        for (i, d) in b.iter().enumerate() {
            let out = rx.push(d, base as u64 * MS);
            let want: Vec<u16> = if i < 4 { vec![base + i as u16] } else { vec![] };
            assert_eq!(seqs(&out), want, "the push that brought it");
            assert_eq!(rx.deadline(), None, "nothing held");
        }
    }
    assert_eq!(rx.stats.reordered, 0);
}

#[test]
fn reorder_wraps_at_65535() {
    let mut rx = AudioRx::new(0, [0; 16], 0);
    let b = block(65532);
    let mut out = Vec::new();
    for d in [&b[0], &b[2], &b[3], &b[4]] {
        out.extend(rx.push(d, 0));
    }
    assert_eq!(seqs(&out), vec![65532, 65533, 65534, 65535]);
    let n = block(0);
    assert_eq!(seqs(&rx.push(&n[0], 0)), vec![0], "0 follows 65535");
    assert_eq!(rx.stats.skipped, 0);
}

#[test]
fn unrecoverable_loss_is_flushed_in_order_after_the_timeout() {
    let mut rx = AudioRx::new(0, [0; 16], 0);
    let b = block(4);
    // Slots 1 and 2 lost and only one parity packet arrives: 3 of 6 shards, beyond FEC.
    assert_eq!(seqs(&rx.push(&b[0], 0)), vec![4]);
    assert!(rx.push(&b[3], MS).is_empty(), "7 waits for 5 and 6");
    assert!(rx.push(&b[4], 2 * MS).is_empty());
    let dl = rx.deadline().expect("holding");
    assert_eq!(dl, MS + REORDER_HOLD_NS);
    assert!(rx.poll(dl - 1).is_empty(), "not before the deadline");
    assert_eq!(seqs(&rx.poll(dl)), vec![7]);
    assert_eq!(rx.stats.skipped, 2, "5 and 6 given up, once");
    assert_eq!(rx.deadline(), None);
    assert!(rx.poll(dl + 10 * MS).is_empty());
    assert_eq!(rx.stats.skipped, 2);
    // A late rebuild of 5 or 6 can no longer be played: dropped, not sent out of order.
    let n = block(8);
    assert_eq!(seqs(&rx.push(&n[0], dl + MS)), vec![8]);
}

#[test]
fn holding_never_spans_more_than_one_block() {
    let mut rx = AudioRx::new(0, [0; 16], 0);
    let b = block(0);
    let n = block(4);
    assert_eq!(seqs(&rx.push(&b[0], 0)), vec![0]);
    assert!(rx.push(&b[2], 0).is_empty());
    assert!(rx.push(&b[3], 0).is_empty()); // 1 missing, no parity yet
    // The next block's first packet: 1's parity was sent before it, so give up at once.
    assert_eq!(seqs(&rx.push(&n[0], 0)), vec![2, 3, 4]);
    assert_eq!(rx.stats.skipped, 1);
    assert_eq!(rx.deadline(), None);
    // A late parity for block 0 rebuilds 1 now: too late, dropped and counted, not sent.
    assert!(rx.push(&b[4], 0).is_empty());
    assert_eq!(rx.stats.late, 1);
}

#[test]
fn a_host_restart_far_behind_resyncs() {
    let mut rx = AudioRx::new(0, [0; 16], 0);
    for d in &block(4000)[..4] {
        rx.push(d, 0);
    }
    // AudioFec resyncs after 8 far packets; the reorder must follow, not call them late.
    let mut out = Vec::new();
    for base in [0u16, 4, 8] {
        for d in &block(base)[..4] {
            out.extend(rx.push(d, 0));
        }
    }
    assert!(seqs(&out).ends_with(&[8, 9, 10, 11]), "{:?}", seqs(&out));
}

#[test]
fn a_late_rebuild_across_the_wrap_is_late() {
    let mut rx = AudioRx::new(0, [0; 16], 0);
    let b = block(65532);
    for d in &b[..3] {
        rx.push(d, 0);
    }
    // 65535 lost; block 0 starts before 65532's parity arrives (reordered on the way).
    assert_eq!(seqs(&rx.push(&block(0)[0], 0)), vec![0]);
    assert!(rx.push(&b[4], 0).is_empty(), "65535 rebuilt too late");
    assert_eq!(rx.stats.late, 1);
    assert_eq!(rx.deadline(), None);
}

#[test]
fn the_deadline_runs_from_the_first_held_packet() {
    let mut rx = AudioRx::new(0, [0; 16], 0);
    let b = block(0);
    rx.push(&b[0], 0);
    rx.push(&b[2], MS);
    rx.push(&b[3], 3 * MS);
    assert_eq!(rx.deadline(), Some(MS + REORDER_HOLD_NS));
}

#[test]
fn giving_up_on_a_block_still_waits_for_a_gap_in_the_next() {
    let mut rx = AudioRx::new(0, [0; 16], 0);
    let (b, n) = (block(0), block(4));
    for d in [&b[0], &b[2], &b[3]] {
        rx.push(d, 0); // 1 missing, its parity lost
    }
    // 4 is missing too: 5 gives up on 1, then waits for 4.
    assert_eq!(seqs(&rx.push(&n[1], 0)), vec![2, 3]);
    assert!(rx.push(&n[2], 0).is_empty());
    assert!(rx.push(&n[3], 0).is_empty());
    assert_eq!(
        seqs(&rx.push(&n[4], 0)),
        vec![4, 5, 6, 7],
        "4 rebuilt in place"
    );
    assert_eq!(rx.stats.skipped, 1);
}

#[test]
fn a_start_mid_block_waits_for_the_rebuild_of_its_first_slots() {
    // The bridge's AudioRx starts when the tap connects, mid-stream: slots 0-1 of this block went
    // by before it existed, and parity rebuilds them. They must come first, not be called late.
    let mut rx = AudioRx::new(0, [0; 16], 0);
    let b = block(20);
    assert!(rx.push(&b[2], 0).is_empty());
    assert!(rx.push(&b[3], 0).is_empty());
    assert!(rx.push(&b[4], 0).is_empty(), "one parity: 3 of 6 shards");
    assert_eq!(seqs(&rx.push(&b[5], 0)), vec![20, 21, 22, 23]);
    assert_eq!((rx.stats.late, rx.stats.skipped), (0, 0));
}
