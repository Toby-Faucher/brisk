//! RetroAchievements hash of a plain GameCube ISO, following rcheevos `rc_hash_gamecube`
//! (`src/hash/rc_hash_disc.c`, `rc_hash_nintendo_disc_partition`). Wii, WBFS and RVZ are not
//! done here: see docs/research/ra-hash-notes.md.
//!
//! The hash is MD5 over: the first `0x2440 + 0x20 + apploader body + apploader trailer` bytes
//! (capped at 1 MiB), then the data of each of the 18 main.dol segments with a non-zero size.
//! The DOL header itself is not hashed. Unlike rcheevos, a read past the end of the file is an
//! error rather than a hash of whatever was read.

use crate::discid::{self, Error, System};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Streaming MD5 (RFC 1321), std only.
pub struct Md5 {
    state: [u32; 4],
    buf: [u8; 64],
    fill: usize,
    len: u64,
}

const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Md5 {
    pub fn new() -> Self {
        Md5 {
            state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476],
            buf: [0; 64],
            fill: 0,
            len: 0,
        }
    }

    fn block(&mut self, chunk: &[u8; 64]) {
        let mut m = [0u32; 16];
        for (i, w) in m.iter_mut().enumerate() {
            *w = u32::from_le_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        let [mut a, mut b, mut c, mut d] = self.state;
        for (i, &s) in S.iter().enumerate() {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            // K[i] = floor(2^32 * abs(sin(i + 1))), as in RFC 1321.
            let k = ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32;
            let t = a.wrapping_add(f).wrapping_add(k).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(t.rotate_left(s));
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d]) {
            *s = s.wrapping_add(v);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let n = (64 - self.fill).min(data.len());
            self.buf[self.fill..self.fill + n].copy_from_slice(&data[..n]);
            self.fill += n;
            data = &data[n..];
            if self.fill == 64 {
                let b = self.buf;
                self.block(&b);
                self.fill = 0;
            }
        }
    }

    /// Lowercase hex digest.
    pub fn finish_hex(mut self) -> String {
        let bits = self.len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.fill != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_le_bytes());
        self.state
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

const BASE_HEADER: u64 = 0x2440;
const APPLOADER_HEADER: u64 = 0x20;
const MAX_HEADER: u64 = 1024 * 1024;

fn be32(b: &[u8]) -> u64 {
    u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn read_at(f: &mut File, len: u64, off: u64, n: u64) -> Result<Vec<u8>, Error> {
    if off.checked_add(n).is_none_or(|end| end > len) {
        return Err(Error::Truncated);
    }
    f.seek(SeekFrom::Start(off))?;
    let mut v = vec![0; n as usize];
    f.read_exact(&mut v)?;
    Ok(v)
}

/// RA hash (MD5 hex) of a plain GameCube ISO/GCM, as rcheevos computes it for console 16.
pub fn ra_hash_gamecube_iso(path: &Path) -> Result<String, Error> {
    let id = discid::identify(path)?;
    if id.system != System::GameCube || id.container != discid::Container::Iso {
        return Err(Error::Invalid("not a plain GameCube disc image"));
    }
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut md5 = Md5::new();

    let sizes = read_at(&mut f, len, BASE_HEADER + 0x14, 8)?;
    let header_size =
        (BASE_HEADER + APPLOADER_HEADER + be32(&sizes) + be32(&sizes[4..])).min(MAX_HEADER);
    let header = read_at(&mut f, len, 0, header_size)?;
    md5.update(&header);
    let dol = be32(&header[0x420..]);

    let table = read_at(&mut f, len, dol, 0xD8)?;
    for ix in 0..18 {
        let (off, size) = (be32(&table[ix * 4..]), be32(&table[0x90 + ix * 4..]));
        if size == 0 {
            continue;
        }
        if off.checked_add(size).is_none_or(|end| end > len) {
            return Err(Error::Truncated);
        }
        f.seek(SeekFrom::Start(off))?;
        let mut left = size;
        let mut chunk = vec![0; left.min(1 << 20) as usize];
        while left > 0 {
            let n = left.min(chunk.len() as u64) as usize;
            f.read_exact(&mut chunk[..n])?;
            md5.update(&chunk[..n]);
            left -= n as u64;
        }
    }
    Ok(md5.finish_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md5hex(b: &[u8]) -> String {
        let mut m = Md5::new();
        m.update(b);
        m.finish_hex()
    }

    #[test]
    fn md5_rfc1321_vectors() {
        for (i, o) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(md5hex(i.as_bytes()), o);
        }
    }

    #[test]
    fn md5_streaming_matches_oneshot() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
        let mut m = Md5::new();
        for c in data.chunks(13) {
            m.update(c);
        }
        assert_eq!(m.finish_hex(), md5hex(&data));
    }

    fn put(img: &mut [u8], at: usize, v: &[u8]) {
        img[at..at + v.len()].copy_from_slice(v);
    }

    /// Header 0x24A0 bytes (apploader body 0x30, trailer 0x10), DOL at 0x3000 with a code
    /// segment (5 bytes at 0x3100) and a data segment, index 7 (3 bytes at 0x3200).
    fn image() -> Vec<u8> {
        let mut img = vec![0u8; 0x3300];
        put(&mut img, 0, b"GTESTA");
        put(&mut img, 0x1C, &[0xC2, 0x33, 0x9F, 0x3D]);
        put(&mut img, 0x20, b"Synthetic");
        put(&mut img, 0x420, &0x3000u32.to_be_bytes());
        put(&mut img, 0x2454, &0x30u32.to_be_bytes());
        put(&mut img, 0x2458, &0x10u32.to_be_bytes());
        put(&mut img, 0x2440 + 0x20, b"apploader");
        put(&mut img, 0x3000, &0x3100u32.to_be_bytes());
        put(&mut img, 0x3000 + 0x90, &5u32.to_be_bytes());
        put(&mut img, 0x3000 + 7 * 4, &0x3200u32.to_be_bytes());
        put(&mut img, 0x3000 + 0x90 + 7 * 4, &3u32.to_be_bytes());
        put(&mut img, 0x3100, b"CODE!");
        put(&mut img, 0x3200, b"dat");
        img
    }

    fn write(name: &str, img: &[u8]) -> std::path::PathBuf {
        let p =
            std::env::temp_dir().join(format!("brisk-rahash-{}-{name}.iso", std::process::id()));
        std::fs::write(&p, img).unwrap();
        p
    }

    #[test]
    fn gamecube_hash_follows_rcheevos_by_hand() {
        let img = image();
        let p = write("ok", &img);
        // By hand: 0x2440 + 0x20 + 0x30 + 0x10 header bytes, then segment 0, then segment 7.
        let mut want = Vec::new();
        want.extend_from_slice(&img[..0x24A0]);
        want.extend_from_slice(b"CODE!");
        want.extend_from_slice(b"dat");
        assert_eq!(ra_hash_gamecube_iso(&p).unwrap(), md5hex(&want));
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn rejects_truncated_and_hostile() {
        let img = image();
        // Truncated inside the last segment, inside the DOL table, inside the header, tiny.
        for (n, cut) in [
            ("seg", 0x3202),
            ("dol", 0x3050),
            ("hdr", 0x2480),
            ("tiny", 0x40),
        ] {
            let p = write(n, &img[..cut]);
            assert!(ra_hash_gamecube_iso(&p).is_err(), "{n}");
            std::fs::remove_file(p).ok();
        }
        // DOL offset past the end, segment size huge, apploader sizes huge (header cap, then EOF).
        for (n, at, v) in [
            ("dolpast", 0x420, 0xFFFF_FFF0u32),
            ("bigseg", 0x3000 + 0x90, u32::MAX),
            ("bigapp", 0x2454, u32::MAX),
        ] {
            let mut bad = image();
            put(&mut bad, at, &v.to_be_bytes());
            let p = write(n, &bad);
            assert!(ra_hash_gamecube_iso(&p).is_err(), "{n}");
            std::fs::remove_file(p).ok();
        }
        // A Wii disc is refused.
        let mut wii = image();
        put(&mut wii, 0x18, &[0x5D, 0x1C, 0x9E, 0xA3]);
        put(&mut wii, 0x1C, &[0; 4]);
        let p = write("wii", &wii);
        assert!(ra_hash_gamecube_iso(&p).is_err());
        std::fs::remove_file(p).ok();
    }
}
