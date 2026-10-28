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

/// K[i] = floor(2^32 * abs(sin(i + 1))), as in RFC 1321; computed once.
fn k_table() -> &'static [u32; 64] {
    static K: std::sync::OnceLock<[u32; 64]> = std::sync::OnceLock::new();
    K.get_or_init(|| std::array::from_fn(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32))
}

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
            let k = k_table()[i];
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
const WII_SECTOR: u64 = 0x8000;
const WII_DISC_SIZE: u64 = 143432 * 2 * WII_SECTOR; // what Dolphin reports for WBFS
const WBFS_TABLE_OFF: u64 = 0x100; // after the disc header copy, in disc 0's info sector

fn be32(b: &[u8]) -> u64 {
    u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// The raw disc image as Dolphin's `Volume::Read(.., PARTITION_NONE)` presents it: a plain
/// ISO, or a WBFS file decoded through its block map. Nothing is decrypted.
enum Disc {
    Iso(File, u64),
    Wbfs {
        f: File,
        shift: u32,
        table: Vec<u16>,
    },
}

impl Disc {
    fn open(path: &Path) -> Result<Disc, Error> {
        let mut f = discid::open_regular(path)?;
        let len = f.metadata()?.len();
        let mut h = [0u8; 0x0D];
        if f.read_exact(&mut h).is_err() || &h[..4] != b"WBFS" {
            return Ok(Disc::Iso(f, len));
        }
        let (hd_shift, shift) = (u32::from(h[8]), u32::from(h[9]));
        // Dolphin: file size must equal sector count << HD shift; WBFS sector >= one Wii sector.
        if !(9..=24).contains(&hd_shift) || !(15..=30).contains(&shift) {
            return Err(Error::Invalid("WBFS sector shift out of range"));
        }
        if be32(&h[4..]) << hd_shift != len {
            return Err(Error::Invalid("WBFS size does not match its header"));
        }
        if h[0x0C] == 0 {
            return Err(Error::Invalid("WBFS disc slot 0 is empty"));
        }
        let blocks = WII_DISC_SIZE.div_ceil(1 << shift);
        let at = (1u64 << hd_shift) + WBFS_TABLE_OFF;
        if at + blocks * 2 > len {
            return Err(Error::Truncated);
        }
        let mut raw = vec![0u8; blocks as usize * 2];
        f.seek(SeekFrom::Start(at))?;
        f.read_exact(&mut raw)?;
        let table = raw
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        Ok(Disc::Wbfs { f, shift, table })
    }

    fn size(&self) -> u64 {
        match self {
            Disc::Iso(_, len) => *len,
            Disc::Wbfs { .. } => WII_DISC_SIZE,
        }
    }

    /// Exactly `n` bytes at `off`, or `Truncated`. Callers keep `n` small.
    fn read_at(&mut self, off: u64, n: u64) -> Result<Vec<u8>, Error> {
        if off.checked_add(n).is_none_or(|end| end > self.size()) {
            return Err(Error::Truncated);
        }
        let mut out = vec![0; n as usize];
        match self {
            Disc::Iso(f, _) => {
                f.seek(SeekFrom::Start(off))?;
                f.read_exact(&mut out)?;
            }
            Disc::Wbfs { f, shift, table } => {
                let (mut pos, mut done) = (off, 0usize);
                while done < out.len() {
                    let inner = pos & ((1 << *shift) - 1);
                    let take = ((1u64 << *shift) - inner).min((out.len() - done) as u64) as usize;
                    let sector = *table
                        .get((pos >> *shift) as usize)
                        .ok_or(Error::Truncated)?;
                    // Like Dolphin, an unmapped block (0) reads from the start of the WBFS file.
                    let at = (u64::from(sector) << *shift) + inner;
                    f.seek(SeekFrom::Start(at))?;
                    f.read_exact(&mut out[done..done + take])
                        .map_err(|_| Error::Truncated)?;
                    done += take;
                    pos += take as u64;
                }
            }
        }
        Ok(out)
    }

    /// Hash `size` bytes at `off` in bounded chunks.
    fn hash_range(&mut self, md5: &mut Md5, off: u64, size: u64) -> Result<(), Error> {
        let mut done = 0;
        while done < size {
            let n = (size - done).min(1 << 20);
            md5.update(&self.read_at(off + done, n)?);
            done += n;
        }
        Ok(())
    }
}

/// rcheevos `rc_hash_nintendo_disc_partition`: `shift` is 0 for GameCube, 2 for a Wii partition.
fn hash_partition(d: &mut Disc, md5: &mut Md5, part: u64, shift: u32) -> Result<(), Error> {
    let sizes = d.read_at(part + BASE_HEADER + 0x14, 8)?;
    let header_size =
        (BASE_HEADER + APPLOADER_HEADER + be32(&sizes) + be32(&sizes[4..])).min(MAX_HEADER);
    let header = d.read_at(part, header_size)?;
    md5.update(&header);
    let dol = be32(&header[0x420..]) << shift;

    let table = d.read_at(part + dol, 0xD8)?;
    for ix in 0..18 {
        let (off, size) = (
            be32(&table[ix * 4..]) << shift,
            be32(&table[0x90 + ix * 4..]) << shift,
        );
        if size != 0 {
            d.hash_range(md5, part + off, size)?;
        }
    }
    Ok(())
}

/// RA hash (MD5 hex) of a plain GameCube ISO/GCM, as rcheevos computes it for console 16.
pub fn ra_hash_gamecube_iso(path: &Path) -> Result<String, Error> {
    let id = discid::identify(path)?;
    if id.system != System::GameCube || id.container != discid::Container::Iso {
        return Err(Error::Invalid("not a plain GameCube disc image"));
    }
    let mut d = Disc::open(path)?;
    let mut md5 = Md5::new();
    hash_partition(&mut d, &mut md5, 0, 0)?;
    Ok(md5.finish_hex())
}

/// RA hash (MD5 hex) of a Wii disc in an ISO or WBFS file, as rcheevos computes it for console
/// 19. Follows `rc_hash_wii_disc` as written, including that the partition data offset read at
/// +0x2B8 is used as an absolute disc offset. Not yet compared with a real disc's hash.
pub fn ra_hash_wii(path: &Path) -> Result<String, Error> {
    use discid::Container::{Iso, Wbfs};
    let id = discid::identify(path)?;
    if id.system != System::Wii || !matches!(id.container, Iso | Wbfs) {
        return Err(Error::Invalid("not an ISO or WBFS Wii disc image"));
    }
    let mut d = Disc::open(path)?;
    let mut md5 = Md5::new();
    let encrypted = d.read_at(0x61, 1)?[0] == 0;
    md5.update(&d.read_at(0, 0x80)?);
    md5.update(&d.read_at(0x4E000, 4)?);

    let info = d.read_at(0x40000, 32)?;
    let info: Vec<u64> = info.chunks(4).map(be32).collect();
    let total: u64 = info.iter().step_by(2).sum();
    if total == 0 || total > 16 {
        return Err(Error::Invalid("Wii partition count out of range"));
    }
    let mut parts = Vec::new();
    for g in info.chunks(2) {
        let raw = d.read_at(g[1] << 2, g[0] * 8)?;
        parts.extend(raw.chunks(8).map(|e| (be32(e) << 2, be32(&e[4..]))));
    }
    for (base, kind) in parts {
        if kind == 1 {
            continue; // update partition
        }
        let t = d.read_at(base + 0x2A4, 8)?;
        let (tmd_size, tmd_off) = (be32(&t).min(WII_SECTOR - 0x400), be32(&t[4..]) << 2);
        md5.update(&d.read_at(base + tmd_off, tmd_size)?);

        let p = d.read_at(base + 0x2B8, 8)?;
        let (part_off, part_size) = (be32(&p) << 2, be32(&p[4..]) << 2);
        if encrypted {
            // Raw bytes: skip each cluster's 0x400-byte hash block, take 0x7C00.
            for ix in 0..(part_size / WII_SECTOR).min(1024) {
                md5.update(&d.read_at(part_off + ix * WII_SECTOR + 0x400, 0x7C00)?);
            }
        } else {
            hash_partition(&mut d, &mut md5, part_off, 2)?;
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

    // ---- Wii ----

    const WII_LEN: usize = 0x70000;

    /// Wii disc, encrypted flag set (byte 0x61 = 0): one data partition (type 0) and one update
    /// partition (type 1). Partition 0 at 0x50000: TMD (6 bytes) at +0x400, cluster data offset
    /// 0x60000 (absolute, as rcheevos reads it), size 2 clusters. Filler is a byte pattern.
    fn wii_image() -> Vec<u8> {
        let mut img: Vec<u8> = (0..WII_LEN).map(|i| (i * 31 + i / 251) as u8).collect();
        put(&mut img, 0, b"RTESTA");
        put(&mut img, 0x18, &[0x5D, 0x1C, 0x9E, 0xA3]);
        img[0x61] = 0;
        // Partition info: group 0 has 2 partitions, table at 0x40020.
        put(&mut img, 0x40000, &2u32.to_be_bytes());
        put(&mut img, 0x40004, &(0x40020u32 >> 2).to_be_bytes());
        for g in 1..4 {
            put(&mut img, 0x40000 + g * 8, &[0; 8]);
        }
        put(&mut img, 0x40020, &(0x50000u32 >> 2).to_be_bytes());
        put(&mut img, 0x40024, &0u32.to_be_bytes());
        put(&mut img, 0x40028, &(0x58000u32 >> 2).to_be_bytes()); // update partition: skipped
        put(&mut img, 0x4002C, &1u32.to_be_bytes());
        put(&mut img, 0x50000 + 0x2A4, &6u32.to_be_bytes());
        put(&mut img, 0x50000 + 0x2A8, &(0x400u32 >> 2).to_be_bytes());
        put(&mut img, 0x50000 + 0x2B8, &(0x60000u32 >> 2).to_be_bytes());
        put(&mut img, 0x50000 + 0x2BC, &(0x10000u32 >> 2).to_be_bytes());
        img
    }

    fn wii_expected(img: &[u8]) -> String {
        let mut want = Vec::new();
        want.extend_from_slice(&img[..0x80]);
        want.extend_from_slice(&img[0x4E000..0x4E004]);
        want.extend_from_slice(&img[0x50400..0x50406]); // TMD of partition 0 only
        want.extend_from_slice(&img[0x60400..0x60400 + 0x7C00]);
        want.extend_from_slice(&img[0x68400..0x68400 + 0x7C00]);
        md5hex(&want)
    }

    /// Wrap a disc image in WBFS: 512-byte HD sectors, 64 KiB WBFS sectors, block i stored in
    /// sector 5 + (6 - i) so the block map is actually exercised.
    fn wbfs_wrap(disc: &[u8]) -> Vec<u8> {
        let blocks = disc.len() / 0x10000;
        let mut f = vec![0u8; (5 + blocks) * 0x10000];
        put(&mut f, 0, b"WBFS");
        let sectors = (f.len() >> 9) as u32;
        put(&mut f, 4, &sectors.to_be_bytes());
        f[8] = 9;
        f[9] = 16;
        f[0x0C] = 1;
        put(&mut f, 0x200, &disc[..0x100]);
        for i in 0..blocks {
            let sector = 5 + (blocks - 1 - i);
            put(&mut f, 0x300 + i * 2, &(sector as u16).to_be_bytes());
            put(
                &mut f,
                sector * 0x10000,
                &disc[i * 0x10000..(i + 1) * 0x10000],
            );
        }
        f
    }

    #[test]
    fn wii_hash_iso_and_wbfs_match_by_hand() {
        let img = wii_image();
        let want = wii_expected(&img);
        let p = write("wii-iso", &img);
        assert_eq!(ra_hash_wii(&p).unwrap(), want);
        let w = write("wii-wbfs", &wbfs_wrap(&img));
        assert_eq!(ra_hash_wii(&w).unwrap(), want);
        std::fs::remove_file(p).ok();
        std::fs::remove_file(w).ok();
    }

    #[test]
    fn wii_rejects_truncated_and_hostile() {
        let img = wii_image();
        let wb = wbfs_wrap(&img);
        let mut cases: Vec<(&str, Vec<u8>)> = vec![
            ("iso-short", img[..0x68000].to_vec()),
            ("iso-tiny", img[..0x70].to_vec()),
            ("wbfs-short", wb[..wb.len() - 0x10000].to_vec()),
            ("wbfs-tiny", wb[..0x210].to_vec()),
        ];
        let mut bad = img.clone();
        put(&mut bad, 0x40000, &[0; 4]); // no partitions
        cases.push(("nopart", bad));
        let mut bad = img.clone();
        put(&mut bad, 0x40000, &17u32.to_be_bytes()); // too many
        cases.push(("manypart", bad));
        let mut bad = img.clone();
        put(&mut bad, 0x40020, &u32::MAX.to_be_bytes()); // partition far past the end
        cases.push(("farpart", bad));
        let mut bad = img.clone();
        put(&mut bad, 0x50000 + 0x2BC, &u32::MAX.to_be_bytes()); // size is capped at 1024 clusters
        cases.push(("bigsize", bad));
        let mut bad = wb.clone();
        bad[9] = 40; // absurd WBFS shift
        cases.push(("wbfs-shift", bad));
        let mut bad = wb.clone();
        put(&mut bad, 4, &1u32.to_be_bytes()); // size mismatch
        cases.push(("wbfs-size", bad));
        let mut bad = wb.clone();
        put(&mut bad, 0x300, &u16::MAX.to_be_bytes()); // block map points past the file
        cases.push(("wbfs-map", bad));
        for (n, data) in cases {
            let p = write(n, &data);
            assert!(ra_hash_wii(&p).is_err(), "{n}");
            std::fs::remove_file(p).ok();
        }
    }

    /// Golden check against a real disc. Run by hand:
    /// `BRISK_RA_WII_FILE=/path/game.wbfs BRISK_RA_WII_HASH=<hash from dolphin-tool verify -a rchash>
    /// cargo test -p brisk-hostd rahash -- --ignored`.
    #[test]
    #[ignore = "needs a real Wii disc file and its golden hash"]
    fn wii_real_file_matches_golden() {
        let (Ok(file), Ok(want)) = (
            std::env::var("BRISK_RA_WII_FILE"),
            std::env::var("BRISK_RA_WII_HASH"),
        ) else {
            panic!("set BRISK_RA_WII_FILE and BRISK_RA_WII_HASH");
        };
        assert_eq!(ra_hash_wii(Path::new(&file)).unwrap(), want);
    }
}
