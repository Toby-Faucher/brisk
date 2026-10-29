use crate::config::Config;
use crate::steam::{account_id, steam_id};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Screenshots and cloud-save summary from the host's own Steam folder (no network).
#[derive(Serialize, Default, Debug)]
pub struct Local {
    /// File names, newest first, at most 60. Fetch each through `screenshot()`.
    pub screenshots: Vec<String>,
    /// None when the game has no cloud-save folder.
    pub saves: Option<SaveInfo>,
}

/// Counts only: save file names and contents never leave the host.
#[derive(Serialize, Debug)]
pub struct SaveInfo {
    pub files: u32,
    pub bytes: u64,
    /// Newest file mtime, unix s.
    pub modified: u64,
}

const MAX_SHOTS: usize = 60;
const MAX_SHOT_BYTES: u64 = 25 * 1024 * 1024;
/// Directory entries looked at per listing. `read_dir` order is arbitrary, so in a folder with more than this
/// many files the newest can be missed (fine in practice); the ones read are sorted newest first.
const MAX_ENTRIES: usize = 5000;
const SHOTS_TTL: Duration = Duration::from_secs(5);

/// Per-game screenshot listings kept for a few seconds, so a page of tiles (one request each) reads the
/// folder once. In `AppState`, so tests do not share it. Only ever holds file names.
pub struct ShotCache {
    ttl: Duration,
    m: Mutex<HashMap<u32, (Instant, Vec<String>)>>,
}

impl Default for ShotCache {
    fn default() -> Self {
        Self::new(SHOTS_TTL)
    }
}

impl ShotCache {
    /// A cache that keeps listings for `ttl`.
    pub fn new(ttl: Duration) -> Self {
        ShotCache {
            ttl,
            m: Mutex::new(HashMap::new()),
        }
    }

    /// Blocking: lists `dir` unless this game was listed less than the TTL ago (as of `now`).
    fn listing(&self, appid: u32, dir: &Path, now: Instant) -> Vec<String> {
        if let Some((at, v)) = self.m.lock().unwrap().get(&appid)
            && now.saturating_duration_since(*at) < self.ttl
        {
            return v.clone();
        }
        let v = list_shots(dir);
        let mut m = self.m.lock().unwrap();
        m.retain(|_, (at, _)| now.saturating_duration_since(*at) < self.ttl);
        m.insert(appid, (now, v.clone()));
        v
    }
}

/// `steam_dir/userdata/<account id>`; the id comes only from loginusers.vdf, never the client.
fn user_dir(cfg: &Config) -> Option<PathBuf> {
    let id = account_id(&steam_id(&cfg.steam_dir)?)?;
    Some(cfg.steam_dir.join("userdata").join(id.to_string()))
}

fn shots_dir(cfg: &Config, appid: u32) -> Option<PathBuf> {
    Some(user_dir(cfg)?.join(format!("760/remote/{appid}/screenshots")))
}

/// Content type for a plain image file name, else None (separators, NUL and `..` refused outright).
fn image_type(name: &str) -> Option<&'static str> {
    if name.contains(['/', '\\', '\0']) || name.contains("..") {
        return None;
    }
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        _ => None,
    }
}

/// Regular image files (no symlinks, no dirs such as `thumbnails/`, none over the size cap), newest first.
fn list_shots(dir: &Path) -> Vec<String> {
    list_shots_capped(dir, MAX_ENTRIES)
}

fn list_shots_capped(dir: &Path, cap: usize) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<(SystemTime, String)> = rd
        .flatten()
        .take(cap)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            image_type(&name)?;
            let m = std::fs::symlink_metadata(e.path()).ok()?;
            (m.is_file() && m.len() <= MAX_SHOT_BYTES)
                .then(|| (m.modified().unwrap_or(UNIX_EPOCH), name))
        })
        .collect();
    v.sort_by(|a, b| b.cmp(a));
    v.truncate(MAX_SHOTS);
    v.into_iter().map(|(_, n)| n).collect()
}

/// File count, bytes and newest mtime under `dir`, without following symlinks; stops at 10 000
/// entries or depth 8.
fn save_summary(dir: &Path) -> Option<SaveInfo> {
    std::fs::symlink_metadata(dir)
        .ok()?
        .is_dir()
        .then_some(())?;
    let mut s = SaveInfo {
        files: 0,
        bytes: 0,
        modified: 0,
    };
    let (mut stack, mut seen) = (vec![(dir.to_path_buf(), 0u32)], 0u32);
    while let Some((d, depth)) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            seen += 1;
            if seen > 10_000 {
                return Some(s);
            }
            let Ok(m) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            if m.is_dir() && depth < 8 {
                stack.push((e.path(), depth + 1));
            } else if m.is_file() {
                s.files += 1;
                s.bytes = s.bytes.saturating_add(m.len());
                let t = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
                s.modified = s.modified.max(t.map_or(0, |t| t.as_secs()));
            }
        }
    }
    Some(s)
}

/// Blocking fs walk: call from `spawn_blocking`.
pub fn local(cfg: &Config, cache: &ShotCache, appid: u32) -> Local {
    let Some(u) = user_dir(cfg) else {
        return Local::default();
    };
    Local {
        screenshots: cache.listing(
            appid,
            &u.join(format!("760/remote/{appid}/screenshots")),
            Instant::now(),
        ),
        saves: save_summary(&u.join(format!("{appid}/remote"))),
    }
}

/// `(content type, bytes)` for a screenshot `name` that is exactly one of `local()`'s names, else None.
/// The name is only joined onto the folder after it matched a listing entry; the open refuses a
/// symlink swapped in since (O_NOFOLLOW) and the read is capped. Blocking: call from `spawn_blocking`.
pub fn screenshot(
    cfg: &Config,
    cache: &ShotCache,
    appid: u32,
    name: &str,
) -> Option<(&'static str, Vec<u8>)> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let ct = image_type(name)?;
    let dir = shots_dir(cfg, appid)?;
    let listed = cache
        .listing(appid, &dir, Instant::now())
        .into_iter()
        .find(|n| n == name)?;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK) // NONBLOCK: a FIFO swapped in must not hang
        .open(dir.join(listed))
        .ok()?;
    let m = f.metadata().ok()?;
    if !m.is_file() || m.len() > MAX_SHOT_BYTES {
        return None;
    }
    let mut buf = Vec::with_capacity(m.len() as usize);
    f.take(MAX_SHOT_BYTES + 1).read_to_end(&mut buf).ok()?;
    (buf.len() as u64 <= MAX_SHOT_BYTES).then_some((ct, buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_huge_folder_is_capped_without_a_panic() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        for n in 0..MAX_ENTRIES + 10 {
            std::fs::write(d.join(format!("{n}.png")), b"x").unwrap();
        }
        assert_eq!(list_shots(d).len(), MAX_SHOTS);
        assert_eq!(list_shots_capped(d, 3).len(), 3); // the cut itself
    }

    #[test]
    fn two_requests_within_the_ttl_list_once_then_the_ttl_ends_it() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::write(d.join("a.png"), b"x").unwrap();
        let c = ShotCache::new(Duration::from_secs(5));
        let t0 = Instant::now();
        assert_eq!(c.listing(1, d, t0), ["a.png"]);
        std::fs::write(d.join("b.png"), b"x").unwrap();
        // inside the TTL: still the first listing (the folder was not read again)
        assert_eq!(c.listing(1, d, t0 + Duration::from_secs(4)), ["a.png"]);
        // another game has its own entry
        assert_eq!(c.listing(2, d, t0 + Duration::from_secs(4)).len(), 2);
        // after the TTL the new file shows
        assert_eq!(c.listing(1, d, t0 + Duration::from_secs(6)).len(), 2);
    }
}
