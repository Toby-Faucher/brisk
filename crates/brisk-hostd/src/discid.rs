//! Identify a game file by its header, with no emulator. Reads only the bytes it needs (a few KB
//! of header; the cartridge hash is streamed) and checks every offset against the file size.
//! Facts and gaps: docs/research/discid-notes.md.
//!
//! The cartridge `hash` is a plain SHA-1 of the file after a known copier header (NES 16-byte iNES
//! header, SNES 512-byte header) is skipped. It is NOT the RetroAchievements hash (that is 32 hex
//! chars, so MD5-sized, and has per-system rules we have not confirmed).

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    Wii,
    GameCube,
    NES,
    SNES,
    MegaDrive,
    GB,
    GBC,
    GBA,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Iso,
    Wbfs,
    Wia,
    Rvz,
    /// A bare cartridge dump.
    Rom,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Usa,
    Pal,
    Japan,
    Korea,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub system: System,
    pub game_id: Option<String>,
    pub title: Option<String>,
    pub region: Option<Region>,
    pub container: Container,
    pub hash: Option<String>,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    NotRegularFile,
    /// A known format whose header or offsets run past the end of the file.
    Truncated,
    /// A known format with impossible content (non-ASCII game ID, absurd WBFS shift, empty disc table).
    Invalid(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "read failed: {e}"),
            Error::NotRegularFile => write!(f, "not a regular file"),
            Error::Truncated => write!(f, "file is truncated"),
            Error::Invalid(why) => write!(f, "invalid header: {why}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

const WII_MAGIC: [u8; 4] = [0x5D, 0x1C, 0x9E, 0xA3];
const GC_MAGIC: [u8; 4] = [0xC2, 0x33, 0x9F, 0x3D];
const DISC_HEADER_LEN: usize = 0x60; // ID, magics and the 64-byte title
const READ_LEN: u64 = 4096;
const WIA_DHEAD: usize = 0x58; // 0x48 (WIAHeader1) + 16 bytes of WIAHeader2 before disc_header
const WBFS_DISC_HEADER: usize = 0x100;

fn c_text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

fn region_of(c: u8) -> Region {
    match c {
        b'E' => Region::Usa,
        b'P' | b'D' | b'F' | b'I' | b'S' | b'U' | b'X' | b'Y' | b'H' => Region::Pal,
        b'J' => Region::Japan,
        b'K' | b'Q' | b'T' => Region::Korea,
        _ => Region::Other,
    }
}

fn identify_disc(b: &[u8], container: Container) -> Result<Option<Identity>, Error> {
    if b.len() < DISC_HEADER_LEN {
        return Err(Error::Truncated);
    }
    let system = if b[0x18..0x1C] == WII_MAGIC {
        System::Wii
    } else if b[0x1C..0x20] == GC_MAGIC {
        System::GameCube
    } else {
        return Ok(None);
    };
    let id = &b[..6];
    if !id.iter().all(u8::is_ascii_alphanumeric) {
        return Err(Error::Invalid("game ID is not ASCII letters and digits"));
    }
    Ok(Some(Identity {
        system,
        game_id: Some(String::from_utf8_lossy(id).into_owned()),
        title: Some(c_text(&b[0x20..0x60])),
        region: Some(region_of(b[3])),
        container,
        hash: None,
    }))
}

fn unknown() -> Identity {
    Identity {
        system: System::Unknown,
        game_id: None,
        title: None,
        region: None,
        container: Container::Unknown,
        hash: None,
    }
}

/// Opens a regular file for reading. A FIFO or device is refused before the open, and the open
/// itself cannot block (O_NONBLOCK; the handle is checked again in case the path was swapped).
pub(crate) fn open_regular(path: &Path) -> Result<File, Error> {
    use std::os::unix::fs::OpenOptionsExt;
    if !std::fs::metadata(path)?.is_file() {
        return Err(Error::NotRegularFile);
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    if !f.metadata()?.is_file() {
        return Err(Error::NotRegularFile);
    }
    Ok(f)
}

/// Identify `path`. Refuses anything but a regular file.
pub fn identify(path: &Path) -> Result<Identity, Error> {
    identify_as(path, None)
}

/// `identify`, taking the extension for the header-less cartridge fallback from `ext` instead of
/// the path (an upload sits in a `.part` file until it is placed).
pub fn identify_as(path: &Path, ext: Option<&str>) -> Result<Identity, Error> {
    let mut f = open_regular(path)?;
    let len = f.metadata()?.len();
    let mut head = Vec::new();
    (&mut f).take(READ_LEN).read_to_end(&mut head)?;
    let ext = ext
        .or_else(|| path.extension().and_then(|e| e.to_str()))
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if head.starts_with(b"WBFS") {
        return wbfs(&mut f, &head, len);
    }
    if head.starts_with(b"WIA\x01") || head.starts_with(b"RVZ\x01") {
        let c = if head[0] == b'W' {
            Container::Wia
        } else {
            Container::Rvz
        };
        let dhead = head
            .get(WIA_DHEAD..WIA_DHEAD + 0x80)
            .ok_or(Error::Truncated)?;
        return identify_disc(dhead, c)?.ok_or(Error::Invalid("no disc magic in WIA/RVZ header"));
    }
    if head.len() >= DISC_HEADER_LEN
        && let Some(id) = identify_disc(&head, Container::Iso)?
    {
        return Ok(id);
    }
    cartridge(&mut f, &head, len, &ext)
}

fn wbfs(f: &mut File, head: &[u8], len: u64) -> Result<Identity, Error> {
    // Layout verified against Dolphin WbfsBlob.h: magic, u32 BE sector count, u8 hd shift, u8 wbfs
    // shift, 2 pad, disc_table[500]. Disc 0's info starts at one HD sector; its first 0x100 bytes are the disc header.
    if head.len() < 0x0C + 1 {
        return Err(Error::Truncated);
    }
    let shift = head[8];
    if !(9..=24).contains(&shift) {
        return Err(Error::Invalid("WBFS HD sector shift out of range"));
    }
    if head[0x0C] == 0 {
        return Err(Error::Invalid("WBFS disc slot 0 is empty"));
    }
    let off = 1u64 << shift;
    if off
        .checked_add(WBFS_DISC_HEADER as u64)
        .is_none_or(|end| end > len)
    {
        return Err(Error::Truncated);
    }
    let mut buf = [0u8; WBFS_DISC_HEADER];
    if off + WBFS_DISC_HEADER as u64 <= head.len() as u64 {
        buf.copy_from_slice(&head[off as usize..off as usize + WBFS_DISC_HEADER]);
    } else {
        f.seek(SeekFrom::Start(off))?;
        f.read_exact(&mut buf)?;
    }
    identify_disc(&buf, Container::Wbfs)?.ok_or(Error::Invalid("no disc magic in WBFS header copy"))
}

fn cartridge(f: &mut File, h: &[u8], len: u64, ext: &str) -> Result<Identity, Error> {
    let mut id = unknown();
    id.container = Container::Rom;
    let mut skip = 0u64;
    if h.starts_with(b"NES\x1A") {
        id.system = System::NES;
        skip = 16;
    } else if h.len() >= 0xC0 && h[4..8] == [0x24, 0xFF, 0xAE, 0x51] && h[0xB2] == 0x96 {
        id.system = System::GBA;
        id.title = Some(c_text(&h[0xA0..0xAC]));
        let code = &h[0xAC..0xB0];
        if code.iter().all(u8::is_ascii_alphanumeric) {
            id.game_id = Some(String::from_utf8_lossy(code).into_owned());
            id.region = Some(region_of(code[3]));
        }
    } else if h.len() >= 0x150 && h[0x104..0x108] == [0xCE, 0xED, 0x66, 0x66] {
        id.system = if matches!(h[0x143], 0x80 | 0xC0) {
            System::GBC
        } else {
            System::GB
        };
        id.title = Some(c_text(&h[0x134..0x13F]));
    } else if h.len() >= 0x110 && &h[0x100..0x104] == b"SEGA" {
        id.system = System::MegaDrive;
        if h.len() >= 0x180 {
            id.title = Some(c_text(&h[0x150..0x180]));
        }
    } else {
        match ext {
            "nes" => id.system = System::NES,
            "sfc" | "smc" => id.system = System::SNES,
            "gb" => id.system = System::GB,
            "gbc" => id.system = System::GBC,
            "gba" => id.system = System::GBA,
            "md" | "gen" | "smd" => id.system = System::MegaDrive,
            _ => return Ok(unknown()),
        }
    }
    if id.system == System::SNES && len % 1024 == 512 {
        skip = 512;
    }
    if skip > len {
        return Err(Error::Truncated);
    }
    f.seek(SeekFrom::Start(skip))?;
    id.hash = Some(sha1_hex(f)?);
    Ok(id)
}

fn sha1_hex(r: &mut impl Read) -> io::Result<String> {
    let mut h = Sha1::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(h.finish().iter().map(|b| format!("{b:02x}")).collect());
        }
        h.update(&buf[..n]);
    }
}

struct Sha1 {
    s: [u32; 5],
    block: [u8; 64],
    fill: usize,
    total: u64,
}

impl Sha1 {
    fn new() -> Self {
        Sha1 {
            s: [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0],
            block: [0; 64],
            fill: 0,
            total: 0,
        }
    }
    fn update(&mut self, mut d: &[u8]) {
        self.total += d.len() as u64;
        while !d.is_empty() {
            let n = (64 - self.fill).min(d.len());
            self.block[self.fill..self.fill + n].copy_from_slice(&d[..n]);
            self.fill += n;
            d = &d[n..];
            if self.fill == 64 {
                let b = self.block;
                self.compress(&b);
                self.fill = 0;
            }
        }
    }
    fn finish(mut self) -> [u8; 20] {
        let bits = self.total * 8;
        self.update(&[0x80]);
        while self.fill != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut out = [0u8; 20];
        for (i, w) in self.s.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }
    fn compress(&mut self, b: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b_, mut c, mut d, mut e] = self.s;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b_ & c) | (!b_ & d), 0x5A827999),
                20..=39 => (b_ ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b_ & c) | (b_ & d) | (c & d), 0x8F1BBCDC),
                _ => (b_ ^ c ^ d, 0xCA62C1D6u32),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b_.rotate_left(30);
            b_ = a;
            a = t;
        }
        for (s, v) in self.s.iter_mut().zip([a, b_, c, d, e]) {
            *s = s.wrapping_add(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A plain disc header, with "no magic" turned into an error.
    fn identify_header(b: &[u8]) -> Result<Identity, Error> {
        identify_disc(b, Container::Iso)?.ok_or(Error::Invalid("no GameCube or Wii magic"))
    }

    #[test]
    fn a_fifo_is_refused_without_blocking_and_the_extension_can_be_given() {
        use std::os::unix::ffi::OsStrExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.iso");
        let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
        // SAFETY: c is a valid NUL-terminated path for the whole call.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert!(matches!(identify(&p), Err(Error::NotRegularFile)));
        assert!(matches!(open_regular(&p), Err(Error::NotRegularFile)));
        // a header-less SNES dump named .part: the extension comes from the caller
        let q = d.path().join("a.part");
        std::fs::write(&q, vec![1u8; 2048]).unwrap();
        assert_eq!(identify(&q).unwrap().system, System::Unknown);
        assert_eq!(identify_as(&q, Some("SFC")).unwrap().system, System::SNES);
    }

    fn disc(id: &[u8; 6], wii: bool, title: &str) -> Vec<u8> {
        let mut h = vec![0u8; 0x60];
        h[..6].copy_from_slice(id);
        if wii {
            h[0x18..0x1C].copy_from_slice(&WII_MAGIC)
        } else {
            h[0x1C..0x20].copy_from_slice(&GC_MAGIC)
        }
        h[0x20..0x20 + title.len()].copy_from_slice(title.as_bytes());
        h
    }
    fn write(dir: &tempfile::TempDir, name: &str, data: &[u8]) -> std::path::PathBuf {
        let p = dir.path().join(name);
        File::create(&p).unwrap().write_all(data).unwrap();
        p
    }
    fn wbfs_file(h: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; 0x200 + 0x100];
        f[..4].copy_from_slice(b"WBFS");
        f[8] = 9; // 512-byte HD sectors
        f[9] = 15;
        f[0x0C] = 1;
        f[0x200..0x200 + h.len()].copy_from_slice(h);
        f
    }
    fn rvz_file(magic: &[u8; 4], h: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; WIA_DHEAD + 0x80];
        f[..4].copy_from_slice(magic);
        f[WIA_DHEAD..WIA_DHEAD + h.len()].copy_from_slice(h);
        f
    }

    #[test]
    fn sha1_vectors() {
        let hex = |d: &[u8]| {
            let mut h = Sha1::new();
            h.update(d);
            h.finish()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert_eq!(hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        let mut h = Sha1::new();
        for _ in 0..1000 {
            h.update(&[b'a'; 1000]);
        }
        let s: String = h.finish().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(s, "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    #[test]
    fn wii_iso_header() {
        let id = identify_header(&disc(b"RMCE01", true, "Mario Kart Wii")).unwrap();
        assert_eq!(id.system, System::Wii);
        assert_eq!(id.game_id.as_deref(), Some("RMCE01"));
        assert_eq!(id.title.as_deref(), Some("Mario Kart Wii"));
        assert_eq!(id.region, Some(Region::Usa));
        assert_eq!(id.container, Container::Iso);
    }

    #[test]
    fn gamecube_header() {
        let id = identify_header(&disc(b"GALP01", false, "Super Smash")).unwrap();
        assert_eq!(id.system, System::GameCube);
        assert_eq!(id.region, Some(Region::Pal));
    }

    #[test]
    fn bad_headers() {
        assert!(matches!(identify_header(&[0u8; 10]), Err(Error::Truncated)));
        assert!(matches!(
            identify_header(&[0u8; 0x60]),
            Err(Error::Invalid(_))
        ));
        let mut h = disc(b"RMCE01", true, "x");
        h[1] = 0xC3;
        assert!(matches!(identify_header(&h), Err(Error::Invalid(_))));
    }

    #[test]
    fn files() {
        let d = tempfile::tempdir().unwrap();
        let h = disc(b"RMCE01", true, "Mario Kart Wii");
        let iso = identify(&write(&d, "a.iso", &h)).unwrap();
        assert_eq!(iso.container, Container::Iso);
        let w = identify(&write(&d, "a.wbfs", &wbfs_file(&h))).unwrap();
        assert_eq!(
            (w.system, w.container, w.game_id.as_deref()),
            (System::Wii, Container::Wbfs, Some("RMCE01"))
        );
        let r = identify(&write(&d, "a.rvz", &rvz_file(b"RVZ\x01", &h))).unwrap();
        assert_eq!((r.system, r.container), (System::Wii, Container::Rvz));
        let wia = identify(&write(
            &d,
            "a.wia",
            &rvz_file(b"WIA\x01", &disc(b"GALE01", false, "g")),
        ))
        .unwrap();
        assert_eq!(
            (wia.system, wia.container),
            (System::GameCube, Container::Wia)
        );
    }

    #[test]
    fn hostile_inputs() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            identify(&write(&d, "empty", b"")).unwrap().system,
            System::Unknown
        );
        assert_eq!(
            identify(&write(&d, "junk", &[0xAB; 5000])).unwrap().system,
            System::Unknown
        );
        assert!(matches!(identify(d.path()), Err(Error::NotRegularFile)));
        assert!(matches!(
            identify(&d.path().join("missing")),
            Err(Error::Io(_))
        ));
        assert!(matches!(
            identify(&write(&d, "t.wbfs", b"WBFS")),
            Err(Error::Truncated)
        ));
        let mut w = wbfs_file(&disc(b"RMCE01", true, ""));
        w[8] = 40; // absurd shift
        assert!(matches!(
            identify(&write(&d, "s.wbfs", &w)),
            Err(Error::Invalid(_))
        ));
        let mut w = wbfs_file(&disc(b"RMCE01", true, ""));
        w[0x0C] = 0;
        assert!(matches!(
            identify(&write(&d, "e.wbfs", &w)),
            Err(Error::Invalid(_))
        ));
        let mut h = disc(b"RMCE01", true, "");
        h[2] = 0xFF;
        assert!(matches!(
            identify(&write(&d, "n.iso", &h)),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn truncations_never_panic() {
        let d = tempfile::tempdir().unwrap();
        let h = disc(b"RMCE01", true, "Mario Kart Wii");
        let mut gba = vec![0u8; 0x200];
        gba[4..8].copy_from_slice(&[0x24, 0xFF, 0xAE, 0x51]);
        gba[0xB2] = 0x96;
        let mut fixtures = vec![wbfs_file(&h), rvz_file(b"RVZ\x01", &h), h, gba];
        fixtures.push([&b"NES\x1A"[..], &[0u8; 32]].concat());
        let mut seed = 0x1234_5678u32;
        for f in &fixtures {
            for _ in 0..200 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let cut = (seed >> 8) as usize % (f.len() + 1);
                let mut data = f[..cut].to_vec();
                if !data.is_empty() {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let i = (seed >> 8) as usize % data.len();
                    data[i] ^= (seed >> 24) as u8;
                }
                let _ = identify(&write(&d, "f.bin", &data));
                let _ = identify_header(&data);
            }
        }
    }

    #[test]
    fn cartridges() {
        let d = tempfile::tempdir().unwrap();
        let body = vec![7u8; 100];
        let want = {
            let mut s = Sha1::new();
            s.update(&body);
            s.finish()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let with = [&b"NES\x1A"[..], &[0u8; 12], &body].concat();
        let a = identify(&write(&d, "a.nes", &with)).unwrap();
        assert_eq!(
            (a.system, a.hash.as_deref()),
            (System::NES, Some(want.as_str()))
        );
        let headerless = identify(&write(&d, "b.nes", &body)).unwrap();
        assert_eq!(headerless.hash.as_deref(), Some(want.as_str()));
        let snes = [&[0u8; 512][..], &vec![7u8; 512]].concat();
        let s = identify(&write(&d, "c.sfc", &snes)).unwrap();
        assert_eq!(s.system, System::SNES);
        let mut gb = vec![0u8; 0x150];
        gb[0x104..0x108].copy_from_slice(&[0xCE, 0xED, 0x66, 0x66]);
        gb[0x134..0x138].copy_from_slice(b"TEST");
        gb[0x143] = 0x80;
        let g = identify(&write(&d, "d.gb", &gb)).unwrap();
        assert_eq!((g.system, g.title.as_deref()), (System::GBC, Some("TEST")));
        let mut md = vec![0u8; 0x200];
        md[0x100..0x104].copy_from_slice(b"SEGA");
        assert_eq!(
            identify(&write(&d, "e.bin", &md)).unwrap().system,
            System::MegaDrive
        );
    }
}
