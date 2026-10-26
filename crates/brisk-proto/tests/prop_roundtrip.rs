//! Property tests: encode -> parse round trips, Reed-Solomon recovery under random loss, and the
//! AES wrappers (round trip; wrong key, tag or sequence fails cleanly). See prop_parsers.rs for
//! the no-panic/heap-budget properties. `PROPTEST_CASES=5000` for a longer run.

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use aes_gcm::aead::AeadInPlace;
use aes_gcm::{Aes128Gcm, Nonce};
use brisk_proto::audio::{AudioCipher, AudioFec, AudioPayload, PARITY, encode_parity};
use brisk_proto::control::{Control, ControlError, ControlMsg};
use brisk_proto::enet::{EnetEvent, EnetPeer};
use brisk_proto::fec::{FecError, rs_decode, rs_decode_matrix, rs_encode};
use brisk_proto::input::{InputEvent, MAX_MOUSE_DELTA, encode_input};
use brisk_proto::rtp::{FLAG_CONTAINS_PIC_DATA, FLAG_EOF, FLAG_SOF, FrameAssembler, RtpError};
use brisk_proto::rtsp::{RtspCipher, RtspError, parse_request, request};
use proptest::prelude::*;

const KEY: [u8; 16] = *b"0123456789abcdef";

fn bytes(r: std::ops::Range<usize>) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), r)
}

/// Deterministic pick of `k` distinct indices below `n` from `seed`.
fn pick(seed: u64, n: usize, k: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..n).collect();
    let mut s = seed | 1;
    for i in (1..n).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        idx.swap(i, (s % (i as u64 + 1)) as usize);
    }
    idx.truncate(k);
    idx
}

// ---- Reed-Solomon ------------------------------------------------------------------------

fn rs_case() -> impl Strategy<Value = (usize, usize, Vec<Vec<u8>>, u64)> {
    (1usize..=24, 1usize..=8, 1usize..=48).prop_flat_map(|(ds, ps, bs)| {
        (
            Just(ds),
            Just(ps),
            prop::collection::vec(bytes(bs..bs + 1), ds),
            any::<u64>(),
        )
    })
}

fn encoded(ds: usize, ps: usize, data: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut shards = data.to_vec();
    shards.extend((0..ps).map(|_| vec![0; data[0].len()]));
    rs_encode(ds, ps, &mut shards).unwrap();
    shards
}

proptest! {
    /// Any loss pattern of at most `ps` shards (data or parity) restores every data shard.
    #[test]
    fn rs_recovers_any_loss_up_to_the_parity_count(
        (ds, ps, data, seed) in rs_case(),
        k in 0usize..=8,
    ) {
        let shards = encoded(ds, ps, &data);
        let mut have: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
        for i in pick(seed, ds + ps, k.min(ps)) {
            have[i] = None;
        }
        rs_decode(ds, ps, &mut have).unwrap();
        for i in 0..ds {
            prop_assert_eq!(have[i].as_ref(), Some(&data[i]));
        }
    }

    /// More losses than parity: `TooManyLosses`, and the shards are left as they were.
    #[test]
    fn rs_reports_too_many_losses_and_changes_nothing(
        (ds, ps, data, seed) in rs_case(),
        extra in 1usize..=8,
    ) {
        let shards = encoded(ds, ps, &data);
        let mut have: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
        for i in pick(seed, ds + ps, (ps + extra).min(ds + ps)) {
            have[i] = None;
        }
        let before = have.clone();
        prop_assert_eq!(rs_decode(ds, ps, &mut have), Err(FecError::TooManyLosses));
        prop_assert_eq!(have, before);
    }

    /// Sunshine's audio matrix: any 2 of the 6 shards lost are recovered, 3 are not.
    #[test]
    fn audio_parity_matrix_recovers_any_two(data in prop::collection::vec(bytes(32..33), 4), seed in any::<u64>(), k in 0usize..=3) {
        let mut shards = data.clone();
        shards.extend([vec![0; 32], vec![0; 32]]);
        encode_parity(&mut shards).unwrap();
        let mut have: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();
        for i in pick(seed, 6, k) {
            have[i] = None;
        }
        let r = rs_decode_matrix(4, 2, &PARITY, &mut have);
        if k <= 2 {
            prop_assert_eq!(r, Ok(()));
            for i in 0..4 {
                prop_assert_eq!(have[i].as_ref(), Some(&data[i]));
            }
        } else {
            prop_assert!(r.is_err());
        }
    }
}

// ---- Video: Sunshine-shaped frames through the assembler ---------------------------------

/// Packets of one frame as Sunshine's videoBroadcastThread + fec::encode build them (see
/// tests/rtp.rs `Host`): per FEC block, data shards then parity shards.
fn sunshine_frame(
    idx: u32,
    annexb: &[u8],
    packet_size: usize,
    pct: usize,
    blocks: usize,
) -> Vec<Vec<Vec<u8>>> {
    let blocksize = packet_size + 16;
    let per = blocksize - 32;
    let mut last_len = ((annexb.len() + 8) % (packet_size - 16)) as u16;
    if last_len == 0 {
        last_len = (packet_size - 16) as u16;
    }
    let mut body = vec![0x01, 0, 0, 2];
    body.extend(last_len.to_le_bytes());
    body.extend([0, 0]);
    body.extend(annexb);
    let chunks: Vec<&[u8]> = body.chunks(per).collect();
    let groups: Vec<&[&[u8]]> = chunks.chunks(chunks.len().div_ceil(blocks)).collect();
    let nblocks = groups.len();
    let mut seq = 0u16;
    let mut out = Vec::new();
    for (b, group) in groups.into_iter().enumerate() {
        let n = group.len();
        let mut shards: Vec<Vec<u8>> = group
            .iter()
            .enumerate()
            .map(|(x, c)| {
                let mut s = vec![0u8; blocksize];
                s[24] = FLAG_CONTAINS_PIC_DATA
                    | if x == 0 { FLAG_SOF } else { 0 }
                    | if x == n - 1 { FLAG_EOF } else { 0 };
                s[32..32 + c.len()].copy_from_slice(c);
                s
            })
            .collect();
        let parity = (n * pct).div_ceil(100);
        shards.extend((0..parity).map(|_| vec![0u8; blocksize]));
        rs_encode(n, parity, &mut shards).unwrap();
        for (x, s) in shards.iter_mut().enumerate() {
            s[0] = 0x90;
            s[2..4].copy_from_slice(&seq.wrapping_add(x as u16).to_be_bytes());
            s[20..24].copy_from_slice(&idx.to_le_bytes());
            s[27] = ((b << 4) | ((nblocks - 1) << 6)) as u8;
            s[28..32].copy_from_slice(&((x << 12 | n << 22 | pct << 4) as u32).to_le_bytes());
        }
        seq = seq.wrapping_add(shards.len() as u16);
        out.push(shards);
    }
    out
}

proptest! {
    /// A frame split over 1-4 blocks, with any loss of at most the parity count per block and the
    /// packets in any order, comes out byte-exact (and only once).
    #[test]
    fn assembler_rebuilds_frames_under_loss_and_reordering(
        annexb in bytes(1..1500),
        packet_size in 48usize..=160,
        pct in 10usize..=60,
        blocks in 1usize..=4,
        loss in any::<u64>(),
        order in any::<u64>(),
    ) {
        let mut pkts = Vec::new();
        for (b, block) in sunshine_frame(1, &annexb, packet_size, pct, blocks).into_iter().enumerate() {
            let n = block.len();
            let parity = block
                .iter()
                .filter(|s| brisk_proto::rtp::parse_video_packet(s, 0).unwrap().is_parity)
                .count();
            let lost = pick(loss.rotate_left(b as u32 * 8), n, parity);
            pkts.extend(block.into_iter().enumerate().filter(|(i, _)| !lost.contains(i)).map(|(_, p)| p));
        }
        let mut a = FrameAssembler::new();
        let mut frames = Vec::new();
        for i in pick(order, pkts.len(), pkts.len()) {
            frames.extend(a.push(brisk_proto::rtp::parse_video_packet(&pkts[i], 0).unwrap()));
        }
        prop_assert_eq!(frames.len(), 1);
        prop_assert_eq!(&frames[0].annexb, &annexb);
        prop_assert!(frames[0].is_idr);
        prop_assert_eq!(a.take_dropped(), 0);
    }

    /// SS_ENC_VIDEO: Sunshine's sealing opens to the same packet; a flipped bit or another key fails.
    #[test]
    fn video_cipher_round_trip_and_rejects_tampering(plain in bytes(33..1400), ctr in any::<u64>(), flip in any::<usize>(), key in any::<[u8; 16]>()) {
        let mut iv = [0u8; 12];
        iv[..8].copy_from_slice(&ctr.to_le_bytes());
        iv[11] = b'V';
        let mut ct = plain.clone();
        let tag = Aes128Gcm::new(&KEY.into())
            .encrypt_in_place_detached(Nonce::from_slice(&iv), &[], &mut ct)
            .unwrap();
        let mut wire = iv.to_vec();
        wire.extend(1u32.to_le_bytes());
        wire.extend(tag);
        wire.extend(ct);
        prop_assert_eq!(brisk_proto::rtp::VideoCipher::new(&KEY).open(&wire), Ok(plain));
        let mut bad = wire.clone();
        let at = flip % bad.len();
        // Bytes 12..16 (frameNumber) are not authenticated: Sunshine and moonlight ignore them.
        if !(12..16).contains(&at) {
            bad[at] ^= 1;
            prop_assert_eq!(brisk_proto::rtp::VideoCipher::new(&KEY).open(&bad), Err(RtpError::BadTag));
        }
        if key != KEY {
            prop_assert_eq!(brisk_proto::rtp::VideoCipher::new(&key).open(&wire), Err(RtpError::BadTag));
        }
    }
}

// ---- Audio --------------------------------------------------------------------------------

/// Sunshine's `encode_audio`: AES-128-CBC/PKCS7, IV = BE32(rikeyid + seq) then zeros.
fn cbc(key: &[u8; 16], key_id: u32, seq: u16, plain: &[u8]) -> Vec<u8> {
    let aes = Aes128::new(key.into());
    let mut data = plain.to_vec();
    let pad = 16 - plain.len() % 16;
    data.extend(std::iter::repeat_n(pad as u8, pad));
    let mut prev = [0u8; 16];
    prev[..4].copy_from_slice(&key_id.wrapping_add(seq as u32).to_be_bytes());
    for c in data.chunks_mut(16) {
        c.iter_mut().zip(prev).for_each(|(b, p)| *b ^= p);
        let mut blk = GenericArray::clone_from_slice(c);
        aes.encrypt_block(&mut blk);
        c.copy_from_slice(&blk);
        prev.copy_from_slice(c);
    }
    data
}

fn rtp(pt: u8, seq: u16, ts: u32, body: &[u8]) -> Vec<u8> {
    let mut v = vec![0x80, pt];
    v.extend(seq.to_be_bytes());
    v.extend(ts.to_be_bytes());
    v.extend([0; 4]);
    v.extend(body);
    v
}

proptest! {
    #[test]
    fn audio_cbc_round_trip_and_wrong_key(plain in bytes(1..1400), seq in any::<u16>(), id in any::<u32>(), key in any::<[u8; 16]>()) {
        let p = AudioPayload { seq, ts: 9, payload: cbc(&KEY, id, seq, &plain) };
        prop_assert_eq!(AudioCipher::new(&KEY, id).open(&p).map(|f| f.data), Ok(plain.clone()));
        if key != KEY {
            // CBC has no tag: a wrong key fails the padding check or yields other bytes, never panics.
            prop_assert_ne!(AudioCipher::new(&key, id).open(&p).map(|f| f.data), Ok(plain));
        }
    }

    /// Consecutive 4+2 blocks with any 2 datagrams of each lost: every data packet comes out once.
    #[test]
    fn audio_fec_recovers_two_losses_per_block(
        len in 1usize..=64,
        nblocks in 1usize..=6,
        start in any::<u16>(),
        loss in any::<u64>(),
        k in 0usize..=2,
    ) {
        let base0 = start & !3;
        let mut fec = AudioFec::new(5);
        let mut got = std::collections::BTreeMap::new();
        for b in 0..nblocks {
            let base = base0.wrapping_add(4 * b as u16);
            let mut shards: Vec<Vec<u8>> = (0..4u16)
                .map(|i| (0..len).map(|j| (j as u16 ^ base.wrapping_add(i)) as u8).collect())
                .collect();
            shards.extend([vec![0; len], vec![0; len]]);
            encode_parity(&mut shards).unwrap();
            let mut dgs: Vec<Vec<u8>> = (0..4u16)
                .map(|i| rtp(97, base.wrapping_add(i), u32::from(i) * 5, &shards[i as usize]))
                .collect();
            for x in 0..2u16 {
                let mut body = vec![x as u8, 97];
                body.extend(base.to_be_bytes());
                body.extend(0u32.to_be_bytes());
                body.extend([0; 4]);
                body.extend(&shards[4 + x as usize]);
                dgs.push(rtp(127, base.wrapping_add(4 + x), 0, &body));
            }
            let lost = pick(loss.rotate_left(b as u32 * 5), 6, k);
            for (i, d) in dgs.iter().enumerate().filter(|(i, _)| !lost.contains(i)) {
                let _ = i;
                for p in fec.push(d).unwrap() {
                    prop_assert!(got.insert(p.seq, p.payload).is_none(), "seq {} twice", p.seq);
                }
            }
            for i in 0..4u16 {
                let s = base.wrapping_add(i);
                prop_assert_eq!(got.get(&s), Some(&shards[i as usize]));
            }
        }
        prop_assert_eq!(got.len(), 4 * nblocks);
    }
}

// ---- Control ------------------------------------------------------------------------------

/// Every ControlMsg the host or client sends, with the form `decode` gives back.
fn control_msg() -> impl Strategy<Value = (ControlMsg, ControlMsg)> {
    use ControlMsg::*;
    let same = |m: ControlMsg| (m.clone(), m);
    prop_oneof![
        Just(same(StartA)),
        Just(same(StartB)),
        Just(same(Ping)),
        Just(same(IdrRequest)),
        (any::<u32>(), any::<u32>())
            .prop_map(move |(first, last)| same(InvalidateRefFrames { first, last })),
        bytes(0..200).prop_map(move |p| same(LossStats(p))),
        bytes(0..200).prop_map(move |p| same(FrameStats(p))),
        (any::<u8>(), bytes(0..200)).prop_map(|(channel, packet)| (
            Input {
                channel,
                packet: packet.clone()
            },
            Input { channel: 0, packet }
        )),
        (any::<u16>(), any::<u16>(), any::<u16>()).prop_map(move |(controller, low, high)| same(
            Rumble {
                controller,
                low,
                high
            }
        )),
        (any::<u16>(), any::<u16>(), any::<u16>()).prop_map(move |(controller, left, right)| same(
            RumbleTriggers {
                controller,
                left,
                right
            }
        )),
        any::<bool>().prop_map(move |enabled| same(HdrMode { enabled })),
        any::<u32>().prop_map(move |c| same(Termination(c))),
        (0x6000u16.., bytes(0..200)).prop_map(move |(t, p)| same(Unknown(t, p))),
    ]
}

proptest! {
    /// Sealed in order by one side, opened by the other: the same messages; a replay, a flipped
    /// bit or another key fails with the matching error.
    #[test]
    fn control_round_trip_both_directions(msgs in prop::collection::vec(control_msg(), 1..6), flip in any::<usize>(), key in any::<[u8; 16]>()) {
        let (mut host, mut client) = (Control::host(KEY), Control::new(KEY));
        let (mut c_tx, mut h_rx) = (Control::new(KEY), Control::host(KEY));
        let mut last = Vec::new();
        for (m, back) in &msgs {
            let f = host.encode(m).unwrap();
            prop_assert_eq!(&client.decode(&f).unwrap(), back);
            prop_assert_eq!(&h_rx.decode(&c_tx.encode(m).unwrap()).unwrap(), back);
            last = f;
        }
        prop_assert_eq!(client.decode(&last), Err(ControlError::Replay));
        let mut fresh = Control::new(KEY);
        let mut bad = last.clone();
        let at = 24 + flip % (bad.len() - 24); // tag or ciphertext
        bad[at] ^= 0x40;
        prop_assert_eq!(fresh.decode(&bad), Err(ControlError::BadTag));
        if key != KEY {
            prop_assert_eq!(Control::new(key).decode(&last), Err(ControlError::BadTag));
        }
    }
}

// ---- RTSP ---------------------------------------------------------------------------------

proptest! {
    #[test]
    fn rtsp_request_round_trip(
        method in "[A-Z]{1,10}",
        target in "[!-~]{1,40}",
        seq in any::<u32>(),
        headers in prop::collection::vec(("X-[A-Za-z0-9-]{1,15}", "[!-~]([ -~]{0,30}[!-~])?"), 0..6),
        body in bytes(0..300),
    ) {
        let mut hs: Vec<(String, String)> = headers;
        if !body.is_empty() {
            hs.push(("Content-length".into(), body.len().to_string()));
        }
        let refs: Vec<(&str, &str)> = hs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let r = parse_request(&request(&method, &target, seq, &refs, &body)).unwrap();
        prop_assert_eq!(&r.method, &method);
        prop_assert_eq!(&r.target, &target);
        let cseq = seq.to_string();
        prop_assert_eq!(r.header("CSeq"), Some(cseq.as_str()));
        prop_assert_eq!(&r.headers[2..], &hs[..]);
        prop_assert_eq!(r.body, body);
    }

    #[test]
    fn rtsp_cipher_round_trip_and_rejects(msgs in prop::collection::vec(bytes(1..600), 1..5), flip in any::<usize>(), key in any::<[u8; 16]>()) {
        let (mut host, mut client) = (RtspCipher::host(KEY), RtspCipher::client(KEY));
        let mut last = Vec::new();
        for m in &msgs {
            last = host.seal(m).unwrap();
            prop_assert_eq!(&client.open(&last).unwrap(), m);
            let mut back = RtspCipher::host(KEY);
            prop_assert_eq!(&back.open(&RtspCipher::client(KEY).seal(m).unwrap()).unwrap(), m);
        }
        prop_assert_eq!(client.open(&last), Err(RtspError::Replay));
        let mut bad = last.clone();
        let at = 8 + flip % (bad.len() - 8); // tag or ciphertext
        bad[at] ^= 1;
        prop_assert_eq!(RtspCipher::client(KEY).open(&bad), Err(RtspError::BadTag));
        if key != KEY {
            prop_assert_eq!(RtspCipher::client(key).open(&last), Err(RtspError::BadTag));
        }
    }
}

// ---- SDP, tap, HTTP XML, hex, ECB ---------------------------------------------------------

proptest! {
    /// Every number build_sdp writes reads back with sdp_attr (the DESCRIBE/ANNOUNCE reader).
    #[test]
    fn sdp_build_round_trips_through_sdp_attr(
        width in any::<u16>(), height in any::<u16>(), fps in any::<u16>(),
        bitrate_kbps in any::<u32>(), hevc in any::<bool>(), packet_size in any::<u16>(),
        enc_flags in any::<u32>(), tap in any::<bool>(),
    ) {
        use brisk_proto::sdp::*;
        let cfg = StreamConfig {
            width, height, fps, bitrate_kbps,
            codec: if hevc { Codec::Hevc } else { Codec::H264 },
            packet_size, fec_percent: 0, enc_flags, tap,
        };
        let s = build_sdp(&cfg, brisk_proto::version::SUNSHINE_APPVERSION);
        let a = |n: &str| sdp_attr(&s, n);
        prop_assert_eq!(a("x-nv-video[0].clientViewportWd"), Some(u32::from(width)));
        prop_assert_eq!(a("x-nv-video[0].clientViewportHt"), Some(u32::from(height)));
        prop_assert_eq!(a("x-nv-video[0].maxFPS"), Some(u32::from(fps)));
        prop_assert_eq!(a("x-nv-video[0].packetSize"), Some(u32::from(packet_size)));
        prop_assert_eq!(a("x-ml-video.configuredBitrateKbps"), Some(bitrate_kbps));
        prop_assert_eq!(a("x-ss-general.encryptionEnabled"), Some(enc_flags));
        prop_assert_eq!(a("x-nv-vqos[0].bitStreamFormat"), Some(u32::from(hevc)));
        prop_assert_eq!(a("x-brisk.tap"), tap.then_some(1));
    }

    #[test]
    fn tap_header_round_trip(
        flags in any::<u8>(), cd in any::<u32>(), codec in any::<u32>(), idx in any::<u64>(),
        cap in any::<u64>(), tap in any::<u64>(), dropped in any::<u32>(),
        extra in 0usize..64, payload in bytes(0..256),
    ) {
        let mut m = b"BTAP\x01".to_vec();
        m.push(flags);
        m.extend((44 + extra as u16).to_le_bytes());
        for v in [cd, codec] { m.extend(v.to_le_bytes()); }
        for v in [idx, cap, tap] { m.extend(v.to_le_bytes()); }
        m.extend(dropped.to_le_bytes());
        m.extend(std::iter::repeat_n(0xEE, extra));
        m.extend(&payload);
        let (h, p) = brisk_proto::tap::parse_tap(&m).unwrap();
        prop_assert_eq!(
            (h.flags, h.connect_data, h.codec, h.frame_index, h.capture_ns, h.tap_ns, h.dropped_total),
            (flags, cd, codec, idx, cap, tap, dropped)
        );
        prop_assert_eq!(p, &payload[..]);
    }

    /// serverinfo and applist with XML-special characters in every text field parse back exactly.
    #[test]
    fn serverinfo_and_applist_round_trip(
        texts in prop::collection::vec("[A-Za-z0-9<>&'\"._-]([A-Za-z0-9<>&'\" ._-]{0,18}[A-Za-z0-9<>&'\"._-])?", 4),
        port in any::<u16>(), paired in any::<bool>(), game in any::<u32>(), codecs in any::<u32>(),
        apps in prop::collection::vec((any::<u32>(), "[A-Za-z0-9<>&'\"._-]([A-Za-z0-9<>&'\" ._-]{0,18}[A-Za-z0-9<>&'\"._-])?"), 0..5),
    ) {
        use brisk_proto::http::*;
        let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let xml = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200"><appversion>{}</appversion><GfeVersion>{}</GfeVersion><uniqueid>{}</uniqueid><HttpsPort>{port}</HttpsPort><PairStatus>{}</PairStatus><currentgame>{game}</currentgame><ServerCodecModeSupport>{codecs}</ServerCodecModeSupport><state>{}</state></root>"#,
            esc(&texts[0]), esc(&texts[1]), esc(&texts[2]), u8::from(paired), esc(&texts[3]),
        );
        prop_assert_eq!(parse_serverinfo(&xml).unwrap(), ServerInfo {
            app_version: texts[0].clone(), gfe_version: texts[1].clone(), unique_id: texts[2].clone(),
            https_port: port, paired, current_game: game, codec_mode_support: codecs,
            server_state: texts[3].clone(),
        });
        let list: String = apps.iter().map(|(id, t)| format!("<App><IsHdrSupported>0</IsHdrSupported><AppTitle>{}</AppTitle><ID>{id}</ID></App>", esc(t))).collect();
        let got = parse_applist(&format!(r#"<root status_code="200">{list}</root>"#)).unwrap();
        let want: Vec<App> = apps.into_iter().map(|(id, title)| App { id, title }).collect();
        prop_assert_eq!(got, want);
    }

    #[test]
    fn hex_and_ecb_round_trip(data in bytes(0..128), key in any::<[u8; 16]>(), cut in 1usize..16) {
        use brisk_proto::pair::*;
        prop_assert_eq!(unhex(&hex(&data)).unwrap(), data.clone());
        prop_assert_eq!(unhex(&hex(&data).to_lowercase()).unwrap(), data.clone());
        let blocks = &data[..data.len() / 16 * 16];
        let aes = Aes128::new(&key.into());
        let mut ct = blocks.to_vec();
        ct.chunks_mut(16).for_each(|b| aes.encrypt_block(GenericArray::from_mut_slice(b)));
        prop_assert_eq!(aes_ecb_decrypt(&key, &ct).unwrap(), blocks.to_vec());
        let mut odd = ct.clone();
        odd.extend(std::iter::repeat_n(0, cut));
        prop_assert_eq!(aes_ecb_decrypt(&key, &odd), Err(PairError::BadLength));
    }
}

// ---- ENet and input -----------------------------------------------------------------------

fn verify() -> Vec<u8> {
    let mut d = vec![0x90, 0x00, 0, 1, 0x83, 0xFF, 0, 1, 0, 7, 1, 2];
    for v in [900u32, 65536, 0x30, 0, 0, 5000, 2, 2] {
        d.extend_from_slice(&v.to_be_bytes());
    }
    d.extend_from_slice(&9u32.to_ne_bytes());
    d
}

proptest! {
    /// Reliable commands arriving shuffled and duplicated (within ENet's reorder window) are
    /// delivered exactly once each, in sequence order, per channel.
    #[test]
    fn enet_reliable_delivery_is_in_order_and_exactly_once(
        n in 1u16..=60,
        channel in 0u8..0x30,
        order in any::<u64>(),
        dups in prop::collection::vec(any::<u16>(), 0..10),
    ) {
        let (mut p, _) = EnetPeer::connect(1, 9, 0);
        p.handle_datagram(&verify(), 0).unwrap();
        let mut seqs: Vec<u16> = pick(order, n as usize, n as usize).into_iter().map(|i| i as u16 + 1).collect();
        seqs.extend(dups.iter().map(|d| d % n + 1));
        let mut got = Vec::new();
        for s in seqs {
            let data = s.to_be_bytes();
            let mut d = vec![0x90, 0, 0, 1, 0x86, channel];
            d.extend(s.to_be_bytes());
            d.extend(2u16.to_be_bytes());
            d.extend(data);
            for e in p.handle_datagram(&d, 1).unwrap() {
                if let EnetEvent::Receive { channel: c, data } = e {
                    prop_assert_eq!(c, channel);
                    got.push(u16::from_be_bytes([data[0], data[1]]));
                }
            }
        }
        prop_assert_eq!(got, (1..=n).collect::<Vec<_>>());
    }

    /// encode_input: Moonlight's NV_INPUT_HEADER on every packet, mouse motion split without
    /// losing a unit, and errors only for out-of-range fields.
    #[test]
    fn input_packets_are_well_formed(
        kind in 0u8..5, a in any::<i32>(), b in any::<i32>(), x in any::<u8>(), y in any::<u8>(),
        down in any::<bool>(), sticks in any::<[i16; 4]>(), buttons in any::<u32>(), mask in any::<u16>(),
    ) {
        let ev = match kind {
            0 => InputEvent::MouseMoveRel { dx: a, dy: b },
            1 => InputEvent::MouseButton { button: x, down },
            2 => InputEvent::Scroll { amount: a as i16 },
            3 => InputEvent::Key { vk: x, down, modifiers: y },
            _ => InputEvent::Gamepad {
                index: x % 20, active_mask: mask, buttons, lt: y, rt: x,
                lx: sticks[0], ly: sticks[1], rx: sticks[2], ry: sticks[3],
            },
        };
        match encode_input(&ev) {
            Ok(ps) => {
                for p in &ps {
                    prop_assert_eq!(u32::from_be_bytes(p[..4].try_into().unwrap()) as usize, p.len() - 4);
                }
                if let InputEvent::MouseMoveRel { dx, dy } = ev {
                    prop_assert!(ps.len() <= 8);
                    let sum = |o: usize| ps.iter().map(|p| i16::from_be_bytes([p[8 + o], p[9 + o]]) as i32).sum::<i32>();
                    prop_assert_eq!(sum(0), dx.clamp(-MAX_MOUSE_DELTA, MAX_MOUSE_DELTA));
                    prop_assert_eq!(sum(2), dy.clamp(-MAX_MOUSE_DELTA, MAX_MOUSE_DELTA));
                }
                prop_assert!(ControlMsg::input(&ev).is_ok());
            }
            Err(_) => {
                let checked = matches!(
                    ev,
                    InputEvent::MouseButton { .. } | InputEvent::Key { .. } | InputEvent::Gamepad { .. }
                );
                prop_assert!(checked, "only checked fields can fail");
            }
        }
    }
}
