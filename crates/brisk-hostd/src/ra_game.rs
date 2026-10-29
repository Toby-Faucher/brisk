//! RetroAchievements for emulator games: disc file to RA hash (cached), and the JSON the
//! achievements route answers for them. Read-only; the key never appears here (see retroachievements.rs).
//!
//! The Wii hash (`rahash::ra_hash_wii`) is verified only against a synthetic image, not a real
//! disc. `GET /api/games/{id}/ra-hash` exists so the hash can be looked up on the RA site by hand.

use crate::config::Config;
use crate::discid::{self, Container, System};
use crate::game::Game;
use crate::rahash;
use crate::retroachievements::{
    self as ra, Achievement, CONSOLE_GAMECUBE, CONSOLE_WII, GameProgress, Matched, RaConfig,
    RaError,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CACHE_MAX: usize = 256;

impl Config {
    /// The RA client settings for this config (key file and base come from `Config`).
    pub fn ra(&self) -> RaConfig {
        let mut r = RaConfig::from_env(&self.data_dir);
        r.key_file = self.ra_key_file.clone();
        r.base = self.ra_base.clone();
        r.min_interval = self.ra_min_interval;
        r
    }
}

/// Wii and GameCube are the systems RA can serve; true only with the key file present.
pub fn caps_on(cfg: &Config, games: &mut [Game]) {
    let key = cfg.ra_key_file.is_file();
    for g in games {
        g.caps.achievements = key && matches!(g.system.as_deref(), Some("wii" | "gamecube"));
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HashInfo {
    /// RA console id (16 GameCube, 19 Wii).
    pub console: u32,
    pub hash: String,
    pub container: String,
    pub computed_at: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HashErr {
    /// A container or format `rahash` cannot decode (RVZ, WIA, unknown): never a guessed hash.
    Unsupported(String),
    Failed(String),
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Canonical path + size + mtime (ns): a changed or replaced file misses.
fn cache_key(file: &Path) -> Option<String> {
    let m = std::fs::metadata(file).ok()?;
    let mt = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!(
        "{}|{}|{}",
        std::fs::canonicalize(file).ok()?.display(),
        m.len(),
        mt.as_nanos()
    ))
}

/// Blocking: reads up to a GB on a miss, so callers use `spawn_blocking`.
pub fn hash_for(data_dir: &Path, file: &Path) -> Result<HashInfo, HashErr> {
    let cache = data_dir.join("ra/hash-cache.json");
    let key = cache_key(file).ok_or_else(|| HashErr::Failed("game file unreadable".into()))?;
    // A corrupt or missing cache is an empty one.
    let mut map: HashMap<String, HashInfo> =
        ra::read_cache(&cache, Duration::from_secs(u64::MAX / 2)).unwrap_or_default();
    if let Some(h) = map.get(&key) {
        return Ok(h.clone());
    }
    let id = discid::identify(file).map_err(|e| match e {
        discid::Error::Io(_) => HashErr::Failed("game file unreadable".into()),
        _ => HashErr::Unsupported("not a disc image brisk can read".into()),
    })?;
    let (console, hashed) = match (id.system, id.container) {
        (System::Wii, Container::Iso | Container::Wbfs) => (CONSOLE_WII, rahash::ra_hash_wii(file)),
        (System::GameCube, Container::Iso) => {
            (CONSOLE_GAMECUBE, rahash::ra_hash_gamecube_iso(file))
        }
        (System::Wii | System::GameCube, c) => {
            return Err(HashErr::Unsupported(format!(
                "{c:?} images cannot be hashed yet, use an ISO or WBFS file"
            )));
        }
        (System::ThreeDS, _) => {
            return Err(HashErr::Unsupported(
                "RetroAchievements has no 3DS support in Azahar".into(),
            ));
        }
        _ => {
            return Err(HashErr::Unsupported(
                "RetroAchievements here covers Wii and GameCube".into(),
            ));
        }
    };
    let hash = hashed.map_err(|e| match e {
        discid::Error::Io(_) => HashErr::Failed("game file unreadable".into()),
        _ => HashErr::Unsupported("disc layout brisk cannot hash".into()),
    })?;
    let info = HashInfo {
        console,
        hash,
        container: format!("{:?}", id.container).to_lowercase(),
        computed_at: now(),
    };
    if map.len() >= CACHE_MAX {
        let oldest = map
            .iter()
            .min_by_key(|(_, v)| v.computed_at)
            .map(|(k, _)| k.clone());
        if let Some(k) = oldest {
            map.remove(&k);
        }
    }
    map.insert(key, info.clone());
    let _ = ra::write_cache(&cache, &map); // a failed write only costs a rehash
    Ok(info)
}

/// "YYYY-MM-DD HH:MM:SS" (UTC) to unix seconds.
fn unix(s: &str) -> Option<u64> {
    let n: Vec<i64> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let [y, m, d, h, mi, sec] = n[..] else {
        return None;
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 24 || mi > 59 || sec > 60 {
        return None;
    }
    // checked: the digits come from a remote reply
    let y = if m <= 2 { y.checked_sub(1)? } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let days = era
        .checked_mul(146097)?
        .checked_add(yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468)?;
    let secs = days
        .checked_mul(86400)?
        .checked_add(h * 3600 + mi * 60 + sec)?;
    u64::try_from(secs).ok()
}

fn ach_json(a: &Achievement) -> Value {
    let lock = if a.unlocked.is_some() { "" } else { "_lock" };
    json!({
        "id": a.id.to_string(),
        "name": a.title,
        "description": a.description,
        "icon": format!("https://media.retroachievements.org/Badge/{}{lock}.png", a.badge_name),
        "unlockedAt": a.unlocked.as_deref().and_then(unix),
        "points": a.points,
        "hardcore": a.unlocked_hardcore.is_some(),
    })
}

/// Steam's `Achievements` shape (`available`, `unlocked`, `total`, `list` of `id name description
/// icon unlockedAt`) plus `state`, `source`, points and hardcore.
pub fn ok_json(p: &GameProgress, how: Matched) -> Value {
    let un = |a: &&Achievement| a.unlocked.is_some();
    json!({
        "state": "ok",
        "source": "retroachievements",
        "matched": if how == Matched::Title { "title" } else { "hash" },
        "available": true,
        "unlocked": p.achievements.iter().filter(un).count(),
        "total": p.achievements.len(),
        "unlockedHardcore": p.achievements.iter().filter(|a| a.unlocked_hardcore.is_some()).count(),
        "pointsUnlocked": p.achievements.iter().filter(un).map(|a| a.points).sum::<u32>(),
        "pointsTotal": p.achievements.iter().map(|a| a.points).sum::<u32>(),
        "gameId": p.game_id,
        "title": p.title,
        "url": format!("https://retroachievements.org/game/{}", p.game_id),
        "list": p.achievements.iter().map(ach_json).collect::<Vec<_>>(),
    })
}

/// Said when the file's hash is not in RA's list. On Wii the hash reads raw disc bytes past the
/// partition table, so a dump trimmed of its update partition (most WBFS files) cannot match.
fn not_found_reason(console: u32) -> &'static str {
    if console == CONSOLE_WII {
        "RetroAchievements supports Wii, but this file's hash is not in its list. A trimmed dump (for example a WBFS without the update partition) cannot match; a full ISO of the same game can."
    } else {
        "This file's hash is not in the RetroAchievements list. It may be a modified or trimmed dump."
    }
}

pub fn err_json(e: &RaError) -> Value {
    let state = match e {
        RaError::Off => "off",
        RaError::BadCredentials | RaError::Unauthorized => "bad-credentials",
        RaError::NotFound => "not-found",
        RaError::RateLimited => "rate-limited",
        _ => "error",
    };
    json!({ "state": state })
}

/// The route body for one emulator game. `fresh`: drop the cached progress first.
pub async fn state_json(cfg: &Config, file: &Path, names: &[String], fresh: bool) -> Value {
    let rc = cfg.ra();
    if !rc.key_file.is_file() {
        return json!({ "state": "off" });
    }
    match hash_blocking(cfg, file).await {
        Err(HashErr::Unsupported(reason)) => json!({ "state": "unsupported", "reason": reason }),
        Err(HashErr::Failed(_)) => json!({ "state": "error" }),
        Ok(h) => match ra::progress_for_names(&rc, h.console, &h.hash, names, fresh).await {
            Ok((p, how)) => ok_json(&p, how),
            Err(RaError::NotFound) => {
                json!({ "state": "not-found", "reason": not_found_reason(h.console) })
            }
            Err(e) => err_json(&e),
        },
    }
}

pub async fn hash_blocking(cfg: &Config, file: &Path) -> Result<HashInfo, HashErr> {
    let (d, f) = (cfg.data_dir.clone(), file.to_path_buf());
    tokio::task::spawn_blocking(move || hash_for(&d, &f))
        .await
        .unwrap_or_else(|_| Err(HashErr::Failed("hash task failed".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("brisk-ragame-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn gc(file: &Path, extra: u8) {
        let mut img = vec![0u8; 0x3300];
        img[..6].copy_from_slice(b"GTESTA");
        img[0x1C..0x20].copy_from_slice(&[0xC2, 0x33, 0x9F, 0x3D]);
        img[0x420..0x424].copy_from_slice(&0x3000u32.to_be_bytes());
        img[0x100] = extra;
        std::fs::write(file, img).unwrap();
    }

    #[test]
    fn hash_cache_hits_misses_and_follows_mtime() {
        let d = tmp("cache");
        let f = d.join("g.iso");
        gc(&f, 0);
        let a = hash_for(&d, &f).unwrap();
        assert_eq!(
            (a.console, a.hash.len(), a.container.as_str()),
            (16, 32, "iso")
        );
        // Hit: corrupt the file in place but restore size and mtime; the cached answer returns.
        let m = std::fs::metadata(&f).unwrap().modified().unwrap();
        gc(&f, 9);
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(m)
            .unwrap();
        assert_eq!(hash_for(&d, &f).unwrap(), a);
        // Miss: a new mtime recomputes.
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(m + Duration::from_secs(5))
            .unwrap();
        assert_eq!(hash_for(&d, &f).unwrap().hash.len(), 32);
        assert_ne!(hash_for(&d, &f).unwrap().hash, a.hash);
        // The cache file is private and a corrupt one is tolerated.
        let c = d.join("ra/hash-cache.json");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&c).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::write(&c, "{{{").unwrap();
        assert!(hash_for(&d, &f).is_ok());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn rvz_and_junk_are_unsupported_never_hashed() {
        let d = tmp("rvz");
        let f = d.join("g.rvz");
        let mut b = vec![0u8; 0x200];
        b[..4].copy_from_slice(b"RVZ\x01");
        b[0x58..0x5E].copy_from_slice(b"GTESTA");
        b[0x58 + 0x1C..0x58 + 0x20].copy_from_slice(&[0xC2, 0x33, 0x9F, 0x3D]);
        std::fs::write(&f, b).unwrap();
        assert!(matches!(hash_for(&d, &f), Err(HashErr::Unsupported(_))));
        std::fs::write(&f, b"nothing").unwrap();
        assert!(matches!(hash_for(&d, &f), Err(HashErr::Unsupported(_))));
        assert!(matches!(
            hash_for(&d, &d.join("missing")),
            Err(HashErr::Failed(_))
        ));
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn dates_parse_to_unix() {
        assert_eq!(unix("1970-01-02 00:00:01"), Some(86401));
        assert_eq!(unix("2026-03-12 17:47:29"), Some(1773337649));
        assert_eq!(unix("junk"), None);
        assert_eq!(unix("9000000000000000-01-01 00:00:00"), None);
        assert_eq!(unix("2026-13-40 99:99:99"), None);
    }
}
