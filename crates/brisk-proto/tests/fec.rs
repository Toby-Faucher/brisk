//! Reed-Solomon encode and recovery against known vectors.

use brisk_proto::fec::{FecError, rs_decode, rs_encode};

/// tools/nanors-vectors/vectors.bin, produced by nanors itself (see that README).
struct Case {
    ds: usize,
    ps: usize,
    lost: Vec<usize>,
    shards: Vec<Vec<u8>>, // data then nanors parity
    recovered: Vec<Vec<u8>>,
}

fn vectors() -> Vec<Case> {
    let raw = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/nanors-vectors/vectors.bin"
    ))
    .unwrap();
    assert_eq!(&raw[..8], b"NRSV0001");
    let n = u32::from_le_bytes(raw[8..12].try_into().unwrap()) as usize;
    let mut at = 12;
    let mut take = |k: usize| {
        let s = raw[at..at + k].to_vec();
        at += k;
        s
    };
    let cases: Vec<Case> = (0..n)
        .map(|_| {
            let h = take(5);
            let (ds, ps, bs) = (
                h[0] as usize,
                h[1] as usize,
                u16::from_le_bytes([h[2], h[3]]) as usize,
            );
            let lost = take(h[4] as usize).into_iter().map(usize::from).collect();
            let shards = (0..ds + ps).map(|_| take(bs)).collect();
            let recovered = (0..ds).map(|_| take(bs)).collect();
            Case {
                ds,
                ps,
                lost,
                shards,
                recovered,
            }
        })
        .collect();
    assert_eq!(at, raw.len(), "trailing bytes in vectors.bin");
    cases
}

#[test]
fn rs_matches_nanors_for_random_schemes() {
    let cases = vectors();
    assert!(cases.len() >= 200);
    for (n, c) in cases.iter().enumerate() {
        let mut enc: Vec<Vec<u8>> = c.shards.clone();
        enc[c.ds..].iter_mut().for_each(|p| p.fill(0x55));
        rs_encode(c.ds, c.ps, &mut enc).unwrap();
        assert_eq!(
            enc, c.shards,
            "parity differs from nanors in case {n} ({}+{})",
            c.ds, c.ps
        );

        let mut dec: Vec<Option<Vec<u8>>> = c.shards.iter().cloned().map(Some).collect();
        for &l in &c.lost {
            dec[l] = None;
        }
        rs_decode(c.ds, c.ps, &mut dec).unwrap();
        for (i, want) in c.recovered.iter().enumerate() {
            assert_eq!(dec[i].as_ref(), Some(want), "case {n} shard {i}");
        }
    }
}

// Recovery up to the parity count and the too-many-losses error are property tests in
// prop_roundtrip.rs; the nanors vectors above cover large schemes.

#[test]
fn rejects_schemes_nanors_rejects() {
    assert_eq!(rs_encode(0, 1, &mut [vec![0]]), Err(FecError::BadScheme));
    assert_eq!(
        rs_decode(200, 56, &mut vec![None; 256]),
        Err(FecError::BadScheme)
    );
    assert_eq!(
        rs_decode(2, 1, &mut [Some(vec![0]), None]),
        Err(FecError::BadScheme)
    );
    assert_eq!(
        rs_decode(1, 1, &mut [Some(vec![0]), Some(vec![0, 1])]),
        Err(FecError::ShardSize)
    );
}
