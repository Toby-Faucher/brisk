//! Audio forward: the recorded Sunshine audio datagrams (fixtures/audio: AES-CBC data packets and
//! RS 4+2 parity) through [`AudioRx`] and out as WebTransport datagrams to a quinn client playing
//! the browser: `{seq u16, ts_48k u32}` little-endian, then the Opus packet as Sunshine encoded it.

mod common;

use std::time::Duration;

use brisk_bridge::session::{audio_datagram, send_audio, AudioRx, AudioStats, AUDIO_HEADER_LEN};
use brisk_bridge::wt::AUDIO_SEND_BUFFER;
use brisk_proto::audio::{parse_audio_packet, OpusFrame, PT_FEC};
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
        got.extend(rx.push(d));
    }
    assert_eq!(rx.stats.errors, 0);
    assert_eq!(rx.stats.packets, want.len() as u64);
    assert_eq!(rx.stats.recovered, 1, "the erased packet came back by FEC");
    let mut by_seq = got.clone();
    by_seq.sort_by_key(|f| f.seq);
    assert_eq!(
        by_seq, want,
        "every Opus packet, decrypted, bytes unchanged"
    );

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
