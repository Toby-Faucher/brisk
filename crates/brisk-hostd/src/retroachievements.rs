//! Read-only RetroAchievements client: the user's achievement progress for one game.
//!
//! Shapes confirmed from https://api-docs.retroachievements.org/ (see docs/research/ra-client-notes.md).
//! The Web API cannot look a game up by hash, so the console's hash list is downloaded once
//! (`API_GetGameList.php?i=<console>&h=1`), cached on disk for days and matched locally.
//! Credentials: `~/.config/brisk/retroachievements`, line 1 username, line 2 Web API key. No file = off.
//! The key goes into the URL only inside `get_json`; errors and Debug output never carry it.

use crate::steam::{base_ok, https_get};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_BASE: &str = "https://retroachievements.org/API";
/// shortcut: console ids from memory of the RA site, not confirmed by the official docs (the docs page
/// lists no ids); check with API_GetConsoleIDs.php once a key exists.
pub const CONSOLE_GAMECUBE: u32 = 16;
pub const CONSOLE_WII: u32 = 19;

#[derive(Debug, Clone)]
pub struct RaConfig {
    pub data_dir: PathBuf,
    pub key_file: PathBuf,
    pub base: String,
    pub hash_ttl: Duration,
    pub progress_ttl: Duration,
    /// Least gap between two requests from this process.
    pub min_interval: Duration,
}

impl RaConfig {
    /// `BRISK_RA_BASE` overrides the host (tests point it at a local mock).
    pub fn from_env(data_dir: &Path) -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Self {
            data_dir: data_dir.to_path_buf(),
            key_file: home.join(".config/brisk/retroachievements"),
            base: std::env::var("BRISK_RA_BASE").unwrap_or_else(|_| DEFAULT_BASE.into()),
            hash_ttl: Duration::from_secs(7 * 86400),
            progress_ttl: Duration::from_secs(300),
            min_interval: Duration::from_secs(1),
        }
    }
}

/// What the UI can show. No variant carries the URL or the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaError {
    /// No credentials file (or it is unreadable): the feature is off.
    Off,
    /// The credentials file is not "username, then key" in plain characters.
    BadCredentials,
    /// HTTP 401 or 403.
    Unauthorized,
    /// The hash is not in the console's list.
    NotFound,
    /// HTTP 429.
    RateLimited,
    /// Connection, TLS, timeout or other HTTP status; text is only the kind of failure.
    Network(String),
    /// Body over the size cap.
    TooLarge,
    /// Body is not the documented JSON.
    BadResponse,
}

impl std::fmt::Display for RaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => f.write_str("RetroAchievements is not set up"),
            Self::BadCredentials => f.write_str("RetroAchievements credentials file is malformed"),
            Self::Unauthorized => f.write_str("RetroAchievements refused the key"),
            Self::NotFound => f.write_str("no RetroAchievements set for this game"),
            Self::RateLimited => f.write_str("RetroAchievements rate limit, try later"),
            Self::Network(e) => write!(f, "RetroAchievements unreachable ({e})"),
            Self::TooLarge => f.write_str("RetroAchievements response too large"),
            Self::BadResponse => f.write_str("RetroAchievements sent an unexpected response"),
        }
    }
}

impl std::error::Error for RaError {}

struct Creds {
    user: String,
    key: String,
}

impl std::fmt::Debug for Creds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Creds {{ user: {:?}, key: <redacted> }}", self.user)
    }
}

fn plain(s: &str, extra: &[char]) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
}

/// Both values only use URL-safe characters, so they cannot change the shape of the query.
fn read_creds(cfg: &RaConfig) -> Result<Creds, RaError> {
    let text = std::fs::read_to_string(&cfg.key_file).map_err(|_| RaError::Off)?;
    let mut l = text.lines().map(str::trim);
    match (l.next(), l.next()) {
        (Some(u), Some(k)) if plain(u, &['_', '-', '.']) && plain(k, &['_', '-']) => Ok(Creds {
            user: u.into(),
            key: k.into(),
        }),
        _ => Err(RaError::BadCredentials),
    }
}

// ---- typed results ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Achievement {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub badge_name: String,
    pub display_order: u32,
    /// `DateEarned` as the API gives it ("YYYY-MM-DD HH:MM:SS"); None = locked.
    pub unlocked: Option<String>,
    /// `DateEarnedHardcore`; None = not earned in hardcore mode.
    pub unlocked_hardcore: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameProgress {
    pub game_id: u64,
    pub title: String,
    pub console_id: u32,
    pub console_name: String,
    pub num_achievements: u32,
    pub num_awarded: u32,
    pub num_awarded_hardcore: u32,
    pub achievements: Vec<Achievement>,
}

// Wire shapes (PascalCase, as documented). Unknown fields are ignored.
#[derive(Deserialize)]
struct WireGame {
    #[serde(rename = "ID")]
    id: u64,
    #[serde(rename = "Hashes", default)]
    hashes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireAch {
    #[serde(rename = "ID")]
    id: u64,
    title: String,
    description: String,
    points: u32,
    badge_name: String,
    #[serde(default)]
    display_order: u32,
    date_earned: Option<String>,
    date_earned_hardcore: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WireProgress {
    #[serde(rename = "ID")]
    id: u64,
    title: String,
    #[serde(rename = "ConsoleID")]
    console_id: u32,
    console_name: String,
    #[serde(default)]
    num_achievements: u32,
    #[serde(default)]
    achievements: HashMap<String, WireAch>,
    #[serde(default)]
    num_awarded_to_user: u32,
    #[serde(default)]
    num_awarded_to_user_hardcore: u32,
}

impl From<WireProgress> for GameProgress {
    fn from(w: WireProgress) -> Self {
        let mut achievements: Vec<Achievement> = w
            .achievements
            .into_values()
            .map(|a| Achievement {
                id: a.id,
                title: a.title,
                description: a.description,
                points: a.points,
                badge_name: a.badge_name,
                display_order: a.display_order,
                unlocked: a.date_earned,
                unlocked_hardcore: a.date_earned_hardcore,
            })
            .collect();
        achievements.sort_by_key(|a| (a.display_order, a.id));
        Self {
            game_id: w.id,
            title: w.title,
            console_id: w.console_id,
            console_name: w.console_name,
            num_achievements: w.num_achievements,
            num_awarded: w.num_awarded_to_user,
            num_awarded_hardcore: w.num_awarded_to_user_hardcore,
            achievements,
        }
    }
}

// ---- HTTP ----

/// shortcut: one process-wide clock, so concurrent callers queue behind each other's sleep; fine for
/// one poll at a time, revisit if several games are polled at once.
static LAST_CALL: Mutex<Option<Instant>> = Mutex::new(None);

async fn pace(gap: Duration) {
    let wait = {
        let mut last = LAST_CALL.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let at = last.map_or(now, |t| (t + gap).max(now));
        *last = Some(at);
        at - now
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
}

/// One GET, no retries. The key is added to the URL here and nowhere else.
async fn get_json<T: serde::de::DeserializeOwned + Send + 'static>(
    cfg: &RaConfig,
    creds: &Creds,
    script: &str,
    query: &str,
) -> Result<T, RaError> {
    let base = cfg.base.trim_end_matches('/');
    if !base_ok(base) {
        return Err(RaError::Network("base must be https".into()));
    }
    pace(cfg.min_interval).await;
    let url = format!("{base}/{script}?y={}&{query}", creds.key);
    // Body is capped at 8 MiB by https_get (shortcut: a Wii list with hashes is believed smaller; unmeasured).
    let body = tokio::task::spawn_blocking(move || https_get(&url))
        .await
        .map_err(|_| RaError::Network("request task failed".into()))?
        .map_err(|e| match e.as_str() {
            "HTTP 401" | "HTTP 403" => RaError::Unauthorized,
            "HTTP 429" => RaError::RateLimited,
            "response too large" => RaError::TooLarge,
            _ => RaError::Network(e),
        })?;
    serde_json::from_slice(&body).map_err(|_| RaError::BadResponse)
}

// ---- disk cache ----

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[derive(Serialize, Deserialize)]
struct Cached<T> {
    fetched: u64,
    data: T,
}

fn read_cache<T: serde::de::DeserializeOwned>(path: &Path, ttl: Duration) -> Option<T> {
    let c: Cached<T> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (now_secs().saturating_sub(c.fetched) < ttl.as_secs()).then_some(c.data)
}

/// 0600 temp file, fsync, rename. A failure leaves no temp file behind.
fn write_cache<T: Serialize>(path: &Path, data: &T) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let go = || -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&serde_json::to_vec(&Cached {
            fetched: now_secs(),
            data,
        })?)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    go().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

// ---- public entry points ----

/// Hash (as listed by RA, lowercase hex) to game id, from the cached or freshly downloaded list.
pub async fn resolve_hash(cfg: &RaConfig, console: u32, hash: &str) -> Result<u64, RaError> {
    let creds = read_creds(cfg)?;
    resolve_with(cfg, &creds, console, hash).await
}

async fn resolve_with(
    cfg: &RaConfig,
    creds: &Creds,
    console: u32,
    hash: &str,
) -> Result<u64, RaError> {
    let path = cfg.data_dir.join(format!("ra/hashes-{console}.json"));
    let map: HashMap<String, u64> = match read_cache(&path, cfg.hash_ttl) {
        Some(m) => m,
        None => {
            // f=1: only games that have achievements.
            let games: Vec<WireGame> = get_json(
                cfg,
                creds,
                "API_GetGameList.php",
                &format!("i={console}&h=1&f=1"),
            )
            .await?;
            let m: HashMap<String, u64> = games
                .into_iter()
                .flat_map(|g| {
                    g.hashes
                        .into_iter()
                        .map(move |h| (h.to_ascii_lowercase(), g.id))
                })
                .collect();
            // shortcut: a failed cache write only costs a refetch next time.
            let _ = write_cache(&path, &m);
            m
        }
    };
    map.get(&hash.to_ascii_lowercase())
        .copied()
        .ok_or(RaError::NotFound)
}

/// The user's progress for a game id; cached for `progress_ttl`.
pub async fn game_progress(cfg: &RaConfig, game_id: u64) -> Result<GameProgress, RaError> {
    let creds = read_creds(cfg)?;
    progress_with(cfg, &creds, game_id).await
}

async fn progress_with(
    cfg: &RaConfig,
    creds: &Creds,
    game_id: u64,
) -> Result<GameProgress, RaError> {
    let path = cfg.data_dir.join(format!("ra/game-{game_id}.json"));
    if let Some(p) = read_cache(&path, cfg.progress_ttl) {
        return Ok(p);
    }
    let w: WireProgress = get_json(
        cfg,
        creds,
        "API_GetGameInfoAndUserProgress.php",
        &format!("u={}&g={game_id}", creds.user),
    )
    .await?;
    let p = GameProgress::from(w);
    let _ = write_cache(&path, &p);
    Ok(p)
}

/// Hash to progress in one call: the entry point for the UI.
pub async fn progress_for(
    cfg: &RaConfig,
    console: u32,
    hash: &str,
) -> Result<GameProgress, RaError> {
    let creds = read_creds(cfg)?;
    let id = resolve_with(cfg, &creds, console, hash).await?;
    progress_with(cfg, &creds, id).await
}
