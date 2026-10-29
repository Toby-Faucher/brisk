//! Screenshots of emulator games, taken from the stream in the browser and stored here.
//!
//! `data_dir/shots/<game id with ':' as '_'>/<unix ms>.png`, files 0600, directories 0700. The
//! browser sends a PNG; we check the signature, the IHDR size and the IEND tail, never decode it.

use serde::Serialize;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

pub(crate) const MAX_BYTES: usize = 8 << 20;
const MAX_DIM: u32 = 8192;
const MAX_PER_GAME: usize = 200;
const MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const IEND: &[u8; 12] = b"\0\0\0\0IEND\xaeB`\x82";

#[derive(Debug, PartialEq)]
pub enum Fail {
    Invalid(&'static str),
    NoSpace,
    Io,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
    /// Unix ms, from the name.
    pub taken_at: u64,
}

/// Big-endian u32 at `i`; the caller has checked the length.
fn be32(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Width and height from a whole, sane PNG; an error for anything else.
fn png_dims(b: &[u8]) -> Result<(u32, u32), &'static str> {
    if b.len() < 8 + 25 || &b[..8] != MAGIC {
        return Err("not a PNG");
    }
    if b[8..12] != 13u32.to_be_bytes() || &b[12..16] != b"IHDR" {
        return Err("not a PNG");
    }
    let (w, h) = (be32(b, 16), be32(b, 20));
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err("image size out of range");
    }
    if &b[b.len() - 12..] != IEND {
        return Err("PNG is cut off");
    }
    Ok((w, h))
}

/// Only the names we generate (digits of a unix ms, then `.png`): the ms.
fn valid_name(n: &str) -> Option<u64> {
    let ms = n.strip_suffix(".png")?;
    let ok = (10..=16).contains(&ms.len())
        && ms.bytes().all(|b| b.is_ascii_digit())
        && !ms.starts_with('0');
    ok.then(|| ms.parse().ok()).flatten()
}

fn dir(data: &Path, id: &str) -> PathBuf {
    data.join("shots").join(id.replace(':', "_"))
}

/// Stores a PNG for game `id`; checks the image and the free space first.
pub fn save(
    data: &Path,
    id: &str,
    png: &[u8],
    now_ms: u64,
    free: Option<u64>,
) -> Result<Shot, Fail> {
    let (width, height) = png_dims(png).map_err(Fail::Invalid)?;
    if free.is_some_and(|f| (png.len() as u64).saturating_add(crate::files::RESERVE) > f) {
        return Err(Fail::NoSpace);
    }
    let d = dir(data, id);
    let mk = |p: &Path| {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(p)
    };
    mk(&d).map_err(|_| Fail::Io)?;
    // Two shots in one millisecond: take the next free number.
    let mut ms = now_ms;
    while d.join(format!("{ms}.png")).exists() {
        ms += 1;
    }
    let name = format!("{ms}.png");
    if let Err(e) = crate::fsutil::write_atomic(&d.join(&name), png) {
        return Err(if e.raw_os_error() == Some(libc::ENOSPC) {
            Fail::NoSpace
        } else {
            Fail::Io
        });
    }
    // Drop the oldest beyond the cap.
    let all = names(&d);
    for old in all.iter().skip(MAX_PER_GAME) {
        let _ = std::fs::remove_file(d.join(old));
    }
    Ok(Shot {
        name,
        width,
        height,
        size: png.len() as u64,
        taken_at: ms,
    })
}

/// File names, newest first.
fn names(d: &Path) -> Vec<String> {
    let mut v: Vec<(u64, String)> = std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().into_string().ok()?;
            Some((valid_name(&n)?, n))
        })
        .collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.0));
    v.into_iter().map(|x| x.1).collect()
}

/// The file names of the game's screenshots.
pub(crate) fn list_names(data: &Path, id: &str) -> Vec<String> {
    names(&dir(data, id))
}

/// The game's screenshots.
pub fn list(data: &Path, id: &str) -> Vec<Shot> {
    let d = dir(data, id);
    names(&d)
        .into_iter()
        .filter_map(|name| {
            let p = d.join(&name);
            let mut f = crate::discid::open_regular(&p).ok()?;
            let size = f.metadata().ok()?.len();
            let mut h = [0u8; 24];
            std::io::Read::read_exact(&mut f, &mut h).ok()?;
            Some(Shot {
                taken_at: valid_name(&name)?,
                name,
                width: be32(&h, 16),
                height: be32(&h, 20),
                size,
            })
        })
        .collect()
}

/// One screenshot's bytes, if `name` is a valid listed name.
pub fn read(data: &Path, id: &str, name: &str) -> Option<Vec<u8>> {
    valid_name(name)?;
    let f = crate::discid::open_regular(&dir(data, id).join(name)).ok()?;
    let mut v = Vec::new();
    // The cap guards a file that was swapped in after we wrote it.
    std::io::Read::read_to_end(&mut std::io::Read::take(f, MAX_BYTES as u64), &mut v).ok()?;
    Some(v)
}

/// Idempotent: a missing file is fine. False for a name that is not ours.
pub fn delete(data: &Path, id: &str, name: &str) -> bool {
    if valid_name(name).is_none() {
        return false;
    }
    let _ = std::fs::remove_file(dir(data, id).join(name));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    pub fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = MAGIC.to_vec();
        v.extend(13u32.to_be_bytes());
        v.extend(b"IHDR");
        v.extend(w.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]); // depth..interlace + crc (not checked)
        v.extend(IEND);
        v
    }

    #[test]
    fn validates_png() {
        assert_eq!(png_dims(&png(640, 480)), Ok((640, 480)));
        assert!(png_dims(b"GIF89a....................................").is_err());
        assert!(png_dims(&png(8193, 10)).is_err());
        assert!(png_dims(&png(10, 0)).is_err());
        let mut cut = png(10, 10);
        cut.truncate(cut.len() - 5);
        assert_eq!(png_dims(&cut), Err("PNG is cut off"));
        assert!(png_dims(&png(1, 1)[..20]).is_err());
        assert!(png_dims(b"").is_err());
    }

    #[test]
    fn names() {
        assert!(valid_name("1730000000000.png").is_some());
        for bad in [
            "../1730000000000.png",
            "1730000000000.png/",
            "a.png",
            "1730000000000.PNG",
            "1730000000000.png.png",
            "01730000000000.png",
            "123.png",
            "17300000000000000000.png",
            ".1730000000000.tmp",
            "1730000000000",
            "",
        ] {
            assert!(valid_name(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn save_list_cap_delete() {
        let t = tempfile::tempdir().unwrap();
        let id = "emu:dolphin:x";
        let s = save(t.path(), id, &png(320, 240), 1_730_000_000_000, None).unwrap();
        assert_eq!(
            (s.width, s.height, s.name.as_str()),
            (320, 240, "1730000000000.png")
        );
        let s2 = save(t.path(), id, &png(10, 10), 1_730_000_000_000, None).unwrap();
        assert_eq!(s2.name, "1730000000001.png"); // same ms: next number
        let l = list(t.path(), id);
        assert_eq!(
            l.iter().map(|x| x.taken_at).collect::<Vec<_>>(),
            vec![1_730_000_000_001, 1_730_000_000_000]
        );
        let d = dir(t.path(), id);
        assert_eq!(
            std::fs::metadata(&d).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(d.join(&s.name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(delete(t.path(), id, &s.name) && delete(t.path(), id, &s.name));
        assert!(!delete(t.path(), id, "../x"));
        assert_eq!(list_names(t.path(), id).len(), 1);
        // cap: the oldest go
        for i in 0..MAX_PER_GAME as u64 + 5 {
            save(t.path(), id, &png(2, 2), 1_740_000_000_000 + i, None).unwrap();
        }
        let n = list_names(t.path(), id);
        assert_eq!(n.len(), MAX_PER_GAME);
        assert_eq!(
            n[0],
            format!("{}.png", 1_740_000_000_000u64 + MAX_PER_GAME as u64 + 4)
        );
        assert_eq!(
            save(t.path(), id, &png(2, 2), 1_750_000_000_000, Some(1000)),
            Err(Fail::NoSpace)
        );
        assert!(
            std::fs::read_dir(&d)
                .unwrap()
                .flatten()
                .all(|e| !e.file_name().to_string_lossy().ends_with(".tmp"))
        );
    }
}
