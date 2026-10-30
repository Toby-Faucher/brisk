//! RTP video packet parsing, decryption and frame assembly.

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use brisk_proto::fec::rs_encode;
use brisk_proto::rtp::*;
use brisk_proto::sdp::SS_ENC_VIDEO;

/// Builds packets exactly as Sunshine's videoBroadcastThread + fec::encode do (stream.cpp).
struct Host {
    seq: u16,
    packet_size: usize, // session->config.packetsize
    pct: usize,
}

impl Host {
    fn new(seq: u16) -> Self {
        Host {
            seq,
            packet_size: 64,
            pct: 20,
        }
    }

    /// One frame split over `blocks` FEC blocks; returns packets per block in send order.
    fn frame(
        &mut self,
        idx: u32,
        frame_type: u8,
        latency: u16,
        annexb: &[u8],
        blocks: usize,
    ) -> Vec<Vec<Vec<u8>>> {
        let blocksize = self.packet_size + 16;
        let per = blocksize - 32;
        let mut last_len = ((annexb.len() + 8) % (self.packet_size - 16)) as u16;
        if last_len == 0 {
            last_len = (self.packet_size - 16) as u16;
        }
        let mut body = vec![0x01];
        body.extend(latency.to_le_bytes());
        body.push(frame_type);
        body.extend(last_len.to_le_bytes());
        body.extend([0, 0]);
        body.extend(annexb);
        let chunks: Vec<&[u8]> = body.chunks(per).collect();
        let groups: Vec<&[&[u8]]> = chunks.chunks(chunks.len().div_ceil(blocks)).collect();
        let blocks = groups.len();
        let mut out = Vec::new();
        for (b, group) in groups.into_iter().enumerate() {
            let n = group.len();
            let mut shards: Vec<Vec<u8>> = group
                .iter()
                .enumerate()
                .map(|(x, c)| {
                    let mut s = vec![0u8; blocksize];
                    s[16..20].copy_from_slice(&((self.seq as u32 + x as u32) << 8).to_le_bytes());
                    s[20..24].copy_from_slice(&idx.to_le_bytes());
                    s[24] = FLAG_CONTAINS_PIC_DATA
                        | if x == 0 { FLAG_SOF } else { 0 }
                        | if x == n - 1 { FLAG_EOF } else { 0 };
                    s[26] = 0x10;
                    s[27] = ((b << 4) | ((blocks - 1) << 6)) as u8;
                    s[32..32 + c.len()].copy_from_slice(c);
                    s
                })
                .collect();
            let parity = (n * self.pct).div_ceil(100);
            shards.extend((0..parity).map(|_| vec![0u8; blocksize]));
            rs_encode(n, parity, &mut shards).unwrap();
            for (x, s) in shards.iter_mut().enumerate() {
                let fec = (x << 12 | n << 22 | self.pct << 4) as u32;
                s[28..32].copy_from_slice(&fec.to_le_bytes());
                s[0] = 0x90;
                s[2..4].copy_from_slice(&self.seq.wrapping_add(x as u16).to_be_bytes());
                s[4..8].copy_from_slice(&(idx.wrapping_mul(1500)).to_be_bytes());
                s[27] = ((b << 4) | ((blocks - 1) << 6)) as u8;
                s[20..24].copy_from_slice(&idx.to_le_bytes());
            }
            self.seq = self.seq.wrapping_add(shards.len() as u16);
            out.push(shards);
        }
        out
    }
}

fn annexb(n: usize, salt: u8) -> Vec<u8> {
    let mut v = vec![0, 0, 0, 1, 0x65];
    v.extend((0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt)));
    v
}

fn feed<'a>(
    a: &mut FrameAssembler,
    pkts: impl IntoIterator<Item = &'a Vec<u8>>,
) -> Vec<AssembledFrame> {
    pkts.into_iter()
        .flat_map(|p| a.push(parse_video_packet(p, 0).unwrap()))
        .collect()
}

#[test]
#[cfg_attr(
    not(video_fixture),
    ignore = "SKIPPED: no fixtures/video/*.bin yet; record with `brisk-probe stream --record-samples fixtures`"
)]
fn parses_recorded_video_packet() {
    // `brisk-probe stream --record-samples fixtures` writes NNN.bin (one plain, decrypted
    // datagram) and NNN.meta.json (the fields it parsed live) from the stream start through the
    // first IDR frame.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/video");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no *.bin in {}", dir.display());
    let field = |json: &str, k: &str| -> u64 {
        let v = json
            .split(&format!("\"{k}\":"))
            .nth(1)
            .unwrap_or_else(|| panic!("{k}"));
        v.split([',', '}']).next().unwrap().trim().parse().unwrap()
    };
    let raws: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();
    for (f, raw) in files.iter().zip(&raws) {
        let p = parse_video_packet(raw, 0).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert!(
            p.data_shards >= 1
                && (p.shard_idx as usize) < (p.data_shards + p.parity_shards) as usize
        );
        assert_eq!(p.is_parity, p.shard_idx >= p.data_shards);
        if !p.is_parity {
            assert_eq!(p.flags & !(FLAG_SOF | FLAG_EOF | FLAG_CONTAINS_PIC_DATA), 0);
        }
        let meta = std::fs::read_to_string(f.with_extension("meta.json"))
            .unwrap_or_else(|e| panic!("{}: sidecar meta: {e}", f.display()));
        let got = [
            p.seq as u64,
            p.frame_idx as u64,
            p.shard_idx as u64,
            p.data_shards as u64,
            p.parity_shards as u64,
            p.fec_block as u64,
            p.last_fec_block as u64,
        ];
        let want = [
            "seq",
            "frame_idx",
            "shard_idx",
            "data_shards",
            "parity_shards",
            "fec_block",
            "last_fec_block",
        ]
        .map(|k| field(&meta, k));
        assert_eq!(got, want, "{}", f.display());
    }
    // The samples hold the first IDR frame whole: it reassembles.
    let first = raws
        .iter()
        .map(|r| parse_video_packet(r, 0).unwrap().frame_idx)
        .min()
        .unwrap();
    let mut a = FrameAssembler::starting_at(first);
    let frames: Vec<_> = raws
        .iter()
        .flat_map(|r| a.push(parse_video_packet(r, 0).unwrap()))
        .collect();
    assert!(
        frames.iter().any(|f| f.is_idr),
        "no complete IDR frame in the samples"
    );
    assert_eq!(a.stats().parse_errors, 0);
}

#[test]
fn parses_header_fields_and_host_latency() {
    let mut h = Host::new(700);
    let f = h.frame(42, 1, 123, &annexb(100, 0), 1);
    let p = parse_video_packet(&f[0][0], 0).unwrap();
    assert_eq!(
        (p.seq, p.ts, p.frame_idx, p.shard_idx),
        (700, 42 * 1500, 42, 0)
    );
    assert_eq!((p.data_shards, p.fec_percent, p.parity_shards), (3, 20, 1));
    assert_eq!((p.fec_block, p.last_fec_block, p.is_parity), (0, 0, false));
    assert_eq!(p.flags, FLAG_SOF | FLAG_CONTAINS_PIC_DATA);
    assert_eq!(p.host_latency_100us, Some(123));
    assert_eq!(p.payload, &f[0][0][32..]);

    let second = parse_video_packet(&f[0][1], 0).unwrap();
    assert_eq!((second.shard_idx, second.host_latency_100us), (1, None));
    let parity = parse_video_packet(&f[0][3], 0).unwrap();
    assert!(parity.is_parity && parity.host_latency_100us.is_none());

    let out = feed(&mut FrameAssembler::new(), &f[0]);
    assert_eq!(out[0].host_latency_100us, Some(123));
    // Sunshine sends 0 when it has no measurement.
    let out = feed(
        &mut FrameAssembler::new(),
        &h.frame(43, 1, 0, &annexb(10, 0), 1)[0],
    );
    assert_eq!(out[0].host_latency_100us, None);
}

#[test]
fn idr_flag_follows_frame_type() {
    let mut h = Host::new(0);
    let mut a = FrameAssembler::new();
    let idr = feed(&mut a, &h.frame(1, 2, 5, &annexb(200, 1), 1)[0]);
    let p = feed(&mut a, &h.frame(2, 1, 5, &annexb(200, 2), 1)[0]);
    let rfi = feed(&mut a, &h.frame(3, 5, 5, &annexb(200, 3), 1)[0]);
    assert!(idr[0].is_idr && !p[0].is_idr && !rfi[0].is_idr);
}

#[test]
fn reassembles_exact_annexb_across_sizes_and_blocks() {
    let mut h = Host::new(0);
    let mut a = FrameAssembler::new();
    let mut idx = 1;
    for blocks in 1..=4 {
        for n in [0usize, 1, 37, 38, 39, 40, 300, 1000] {
            let data = annexb(n, n as u8);
            let f = h.frame(idx, 1, 9, &data, blocks);
            let out = feed(&mut a, f.iter().flatten());
            assert_eq!(out.len(), 1, "n={n} blocks={blocks}");
            assert_eq!(
                (out[0].idx, &out[0].annexb),
                (idx, &data),
                "n={n} blocks={blocks}"
            );
            idx += 1;
        }
    }
    assert_eq!(
        a.stats(),
        AssemblerStats {
            frames_ok: 32,
            ..Default::default()
        }
    );
}

#[test]
fn assembler_recovers_up_to_m_losses_per_block() {
    let mut h = Host::new(0);
    h.pct = 50;
    let data = annexb(2000, 7);
    let f = h.frame(1, 2, 1, &data, 2);
    let mut a = FrameAssembler::new();
    let mut out = Vec::new();
    for block in &f {
        let parity = block.len() - parse_video_packet(&block[0], 0).unwrap().data_shards as usize;
        // Drop `parity` data shards (worst case: only parity left to rebuild them).
        out.extend(feed(&mut a, block.iter().skip(parity)));
    }
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].annexb, data);
    assert!(out[0].is_idr);
    let s = a.stats();
    assert_eq!((s.frames_ok, s.fec_recovered, s.frames_dropped), (1, 1, 0));
}

#[test]
fn more_than_m_losses_drops_frame_and_counts_it() {
    let mut h = Host::new(0);
    let mut a = FrameAssembler::new();
    let bad = h.frame(1, 1, 1, &annexb(500, 1), 1);
    let parity = bad[0].len() - parse_video_packet(&bad[0][0], 0).unwrap().data_shards as usize;
    assert!(feed(&mut a, bad[0].iter().skip(parity + 1)).is_empty());
    assert_eq!(
        a.take_dropped(),
        0,
        "not known lost until a later frame completes"
    );
    let good = h.frame(2, 1, 1, &annexb(500, 2), 1);
    let out = feed(&mut a, &good[0]);
    assert_eq!(out.iter().map(|f| f.idx).collect::<Vec<_>>(), [2]);
    assert_eq!(
        (a.take_dropped(), a.take_dropped()),
        (1, 0),
        "reported once"
    );
    // A wholly lost frame 3 is counted too when frame 4 completes.
    h.frame(3, 1, 1, &annexb(10, 3), 1);
    assert_eq!(
        feed(&mut a, &h.frame(4, 1, 1, &annexb(10, 4), 1)[0]).len(),
        1
    );
    let s = a.stats();
    assert_eq!((s.frames_ok, s.frames_dropped), (2, 2));
    assert_eq!(a.take_dropped(), 1);
    // Late shards of the dropped frame are ignored.
    assert!(feed(&mut a, bad[0].iter().take(parity + 1)).is_empty());
}

#[test]
fn rtp_seq_wrap_orders_correctly() {
    let mut h = Host::new(65533);
    let data = annexb(400, 3);
    let mut f = h.frame(1, 1, 1, &data, 1).remove(0);
    assert!(
        u16::from_be_bytes([f[0][2], f[0][3]])
            > u16::from_be_bytes([f[f.len() - 1][2], f[f.len() - 1][3]])
    );
    f.reverse(); // worst reordering, across the wrap
    f.remove(0); // and lose a parity shard
    let out = feed(&mut FrameAssembler::new(), &f);
    assert_eq!(out[0].annexb, data);
}

#[test]
fn shard_with_inconsistent_seq_is_rejected() {
    let mut h = Host::new(10);
    let mut f = h.frame(1, 1, 1, &annexb(200, 3), 1).remove(0);
    f[1][2..4].copy_from_slice(&999u16.to_be_bytes());
    let mut a = FrameAssembler::new();
    feed(&mut a, &f);
    assert_eq!(a.stats().parse_errors, 1);
}

#[test]
fn duplicate_shard_ignored() {
    let mut h = Host::new(0);
    let data = annexb(300, 9);
    let f = h.frame(1, 1, 1, &data, 1).remove(0);
    let mut a = FrameAssembler::new();
    let mut pkts: Vec<&Vec<u8>> = Vec::new();
    for p in &f[..f.len() - 2] {
        pkts.push(p);
        pkts.push(p);
    }
    pkts.extend(&f[f.len() - 2..]);
    pkts.extend(&f); // all again after completion
    let out = feed(&mut a, pkts);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].annexb, data);
    assert_eq!(
        a.stats(),
        AssemblerStats {
            frames_ok: 1,
            ..Default::default()
        }
    );
}

#[test]
fn frame_idx_wrap_orders_correctly() {
    let mut h = Host::new(0);
    let (d1, d2) = (annexb(300, 1), annexb(300, 2));
    let older = h.frame(u32::MAX, 1, 1, &d1, 1).remove(0);
    let newer = h.frame(0, 1, 1, &d2, 1).remove(0);
    let mut a = FrameAssembler::starting_at(u32::MAX);
    assert!(feed(&mut a, &older[..1]).is_empty());
    assert!(feed(&mut a, &newer[..2]).is_empty());
    assert_eq!(feed(&mut a, &older[1..])[0].idx, u32::MAX);
    assert_eq!(feed(&mut a, &newer[2..])[0].annexb, d2);
    // u32::MAX is now behind 0.
    assert!(feed(&mut a, &older).is_empty());
    assert_eq!(
        a.stats(),
        AssemblerStats {
            frames_ok: 2,
            ..Default::default()
        }
    );
}

#[test]
fn truncated_packet_errors() {
    let mut h = Host::new(0);
    let f = h.frame(1, 1, 1, &annexb(10, 0), 1);
    for n in 0..=HEADER {
        assert_eq!(
            parse_video_packet(&f[0][0][..n], 0),
            Err(RtpError::Truncated),
            "len {n}"
        );
    }
    let mut bad = f[0][0].clone();
    bad[0] = 0x80; // extension bit missing
    assert_eq!(parse_video_packet(&bad, 0), Err(RtpError::Malformed));
}

#[test]
fn packet_length_and_count_caps() {
    let mut h = Host::new(0);
    let f = h.frame(1, 1, 1, &annexb(10, 0), 1);
    let mut big = f[0][0].clone();
    big.resize(MAX_VIDEO_PACKET + 1, 0);
    assert_eq!(parse_video_packet(&big, 0), Err(RtpError::TooLarge));

    let with_fec = |fec: u32, blocks: u8| {
        let mut p = f[0][0].clone();
        p[28..32].copy_from_slice(&fec.to_le_bytes());
        p[27] = blocks;
        p
    };
    // 1023 data shards at 255% parity: far beyond nanors' 255 shards.
    assert_eq!(
        parse_video_packet(&with_fec(1023 << 22 | 255 << 4, 0), 0),
        Err(RtpError::Malformed)
    );
    // Zero data shards; shard index beyond the block; block index beyond last block.
    assert_eq!(
        parse_video_packet(&with_fec(20 << 4, 0), 0),
        Err(RtpError::Malformed)
    );
    assert_eq!(
        parse_video_packet(&with_fec(5 << 12 | 4 << 22, 0), 0),
        Err(RtpError::Malformed)
    );
    assert_eq!(
        parse_video_packet(&with_fec(4 << 22, 0x10), 0),
        Err(RtpError::Malformed)
    );
    // FEC off (Sunshine's oversized frames): 1023 data shards are allowed, slots only.
    let p = with_fec(1023 << 22, 0);
    let mut a = FrameAssembler::new();
    assert!(a.push(parse_video_packet(&p, 0).unwrap()).is_empty());
}

#[test]
fn open_frames_are_bounded() {
    let mut h = Host::new(0);
    let mut a = FrameAssembler::new();
    let frames: Vec<_> = (1..=10)
        .map(|i| h.frame(i, 1, 1, &annexb(300, i as u8), 1).remove(0))
        .collect();
    for f in &frames {
        feed(&mut a, &f[..1]);
    }
    // Only the newest MAX_PENDING_FRAMES are still open: frame 1 lost its first shard, so its
    // remaining data shards alone (no parity) cannot finish it.
    let d = parse_video_packet(&frames[0][0], 0).unwrap().data_shards as usize;
    assert!(feed(&mut a, &frames[0][1..d]).is_empty());
    assert_eq!(feed(&mut a, &frames[9][1..]).len(), 1);
    assert_eq!(a.stats().frames_dropped, 9);
}

#[test]
fn encrypted_video_round_trip() {
    let key = [7u8; 16];
    let mut h = Host::new(0);
    let plain = h.frame(1, 1, 1, &annexb(10, 0), 1).remove(0).remove(0);
    // Sunshine: iv = LE u64 counter, iv[11] = 'V'; prefix = iv, frameNumber LE, tag.
    let mut iv = [0u8; 12];
    iv[..8].copy_from_slice(&5u64.to_le_bytes());
    iv[11] = b'V';
    let mut ct = plain.clone();
    let tag = Aes128Gcm::new(&key.into())
        .encrypt_in_place_detached(Nonce::from_slice(&iv), &[], &mut ct)
        .unwrap();
    let mut wire = iv.to_vec();
    wire.extend(1u32.to_le_bytes());
    wire.extend(tag);
    wire.extend(ct);

    assert_eq!(
        parse_video_packet(&wire, SS_ENC_VIDEO),
        Err(RtpError::Encrypted)
    );
    let c = VideoCipher::new(&key);
    let opened = c.open(&wire).unwrap();
    assert_eq!(opened, plain);
    assert_eq!(parse_video_packet(&opened, 0).unwrap().frame_idx, 1);

    let mut tampered = wire.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert_eq!(c.open(&tampered), Err(RtpError::BadTag));
    assert_eq!(
        c.open(&wire[..ENC_PREFIX + HEADER]),
        Err(RtpError::Truncated)
    );
    assert_eq!(
        c.open(&vec![0; MAX_VIDEO_PACKET + ENC_PREFIX + 1]),
        Err(RtpError::TooLarge)
    );
}

#[test]
fn early_frame_2_packets_do_not_drop_first_idr() {
    let mut h = Host::new(0);
    let (d1, d2) = (annexb(300, 1), annexb(300, 2));
    let f1 = h.frame(1, 2, 1, &d1, 1).remove(0);
    let f2 = h.frame(2, 1, 1, &d2, 1).remove(0);
    let mut a = FrameAssembler::new();
    // The first packets seen belong to frame 2; frame 1 finishes afterwards.
    assert!(feed(&mut a, &f2[..1]).is_empty());
    assert!(feed(&mut a, &f1[..2]).is_empty());
    let out = feed(&mut a, &f1[2..]);
    assert_eq!((out[0].idx, out[0].is_idr, &out[0].annexb), (1, true, &d1));
    assert_eq!(feed(&mut a, &f2[1..])[0].annexb, d2);
    assert_eq!(a.stats().frames_dropped, 0);
}

#[test]
fn far_future_frame_is_rejected() {
    let mut h = Host::new(0);
    let (d1, d2) = (annexb(300, 1), annexb(300, 2));
    let far = h.frame(1 + (1 << 16) + 1, 1, 1, &d1, 1).remove(0);
    let ok = h.frame(1, 2, 1, &d2, 1).remove(0);
    let mut a = FrameAssembler::new();
    assert!(feed(&mut a, &far).is_empty());
    assert_eq!(a.stats().parse_errors, far.len() as u64);
    assert_eq!(feed(&mut a, &ok)[0].annexb, d2);
}

#[test]
fn hostile_hand_built_packets_do_not_panic() {
    let mut h = Host::new(0);
    let f = h.frame(1, 2, 1, &annexb(300, 1), 1).remove(0);
    let good = parse_video_packet(&f[0], 0).unwrap();
    let bad: Vec<VideoPacket> = vec![
        VideoPacket {
            fec_block: 200,
            last_fec_block: 255,
            ..good.clone()
        },
        VideoPacket {
            last_fec_block: 9,
            ..good.clone()
        },
        VideoPacket {
            shard_idx: 60000,
            ..good.clone()
        },
        VideoPacket {
            data_shards: u16::MAX,
            parity_shards: u16::MAX,
            ..good.clone()
        },
        VideoPacket {
            data_shards: 0,
            ..good.clone()
        },
        VideoPacket {
            raw: &f[0][..10],
            ..good.clone()
        },
        VideoPacket {
            is_parity: true,
            ..good.clone()
        },
    ];
    let mut a = FrameAssembler::new();
    for p in bad {
        assert!(a.push(p).is_empty());
    }
    assert_eq!(a.stats().parse_errors, 7);
    assert_eq!(feed(&mut a, &f).len(), 1);
}

/// Erasure patterns of `e` out of `n` data shards: every combination for small blocks, else
/// every window of `e` consecutive shards (wrapping) plus an evenly spread one.
fn erasures(n: usize, e: usize) -> Vec<Vec<usize>> {
    if n <= 12 {
        return (0u32..1 << n)
            .filter(|m| m.count_ones() as usize == e)
            .map(|m| (0..n).filter(|i| m & 1 << i != 0).collect())
            .collect();
    }
    let mut v: Vec<Vec<usize>> = (0..n)
        .map(|s| (0..e).map(|k| (s + k) % n).collect())
        .collect();
    v.push((0..e).map(|k| k * n / e).collect());
    v
}

/// Real Sunshine parity: erase up to `parity` data shards of every complete recorded block and
/// recover them. Only payload bytes are compared: Sunshine rewrites the RTP header, frameIndex,
/// multiFecBlocks and fecInfo after encoding, so recovered header bytes are not the received ones.
#[test]
#[cfg_attr(
    not(video_fixture),
    ignore = "SKIPPED: no fixtures/video/*.bin yet; record with `brisk-probe stream --record-samples fixtures`"
)]
fn recorded_video_fec_recovers_erased_data_shards() {
    use brisk_proto::fec::rs_decode;
    use std::collections::BTreeMap;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/video");
    let raws: Vec<Vec<u8>> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .map(|p| std::fs::read(p).unwrap())
        .collect();
    // (frame, fec block) -> shard index -> packet
    let mut blocks: BTreeMap<(u32, u8), BTreeMap<u16, &Vec<u8>>> = BTreeMap::new();
    for r in &raws {
        let p = parse_video_packet(r, 0).unwrap();
        blocks
            .entry((p.frame_idx, p.fec_block))
            .or_default()
            .insert(p.shard_idx, r);
    }
    let (mut checked, mut erased, mut patterns, mut idr_checked) = (0, 0, 0, false);
    for ((frame, _), shards) in &blocks {
        let p0 = parse_video_packet(shards.values().next().unwrap(), 0).unwrap();
        let (d, m) = (p0.data_shards as usize, p0.parity_shards as usize);
        if shards.len() != d + m || m == 0 || p0.last_fec_block != 0 {
            continue; // incomplete (end of the recording) or multi-block: nothing to compare with
        }
        let full: Vec<&Vec<u8>> = shards.values().copied().collect();
        let e = m.min(d);
        let pats = erasures(d, e);
        for (k, gone) in pats.iter().enumerate() {
            let mut s: Vec<Option<Vec<u8>>> = full.iter().map(|r| Some(r.to_vec())).collect();
            for &i in gone {
                s[i] = None;
            }
            rs_decode(d, m, &mut s).unwrap();
            for &i in gone {
                assert!(
                    s[i].as_ref().unwrap()[HEADER..] == full[i][HEADER..],
                    "frame {frame} shard {i} erased {gone:?}: recovered payload differs"
                );
            }
            if k == 0 {
                // The assembler gives back the same frame from the survivors.
                let whole = feed(
                    &mut FrameAssembler::starting_at(*frame),
                    full.iter().copied(),
                );
                let mut a = FrameAssembler::starting_at(*frame);
                let got = feed(
                    &mut a,
                    (0..d + m).filter(|i| !gone.contains(i)).map(|i| full[i]),
                );
                assert_eq!(got.len(), 1, "frame {frame}");
                assert_eq!(got, whole, "frame {frame}");
                assert_eq!(a.stats().fec_recovered, 1);
                idr_checked |= got[0].is_idr;
            }
        }
        checked += 1;
        erased += e;
        patterns += pats.len();
    }
    eprintln!(
        "video FEC replay: {checked} recorded blocks, {erased} data shards erased (max per block), {patterns} erasure patterns, payloads equal"
    );
    assert!(idr_checked, "the IDR block was not exercised");
    assert!(checked >= 10, "{checked}");
}
