//! Reed-Solomon erasure code over GF(2^8), byte-compatible with nanors as Sunshine and
//! moonlight-common-c use it (`reed_solomon_new(ds, ps)`, nanors rs.c): field polynomial 0x11d,
//! generator 2, systematic code whose parity rows form the Cauchy matrix
//! `P[j][i] = inv((ps + i) ^ j)`. Decoding solves for the missing data shards; any square
//! submatrix of a Cauchy matrix is invertible, so the result is the unique one nanors produces.

/// nanors `DATA_SHARDS_MAX`: data + parity shards per block.
pub const MAX_SHARDS: usize = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FecError {
    /// Shard counts nanors rejects (`ds` or `ps` zero, or `ds + ps > 255`) or a slice of the
    /// wrong length.
    BadScheme,
    /// Present shards differ in length.
    ShardSize,
    /// More shards missing than parity can recover.
    TooManyLosses,
}

impl std::fmt::Display for FecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            FecError::BadScheme => "invalid reed-solomon shard counts",
            FecError::ShardSize => "reed-solomon shards differ in size",
            FecError::TooManyLosses => "too many shards lost to recover",
        })
    }
}

impl std::error::Error for FecError {}

const fn tables() -> ([u8; 256], [u8; 512]) {
    let (mut log, mut exp) = ([0u8; 256], [0u8; 512]);
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 255 {
        exp[i] = x as u8;
        exp[i + 255] = x as u8;
        log[x as usize] = i as u8;
        x <<= 1;
        if x & 0x100 != 0 {
            x ^= 0x11d;
        }
        i += 1;
    }
    (log, exp)
}

const LOG_EXP: ([u8; 256], [u8; 512]) = tables();

const fn mul_table() -> [[u8; 256]; 256] {
    let (log, exp) = LOG_EXP;
    let mut t = [[0u8; 256]; 256];
    let mut a = 1;
    while a < 256 {
        let mut b = 1;
        while b < 256 {
            t[a][b] = exp[log[a] as usize + log[b] as usize];
            b += 1;
        }
        a += 1;
    }
    t
}

static MUL: [[u8; 256]; 256] = mul_table();

fn inv(a: u8) -> u8 {
    debug_assert!(a != 0);
    LOG_EXP.1[255 - LOG_EXP.0[a as usize] as usize]
}

/// nanors parity-matrix coefficient for parity row `j`, data column `i`.
fn coef(ps: usize, j: usize, i: usize) -> u8 {
    inv(((ps + i) ^ j) as u8)
}

/// `acc ^= u * src`, bytewise.
fn axpy(acc: &mut [u8], src: &[u8], u: u8) {
    let row = &MUL[u as usize];
    for (a, b) in acc.iter_mut().zip(src) {
        *a ^= row[*b as usize];
    }
}

fn check(ds: usize, ps: usize, n: usize) -> Result<(), FecError> {
    if ds == 0 || ps == 0 || ds + ps > MAX_SHARDS || n != ds + ps {
        return Err(FecError::BadScheme);
    }
    Ok(())
}

/// Fills `shards[ds..]` with parity computed from `shards[..ds]` (nanors `reed_solomon_encode`).
pub fn rs_encode(
    data_shards: usize,
    parity_shards: usize,
    shards: &mut [Vec<u8>],
) -> Result<(), FecError> {
    check(data_shards, parity_shards, shards.len())?;
    let bs = shards[0].len();
    if shards.iter().any(|s| s.len() != bs) {
        return Err(FecError::ShardSize);
    }
    let (data, parity) = shards.split_at_mut(data_shards);
    for (j, p) in parity.iter_mut().enumerate() {
        p.fill(0);
        for (i, d) in data.iter().enumerate() {
            axpy(p, d, coef(parity_shards, j, i));
        }
    }
    Ok(())
}

/// Recovers missing (`None`) data shards in place from any `data_shards` present shards. Missing
/// parity shards stay `None`, as in nanors. On error nothing is modified.
pub fn rs_decode(
    data_shards: usize,
    parity_shards: usize,
    shards: &mut [Option<Vec<u8>>],
) -> Result<(), FecError> {
    check(data_shards, parity_shards, shards.len())?;
    let mut present = shards.iter().flatten();
    let bs = present.next().ok_or(FecError::TooManyLosses)?.len();
    if present.any(|s| s.len() != bs) {
        return Err(FecError::ShardSize);
    }
    let lost: Vec<usize> = (0..data_shards).filter(|&i| shards[i].is_none()).collect();
    let rows: Vec<usize> = (0..parity_shards)
        .filter(|&j| shards[data_shards + j].is_some())
        .take(lost.len())
        .collect();
    if rows.len() < lost.len() {
        return Err(FecError::TooManyLosses);
    }
    let w = lost.len();
    if w == 0 {
        return Ok(());
    }

    // rhs_k = parity_{rows[k]} - sum over present data of P[rows[k]][i] * d_i
    let mut rhs: Vec<Vec<u8>> = rows
        .iter()
        .map(|&j| shards[data_shards + j].clone().unwrap())
        .collect();
    for (k, &j) in rows.iter().enumerate() {
        for (i, d) in shards[..data_shards].iter().enumerate() {
            if let Some(d) = d {
                axpy(&mut rhs[k], d, coef(parity_shards, j, i));
            }
        }
    }

    // Gauss-Jordan invert m (w x w, m[k][e] = P[rows[k]][lost[e]]) into minv.
    let mut m: Vec<Vec<u8>> = rows
        .iter()
        .map(|&j| lost.iter().map(|&e| coef(parity_shards, j, e)).collect())
        .collect();
    let mut minv: Vec<Vec<u8>> = (0..w)
        .map(|r| (0..w).map(|c| u8::from(r == c)).collect())
        .collect();
    for c in 0..w {
        // A Cauchy submatrix is nonsingular, so a pivot always exists.
        let p = (c..w).find(|&r| m[r][c] != 0).ok_or(FecError::BadScheme)?;
        m.swap(c, p);
        minv.swap(c, p);
        let s = inv(m[c][c]);
        for x in 0..w {
            m[c][x] = MUL[s as usize][m[c][x] as usize];
            minv[c][x] = MUL[s as usize][minv[c][x] as usize];
        }
        for r in 0..w {
            let f = m[r][c];
            if r != c && f != 0 {
                let (mc, ic) = (m[c].clone(), minv[c].clone());
                axpy(&mut m[r], &mc, f);
                axpy(&mut minv[r], &ic, f);
            }
        }
    }

    for (e, &idx) in lost.iter().enumerate() {
        let mut out = vec![0u8; bs];
        for (k, r) in rhs.iter().enumerate() {
            axpy(&mut out, r, minv[e][k]);
        }
        shards[idx] = Some(out);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_matches_nanors_tables() {
        // First entries of nanors deps/obl/gf2_8_tables.h (polynomial 285 = 0x11d).
        assert_eq!(&LOG_EXP.0[1..8], &[0, 1, 25, 2, 50, 26, 198]);
        for a in 1..=255u8 {
            assert_eq!(MUL[a as usize][inv(a) as usize], 1);
        }
    }
}
