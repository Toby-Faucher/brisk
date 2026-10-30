//! Audio packet decryption and audio FEC against known vectors.

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
use brisk_proto::audio::*;

const KEY: [u8; 16] = *b"0123456789abcdef";
const KEY_ID: u32 = 0x1234_5678;
const DUR: u32 = 5;

/// Encrypts like Sunshine's `encode_audio`: AES-128-CBC/PKCS7, IV = BE32(rikeyid + seq) + zeros.
fn encrypt(key: &[u8; 16], key_id: u32, seq: u16, plain: &[u8]) -> Vec<u8> {
    let aes = Aes128::new(key.into());
    let mut data = plain.to_vec();
    let pad = 16 - plain.len() % 16;
    data.extend(std::iter::repeat_n(pad as u8, pad));
    let mut prev = [0u8; 16];
    prev[..4].copy_from_slice(&key_id.wrapping_add(seq as u32).to_be_bytes());
    for c in data.chunks_mut(16) {
        for (b, p) in c.iter_mut().zip(prev) {
            *b ^= p;
        }
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

fn plain(seq: u16) -> Vec<u8> {
    plain_n(seq, 100)
}

fn plain_n(seq: u16, n: u32) -> Vec<u8> {
    (0..n).map(|i| (i as u8) ^ (seq as u8)).collect()
}

/// One FEC block as the host sends it: 4 data datagrams then 2 FEC datagrams.
fn block(base: u16) -> Vec<Vec<u8>> {
    block_n(base, 100)
}

/// [`block`] with `n` plaintext bytes per packet.
fn block_n(base: u16, n: u32) -> Vec<Vec<u8>> {
    let ts = |i: u16| {
        u32::from(base)
            .wrapping_mul(DUR)
            .wrapping_add(i as u32 * DUR)
    };
    let mut shards: Vec<Vec<u8>> = (0..4)
        .map(|i| {
            encrypt(
                &KEY,
                KEY_ID,
                base.wrapping_add(i),
                &plain_n(base.wrapping_add(i), n),
            )
        })
        .collect();
    shards.extend([vec![0; shards[0].len()], vec![0; shards[0].len()]]);
    encode_parity(&mut shards).unwrap();
    let mut out: Vec<Vec<u8>> = (0..4)
        .map(|i| rtp(PT_AUDIO, base.wrapping_add(i), ts(i), &shards[i as usize]))
        .collect();
    for x in 0..2u16 {
        let mut body = vec![x as u8, PT_AUDIO];
        body.extend(base.to_be_bytes());
        body.extend(ts(0).to_be_bytes());
        body.extend([0; 4]);
        body.extend(&shards[4 + x as usize]);
        out.push(rtp(PT_FEC, base.wrapping_add(4 + x), 0, &body));
    }
    out
}

/// Feeds the packets at `keep`; returns the decrypted frames' seqs and plaintext checks.
fn run(fec: &mut AudioFec, b: &[Vec<u8>], keep: &[usize]) -> Vec<OpusFrame> {
    let c = AudioCipher::new(&KEY, KEY_ID);
    keep.iter()
        .flat_map(|&i| fec.push(&b[i]).unwrap())
        .map(|p| c.open(&p).unwrap())
        .collect()
}

fn check(frames: &[OpusFrame], base: u16, want: &[u16]) {
    let mut got: Vec<_> = frames.iter().map(|f| f.seq.wrapping_sub(base)).collect();
    got.sort();
    assert_eq!(got, want);
    for f in frames {
        assert_eq!(f.data, plain(f.seq));
        assert_eq!(
            f.ts,
            u32::from(base)
                .wrapping_mul(DUR)
                .wrapping_add(f.seq.wrapping_sub(base) as u32 * DUR)
        );
    }
}

/// `rikey.hex` next to the recorded datagrams: "<32 hex rikey> <8 hex rikeyid>".
fn fixture_key(dir: &std::path::Path) -> ([u8; 16], u32) {
    let meta = std::fs::read_to_string(dir.join("rikey.hex")).expect("rikey.hex next to the .bin");
    let (k, id) = meta.trim().split_once(' ').unwrap();
    let key = brisk_proto::pair::unhex(k).unwrap().try_into().unwrap();
    (key, u32::from_str_radix(id, 16).unwrap())
}

#[test]
#[cfg_attr(
    not(audio_fixture),
    ignore = "SKIPPED: no fixtures/audio/*.bin yet; record with `brisk-probe stream --record-samples fixtures`"
)]
fn decrypts_recorded_audio_packet() {
    // `brisk-probe stream --record-samples fixtures` writes NNN.bin (one raw datagram each) and
    // rikey.hex ("<32 hex rikey> <8 hex rikeyid>"): the ephemeral key of that one finished
    // session (see the README written next to it). FEC datagrams (payload type 127) are
    // skipped; data packets (type 97) are decrypted.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/audio");
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "bin"))
        .collect();
    assert!(!files.is_empty(), "no *.bin in {}", dir.display());
    let (key, id) = fixture_key(&dir);
    let mut data = 0;
    for f in files {
        let raw = std::fs::read(&f).unwrap();
        if raw.get(1).is_some_and(|b| b & 0x7f == PT_FEC) {
            continue;
        }
        let o =
            parse_audio_packet(&raw, &key, id).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        assert!(!o.data.is_empty() && o.data.len() <= MAX_PAYLOAD);
        data += 1;
    }
    assert!(data > 0, "no data packets");
}

#[test]
fn decrypts_synthetic_packet() {
    let b = block(8);
    let o = parse_audio_packet(&b[1], &KEY, KEY_ID).unwrap();
    assert_eq!((o.seq, o.ts, o.data), (9, 8 * DUR + DUR, plain(9)));
}

#[test]
fn parity_matrix_recovers_any_two_losses() {
    // All 15 pairs of the 6 shards: the Sunshine matrix is MDS, so 4 of 6 always decode.
    for x in 0..6 {
        for y in x + 1..6 {
            let b = block(0);
            let keep: Vec<usize> = (0..6).filter(|&i| i != x && i != y).collect();
            let mut fec = AudioFec::new(DUR);
            let frames = run(&mut fec, &b, &keep);
            check(&frames, 0, &[0, 1, 2, 3]);
            let lost_data = [x, y].iter().filter(|&&i| i < 4).count() as u64;
            assert_eq!(fec.stats().recovered, lost_data, "lost {x} and {y}");
        }
    }
}

#[test]
fn recovers_one_lost_of_four() {
    for lost in 0..4 {
        let b = block(100);
        let mut fec = AudioFec::new(DUR);
        let keep: Vec<usize> = [0, 1, 2, 3, 4].into_iter().filter(|&i| i != lost).collect();
        let frames = run(&mut fec, &b, &keep);
        check(&frames, 100, &[0, 1, 2, 3]);
        assert_eq!(
            fec.stats(),
            AudioFecStats {
                recovered: 1,
                lost: 0
            }
        );
    }
}

#[test]
fn three_lost_is_not_recoverable() {
    let b = block(0);
    let mut fec = AudioFec::new(DUR);
    let frames = run(&mut fec, &b, &[1, 4, 5]);
    // only the packet that arrived
    check(&frames, 0, &[1]);
    // Pushing the next blocks evicts the incomplete one and counts its 3 lost packets.
    for base in [4, 8, 12, 16] {
        run(&mut fec, &block(base), &[0, 1, 2, 3]);
    }
    assert_eq!(
        fec.stats(),
        AudioFecStats {
            recovered: 0,
            lost: 3
        }
    );
}

#[test]
fn wrong_key_errors() {
    let b = block(0);
    let mut bad = KEY;
    bad[0] ^= 1;
    assert_eq!(
        parse_audio_packet(&b[0], &bad, KEY_ID),
        Err(AudioError::BadPadding)
    );

    // A wrong rikeyid only changes the IV: CBC garbles block 0 but the padding stays valid, so
    // there is no error, just never the original frame (the stream is unauthenticated).
    let wrong_id = parse_audio_packet(&b[0], &KEY, KEY_ID + 1).map(|f| f.data);
    assert_ne!(wrong_id, Ok(plain(0)));
}

#[test]
fn malformed_packets_error_without_panic() {
    let b = block(0);
    let d = &b[0];
    let ok = |d: &[u8]| parse_audio_packet(d, &KEY, KEY_ID);
    assert_eq!(ok(&d[..12]), Err(AudioError::Truncated));
    assert_eq!(ok(&[]), Err(AudioError::Truncated));
    // not whole AES blocks
    assert_eq!(ok(&d[..d.len() - 1]), Err(AudioError::BadPadding));
    // one block: wrong padding
    assert_eq!(ok(&d[..12 + 16]), Err(AudioError::BadPadding));
    let mut big = d.clone();
    big.resize(MAX_DATAGRAM + 1, 0);
    assert_eq!(ok(&big), Err(AudioError::TooLarge));
    let mut v = d.clone();
    v[0] = 0x00;
    assert_eq!(ok(&v), Err(AudioError::Malformed));
    // FEC packet is not a data packet
    assert_eq!(ok(&b[4]), Err(AudioError::Malformed));
    // Flipping ciphertext is not authenticated (CBC): it must not return the original frame.
    let mut v = d.clone();
    let n = v.len();
    v[n - 20] ^= 0xff;
    assert_ne!(ok(&v).ok().map(|f| f.data), Some(plain(0)));

    // Garbage into the FEC queue never panics.
    let mut fec = AudioFec::new(DUR);
    let mut x = 0x1234_5678u32;
    for len in 0..200 {
        let g: Vec<u8> = (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        let _ = fec.push(&g);
        let mut g2 = g.clone();
        if !g2.is_empty() {
            g2[0] = 0x80;
        }
        if g2.len() > 1 {
            g2[1] = if len % 2 == 0 { PT_AUDIO } else { PT_FEC };
        }
        let _ = fec.push(&g2);
    }
}

#[test]
fn fec_header_and_window_checks() {
    let b = block(0);
    let mut fec = AudioFec::new(DUR);
    fec.push(&b[0]).unwrap();
    let mut bad_idx = b[4].clone();
    bad_idx[12] = 2;
    assert_eq!(fec.push(&bad_idx), Err(AudioError::Malformed));
    let mut bad_base = b[4].clone();
    bad_base[14..16].copy_from_slice(&2u16.to_be_bytes());
    assert_eq!(fec.push(&bad_base), Err(AudioError::Malformed));
    assert_eq!(fec.push(&b[4][..20]), Err(AudioError::Truncated));
    let mut short = b[4].clone();
    short.truncate(short.len() - 16);
    assert_eq!(fec.push(&short), Err(AudioError::BlockSize));
    let far = block(4 + 2048);
    assert_eq!(fec.push(&far[0]), Err(AudioError::OutOfWindow));
    // The stream still works afterwards.
    assert_eq!(fec.push(&b[1]).unwrap().len(), 1);
}

#[test]
fn seq_u16_wrap() {
    let mut fec = AudioFec::new(DUR);
    let c = AudioCipher::new(&KEY, KEY_ID);
    for base in [65528u16, 65532, 0, 4] {
        let b = block(base);
        let mut got = Vec::new();
        for i in [0usize, 2, 3, 4] {
            got.extend(fec.push(&b[i]).unwrap());
        }
        let seqs: Vec<u16> = got.iter().map(|p| p.seq).collect();
        let want: Vec<u16> = (0..4).map(|i| base.wrapping_add(i)).collect();
        let mut s = seqs.clone();
        s.sort_by_key(|x| x.wrapping_sub(base));
        assert_eq!(s, want);
        for p in &got {
            assert_eq!(c.open(p).unwrap().data, plain(p.seq));
        }
    }
    assert_eq!(fec.stats().recovered, 4);
}

#[test]
fn duplicate_ignored() {
    let b = block(0);
    let mut fec = AudioFec::new(DUR);
    assert_eq!(fec.push(&b[0]).unwrap().len(), 1);
    assert!(fec.push(&b[0]).unwrap().is_empty());
    assert!(fec.push(&b[4]).unwrap().is_empty());
    assert!(fec.push(&b[4]).unwrap().is_empty());
    assert_eq!(fec.push(&b[1]).unwrap().len(), 1);
    // 2 and the recovered 3
    assert_eq!(fec.push(&b[2]).unwrap().len(), 2);
    // already recovered
    assert!(fec.push(&b[3]).unwrap().is_empty());
    assert_eq!(fec.stats().recovered, 1);
}

#[test]
fn parity_matrix_layout_known_answer() {
    // Sunshine stream.cpp / moonlight RtpAudioQueue.c, row-major 2 x 4.
    let rows = [[0x77u8, 0x40, 0x38, 0x0e], [0xc7, 0xa7, 0x0d, 0x6c]];
    for (i, (r0, r1)) in rows[0].iter().zip(&rows[1]).enumerate() {
        let mut shards: Vec<Vec<u8>> = (0..6).map(|j| vec![u8::from(j == i)]).collect();
        encode_parity(&mut shards).unwrap();
        assert_eq!((shards[4][0], shards[5][0]), (*r0, *r1));
    }
}

/// The data packets of `n` blocks from `base` on; returns (OutOfWindow errors, packets out).
fn push_data_blocks(fec: &mut AudioFec, base: u16, n: u16) -> (usize, usize) {
    let (mut errs, mut got) = (0, 0);
    for k in 0..n {
        for p in &block(base + 4 * k)[..4] {
            match fec.push(p) {
                Ok(v) => got += v.len(),
                Err(e) => {
                    assert_eq!(e, AudioError::OutOfWindow);
                    errs += 1;
                }
            }
        }
    }
    (errs, got)
}

#[test]
fn far_jump_resyncs_and_output_resumes() {
    let mut fec = AudioFec::new(DUR);
    run(&mut fec, &block(0), &[0, 1, 2, 3]);
    // Jump 2048 packets ahead (outage or host restart): errors first, then it resumes.
    assert_eq!(push_data_blocks(&mut fec, 2048, 4), (7, 9));
    // And recovery works again in the new position.
    let b = block(2048 + 16);
    let frames = run(&mut fec, &b, &[0, 1, 4, 5]);
    check(&frames, 2048 + 16, &[0, 1, 2, 3]);
}

#[test]
fn lone_stray_and_near_behind_packets_do_not_reset() {
    let mut fec = AudioFec::new(DUR);
    run(&mut fec, &block(5000), &[0, 1, 2, 3]);
    assert_eq!(fec.push(&block(9000)[0]), Err(AudioError::OutOfWindow));
    // Late packets up to 1024 behind the floor are stale drops and never count as strays.
    for _ in 0..20 {
        assert!(fec.push(&block(4000)[0]).unwrap().is_empty());
    }
    let frames = run(&mut fec, &block(5004), &[0, 1, 2, 3]);
    check(&frames, 5004, &[0, 1, 2, 3]);
    assert_eq!(fec.stats().lost, 0);
}

#[test]
fn restart_behind_floor_resyncs() {
    // Host restarts and its sequence numbers begin again at 0 while the floor sits at 5000.
    let mut fec = AudioFec::new(DUR);
    run(&mut fec, &block(5000), &[0, 1, 2, 3]);
    assert_eq!(push_data_blocks(&mut fec, 0, 6), (7, 17));
    let frames = run(&mut fec, &block(24), &[0, 3, 4, 5]);
    check(&frames, 24, &[0, 1, 2, 3]);
}

#[test]
fn odd_sized_data_packet_still_passes_through() {
    let b = block(0);
    let mut fec = AudioFec::new(DUR);
    assert_eq!(fec.push(&b[0]).unwrap().len(), 1);
    let mut odd = b[1].clone();
    odd.truncate(odd.len() - 16);
    assert_eq!(fec.push(&odd).unwrap().len(), 1);
    assert!(fec.push(&odd).is_err()); // a repeat of the odd one is not re-emitted
    assert_eq!(fec.push(&b[2]).unwrap().len(), 1);
}

#[test]
fn odd_sized_first_packet_does_not_set_block_size() {
    let mut fec = AudioFec::new(DUR);
    run(&mut fec, &block(0), &[0, 1, 2, 3]);
    let b = block(4);
    let mut odd = b[0].clone();
    odd.truncate(odd.len() - 16);
    assert_eq!(fec.push(&odd).unwrap().len(), 1);
    // The normal packets of the same block still take part in FEC recovery.
    let frames = run(&mut fec, &b, &[1, 4, 5, 3]);
    check(&frames, 4, &[1, 2, 3]);
}

#[test]
fn resync_to_a_different_packet_size_keeps_fec_working() {
    let mut fec = AudioFec::new(DUR);
    run(&mut fec, &block(5000), &[0, 1, 2, 3]);
    // Host restarts at seq 0 with larger packets (encrypted 112 -> 192 bytes).
    for k in 0..2u16 {
        for p in &block_n(4 * k, 180)[..4] {
            let _ = fec.push(p);
        }
    }
    let b = block_n(8, 180);
    let c = AudioCipher::new(&KEY, KEY_ID);
    let got: Vec<_> = [0usize, 2, 3, 4]
        .iter()
        .flat_map(|&i| fec.push(&b[i]).unwrap())
        .map(|p| c.open(&p).unwrap())
        .collect();
    assert_eq!(got.len(), 4);
    for f in got {
        assert_eq!(f.data, plain_n(f.seq, 180));
    }
    assert_eq!(fec.stats().recovered, 1);
}

/// Real Sunshine audio parity (its replacement matrix [`PARITY`]): erase one or two data packets
/// of every complete recorded 4+2 block, recover them with [`AudioFec`], and check the recovered
/// ciphertext payload and its decrypted Opus bytes equal the received ones. Only payloads are
/// compared, not RTP headers.
#[test]
#[cfg_attr(
    not(audio_fixture),
    ignore = "SKIPPED: no fixtures/audio/*.bin yet; record with `brisk-probe stream --record-samples fixtures`"
)]
fn recorded_audio_fec_recovers_erased_packets() {
    use std::collections::BTreeMap;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/audio");
    let (key, id) = fixture_key(&dir);
    let cipher = AudioCipher::new(&key, id);
    // block base -> slot (0..4 data, 4..6 FEC) -> datagram
    let mut blocks: BTreeMap<u16, BTreeMap<usize, Vec<u8>>> = BTreeMap::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        if e.path().extension().is_none_or(|x| x != "bin") {
            continue;
        }
        let raw = std::fs::read(e.path()).unwrap();
        let seq = u16::from_be_bytes([raw[2], raw[3]]);
        let (base, slot) = if raw[1] & 0x7f == PT_FEC {
            let base = u16::from_be_bytes([raw[RTP_HEADER + 2], raw[RTP_HEADER + 3]]);
            (base, DATA_SHARDS + raw[RTP_HEADER] as usize)
        } else {
            (seq - seq % 4, (seq % 4) as usize)
        };
        blocks.entry(base).or_default().insert(slot, raw);
    }
    let erasures: Vec<Vec<usize>> = (0..4)
        .map(|a| vec![a])
        .chain((0..4).flat_map(|a| (a + 1..4).map(move |b| vec![a, b])))
        .collect();
    let (mut checked, mut recovered) = (0, 0);
    for (base, b) in &blocks {
        if b.len() != DATA_SHARDS + FEC_SHARDS {
            continue; // the recording starts or ends mid-block
        }
        for gone in &erasures {
            let mut fec = AudioFec::new(DUR);
            let mut got: Vec<AudioPayload> = b
                .iter()
                .filter(|(slot, _)| !gone.contains(slot))
                .flat_map(|(_, d)| fec.push(d).unwrap())
                .collect();
            got.sort_by_key(|p| p.seq);
            assert_eq!(
                fec.stats().recovered,
                gone.len() as u64,
                "block {base} {gone:?}"
            );
            assert_eq!(got.len(), DATA_SHARDS, "block {base} {gone:?}");
            for (i, p) in got.iter().enumerate() {
                let orig = parse_audio_payload(&b[&i]).unwrap();
                assert_eq!(p.seq, orig.seq);
                assert!(
                    p.payload == orig.payload,
                    "block {base} packet {i} erased {gone:?}: recovered payload differs"
                );
                assert_eq!(
                    cipher.open(p).unwrap().data,
                    cipher.open(&orig).unwrap().data,
                    "block {base} packet {i}: Opus bytes differ"
                );
            }
            recovered += gone.len();
        }
        checked += 1;
    }
    eprintln!(
        "audio FEC replay: {checked} recorded 4+2 blocks, {} erasure patterns each, {recovered} packets recovered, payloads and Opus bytes equal",
        erasures.len()
    );
    assert!(checked >= 10, "{checked}");
}
