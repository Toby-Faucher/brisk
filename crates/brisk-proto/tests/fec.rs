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

fn rng(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn recovers_up_to_m_losses() {
    let mut seed = 0x1234_5678_9abc_def1;
    for _ in 0..100 {
        let ds = 1 + (rng(&mut seed) % 100) as usize;
        let ps = 1 + (rng(&mut seed) % 30) as usize;
        let bs = 1 + (rng(&mut seed) % 64) as usize;
        let mut shards: Vec<Vec<u8>> = (0..ds + ps)
            .map(|_| (0..bs).map(|_| rng(&mut seed) as u8).collect())
            .collect();
        rs_encode(ds, ps, &mut shards).unwrap();
        let mut got: Vec<Option<Vec<u8>>> = shards.iter().cloned().map(Some).collect();
        // Lose exactly ps shards at random positions.
        let mut lost = 0;
        while lost < ps {
            let i = (rng(&mut seed) as usize) % (ds + ps);
            if got[i].take().is_some() {
                lost += 1;
            }
        }
        rs_decode(ds, ps, &mut got).unwrap();
        for i in 0..ds {
            assert_eq!(got[i].as_ref(), Some(&shards[i]));
        }
    }
}

#[test]
fn more_than_m_losses_is_an_error_and_leaves_shards_untouched() {
    let mut shards = vec![vec![1u8, 2, 3], vec![4, 5, 6], vec![0; 3], vec![0; 3]];
    rs_encode(2, 2, &mut shards).unwrap();
    let mut got = vec![None, None, Some(shards[2].clone()), None];
    let before = got.clone();
    assert_eq!(rs_decode(2, 2, &mut got), Err(FecError::TooManyLosses));
    assert_eq!(got, before);
}

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
