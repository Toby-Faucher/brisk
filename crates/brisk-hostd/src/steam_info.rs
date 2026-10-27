//! Per-game Steam data cached under `data_dir/steam/<name>.json` as `{fetched, value}`.
use crate::config::Config;
use crate::http::AppState;
use crate::steam::{account_id, now, steam_get, steam_id, store_get};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize)]
struct Entry<T> {
    fetched: u64,
    value: T,
}

/// The cached value if fresh (and not `refresh`), else whatever `fetch` returns, stored. A failed
/// fetch falls back to the stale value, or None. `fetch` is not polled when the cache answers.
pub(crate) async fn cached<T: Serialize + DeserializeOwned + Clone>(
    cfg: &Config,
    name: &str,
    ttl_s: u64,
    refresh: bool,
    fetch: impl Future<Output = Result<T, String>>,
) -> Option<T> {
    assert!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "bad cache name {name:?}"
    );
    let dir = cfg.data_dir.join("steam");
    let path = dir.join(format!("{name}.json"));
    let old: Option<Entry<T>> = tokio::fs::read(&path)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(e) = &old
        && !refresh
        && now().saturating_sub(e.fetched) < ttl_s
    {
        return Some(e.value.clone());
    }
    match fetch.await {
        Ok(value) => {
            let e = Entry {
                fetched: now(),
                value,
            };
            let _ = tokio::fs::create_dir_all(&dir).await;
            let _ = tokio::fs::write(&path, serde_json::to_vec(&e).unwrap_or_default()).await;
            Some(e.value)
        }
        Err(err) => {
            eprintln!("warning: steam {name} failed: {err}");
            old.map(|e| e.value)
        }
    }
}

/// Store page facts for the detail panel. `available: false` when the Store has nothing (or is down).
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub available: bool,
    /// Plain text: tags stripped, entities decoded.
    pub description: String,
    pub genres: Vec<String>,
    /// "full" or "partial".
    pub controller: Option<String>,
    pub singleplayer: bool,
    pub coop: bool,
    pub remote_play_together: bool,
    pub release: Option<String>,
    pub metacritic: Option<u32>,
    /// Thumbnail URLs, at most 6.
    pub screenshots: Vec<String>,
}

const WEEK_S: u64 = 7 * 86_400;
const MISS_S: u64 = 600;

/// Two TTLs from the one-TTL `cached()`: the outer entry (`<name>`, 10 minutes) holds whatever the
/// last answer was, success or the default ("unavailable"). When it is stale its fetch asks the inner
/// entry (`<name>-ok`, `ok_ttl_s`), which only ever stores successes because its fetch errs on
/// failure. So a success is reused for `ok_ttl_s` (the inner hit costs no upstream call), a failure
/// for 10 minutes, and a stale success beats the default.
async fn cached_with_misses<T: Serialize + DeserializeOwned + Clone + Default>(
    cfg: &Config,
    name: &str,
    ok_ttl_s: u64,
    refresh: bool,
    fetch: impl Future<Output = Result<T, String>>,
) -> T {
    let inner = async {
        Ok(cached(cfg, &format!("{name}-ok"), ok_ttl_s, refresh, fetch)
            .await
            .unwrap_or_default())
    };
    cached(cfg, name, MISS_S, refresh, inner)
        .await
        .unwrap_or_default()
}

pub async fn info(cfg: &Config, appid: u32, refresh: bool) -> Info {
    cached_with_misses(
        cfg,
        &format!("info-{appid}"),
        WEEK_S,
        refresh,
        fetch_info(cfg, appid),
    )
    .await
}

async fn fetch_info(cfg: &Config, appid: u32) -> Result<Info, String> {
    let v = store_get(cfg, &format!("api/appdetails?appids={appid}&l=english")).await?;
    let e = &v[appid.to_string()];
    if e["success"] != true {
        return Err("store has no details".into());
    }
    let d = &e["data"];
    let strs = |key: &str, field: &str| -> Vec<String> {
        d[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x[field].as_str().map(String::from))
            .collect()
    };
    let cats: Vec<u64> = d["categories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["id"].as_u64())
        .collect();
    let has = |ids: &[u64]| cats.iter().any(|c| ids.contains(c));
    // they become <img src> in the UI: https only
    let mut screenshots = strs("screenshots", "path_thumbnail");
    screenshots.retain(|u| u.starts_with("https://"));
    screenshots.truncate(6);
    Ok(Info {
        available: true,
        description: plain_text(d["short_description"].as_str().unwrap_or("")),
        genres: strs("genres", "description"),
        controller: d["controller_support"]
            .as_str()
            .filter(|c| matches!(*c, "full" | "partial"))
            .map(String::from),
        // Store category ids: 2 single-player; 9 co-op, 38 online co-op, 39 shared screen co-op,
        // 48 LAN co-op; 44 Remote Play Together.
        singleplayer: has(&[2]),
        coop: has(&[9, 38, 39, 48]),
        remote_play_together: has(&[44]),
        release: d["release_date"]["date"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from),
        metacritic: d["metacritic"]["score"]
            .as_u64()
            .and_then(|n| n.try_into().ok()),
        screenshots,
    })
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Achievements {
    pub available: bool,
    pub unlocked: u32,
    pub total: u32,
    pub list: Vec<Ach>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Ach {
    pub id: String,
    pub name: String,
    /// Empty for hidden achievements. Text only: the UI never renders it as HTML.
    pub description: String,
    /// https URL or empty.
    pub icon: String,
    pub unlocked_at: Option<u64>,
}

/// Warm and the panel share this 1 h achievements TTL (acceptable: ~3k calls/day worst case).
const HOUR_S: u64 = 3600;

/// Merged schema (names, icons) and player flags. No stats for the game (HTTP 400) or a private
/// profile (403) both give `available: false`, kept for 10 minutes like `info`.
pub async fn achievements(cfg: &Config, appid: u32, refresh: bool) -> Achievements {
    cached_with_misses(
        cfg,
        &format!("ach-{appid}"),
        HOUR_S,
        refresh,
        fetch_achievements(cfg, appid),
    )
    .await
}

async fn fetch_achievements(cfg: &Config, appid: u32) -> Result<Achievements, String> {
    let id = steam_id(&cfg.steam_dir).ok_or("no Steam account id")?;
    // Player first: it is the call that fails for no-stats games and private profiles.
    let p = steam_get(
        cfg,
        &format!("ISteamUserStats/GetPlayerAchievements/v1/?steamid={id}&appid={appid}&l=english"),
    )
    .await?;
    let sc = steam_get(
        cfg,
        &format!("ISteamUserStats/GetSchemaForGame/v2/?appid={appid}&l=english"),
    )
    .await?;
    let times: HashMap<&str, u64> = p["playerstats"]["achievements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["achieved"] == 1)
        .filter_map(|a| {
            Some((
                a["apiname"].as_str()?,
                a["unlocktime"].as_u64().unwrap_or(0),
            ))
        })
        .collect();
    let list: Vec<Ach> = sc["game"]["availableGameStats"]["achievements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let id = a["name"].as_str()?;
            let at = times.get(id).copied();
            let icon = a[if at.is_some() { "icon" } else { "icongray" }]
                .as_str()
                .filter(|u| u.starts_with("https://")) // becomes <img src>
                .unwrap_or("");
            Some(Ach {
                id: id.into(),
                name: a["displayName"].as_str().unwrap_or(id).into(),
                description: if a["hidden"] == 1 {
                    String::new()
                } else {
                    a["description"].as_str().unwrap_or("").into()
                },
                icon: icon.into(),
                unlocked_at: at,
            })
        })
        .collect();
    // the UI keys its list by id: keep the first of any duplicate
    let mut seen = std::collections::HashSet::new();
    let list: Vec<Ach> = list
        .into_iter()
        .filter(|a| seen.insert(a.id.clone()))
        .collect();
    if list.is_empty() {
        return Err("no achievements".into());
    }
    Ok(Achievements {
        available: true,
        unlocked: list.iter().filter(|a| a.unlocked_at.is_some()).count() as u32,
        total: list.len() as u32,
        list,
    })
}

/// (unlocked, total) from the cache file alone, no network; None when never fetched or unavailable.
pub fn ach_counts(cfg: &Config, appid: u32) -> Option<(u32, u32)> {
    let b = std::fs::read(cfg.data_dir.join(format!("steam/ach-{appid}.json"))).ok()?;
    let e: Entry<Achievements> = serde_json::from_slice(&b).ok()?;
    (e.value.total > 0).then_some((e.value.unlocked, e.value.total))
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct News {
    pub items: Vec<NewsItem>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    /// Plain text: the UI never renders it as HTML.
    pub title: String,
    /// https URL or empty.
    pub url: String,
    /// Unix seconds.
    pub date: u64,
    /// From the game's own announcements feed (patch notes), not press or reviews.
    pub patch: bool,
}

const NEWS_TTL_S: u64 = 6 * 3600;
const UPDATED_WINDOW_S: u64 = 30 * 86_400;

/// The latest headlines. Failure is an empty list, kept for 10 minutes like `achievements`.
pub async fn news(cfg: &Config, appid: u32, refresh: bool) -> News {
    cached_with_misses(
        cfg,
        &format!("news-{appid}"),
        NEWS_TTL_S,
        refresh,
        fetch_news(cfg, appid),
    )
    .await
}

async fn fetch_news(cfg: &Config, appid: u32) -> Result<News, String> {
    let v = steam_get(
        cfg,
        &format!("ISteamNews/GetNewsForApp/v2/?appid={appid}&count=5&maxlength=0"),
    )
    .await?;
    let items = v["appnews"]["newsitems"]
        .as_array()
        .ok_or("no news items")?
        .iter()
        .take(5)
        .map(|n| NewsItem {
            title: plain_text(n["title"].as_str().unwrap_or("")),
            url: n["url"]
                .as_str()
                .filter(|u| u.starts_with("https://")) // becomes <a href>
                .unwrap_or("")
                .into(),
            date: n["date"].as_u64().unwrap_or(0),
            patch: n["feedname"] == "steam_community_announcements",
        })
        .filter(|i| !i.title.is_empty()) // an empty title would be a link with no name
        .collect();
    Ok(News { items })
}

/// A patch newer than both the last play (never played: false) and the last 30 days.
pub fn is_updated(news: &News, last_played: u64, now: u64) -> bool {
    last_played > 0
        && news.items.iter().any(|i| {
            i.patch && i.date > last_played && i.date <= now && now - i.date <= UPDATED_WINDOW_S
        })
}

/// `is_updated` from the cache file alone, no network.
pub fn cached_updated(cfg: &Config, appid: u32, last_played: u64, now: u64) -> bool {
    std::fs::read(cfg.data_dir.join(format!("steam/news-{appid}.json")))
        .ok()
        .and_then(|b| serde_json::from_slice::<Entry<News>>(&b).ok())
        .is_some_and(|e| is_updated(&e.value, last_played, now))
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Friends {
    pub available: bool,
    pub online: Vec<Friend>,
}

/// Other people's data: held in memory only, never logged or written to disk.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Friend {
    /// Text only: the UI never renders it as HTML.
    pub name: String,
    /// https URL on a Steam avatar host, or empty.
    pub avatar: String,
    /// "online", "away", "busy" or "snooze".
    pub state: String,
    pub playing: Option<String>,
    pub appid: Option<u32>,
}

const FRIENDS_TTL: Duration = Duration::from_secs(120);
const FRIENDS_MISS_TTL: Duration = Duration::from_secs(60);
const FRIENDS_MAX: usize = 30;
// shortcut: only the first 1000 friends are looked at (10 calls), upgrade if lists that big matter
const FRIENDS_IDS_MAX: usize = 1000;
const AVATAR_HOSTS: [&str; 5] = [
    "steamcdn-a.akamaihd.net",
    "avatars.steamstatic.com",
    "avatars.akamai.steamstatic.com",
    "avatars.cloudflare.steamstatic.com",
    "avatars.fastly.steamstatic.com",
];

/// `friends` held in `state.friends`: 120 s, or 60 s for a failure (a private list must not hammer Steam).
pub async fn friends_cached(state: &AppState) -> Friends {
    if let Some((at, f)) = &*state.friends.lock().unwrap() {
        let ttl = if f.available {
            FRIENDS_TTL
        } else {
            FRIENDS_MISS_TTL
        };
        if at.elapsed() < ttl {
            return f.clone();
        }
    }
    let f = friends(&state.cfg).await;
    *state.friends.lock().unwrap() = Some((std::time::Instant::now(), f.clone()));
    f
}

/// Friends who are online or in a game, playing first then by name, at most 30. Any failure
/// (private list, no key) is `available: false`.
pub async fn friends(cfg: &Config) -> Friends {
    fetch_friends(cfg).await.unwrap_or_else(|err| {
        eprintln!("warning: steam friends failed: {err}");
        Friends::default()
    })
}

async fn fetch_friends(cfg: &Config) -> Result<Friends, String> {
    let id = steam_id(&cfg.steam_dir).ok_or("no Steam account id")?;
    let v = steam_get(
        cfg,
        &format!("ISteamUser/GetFriendList/v1/?steamid={id}&relationship=friend"),
    )
    .await?;
    // ids go into a query string: digits only
    let ids: Vec<&str> = v["friendslist"]["friends"]
        .as_array()
        .ok_or("no friends list")?
        .iter()
        .filter_map(|f| f["steamid"].as_str())
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .take(FRIENDS_IDS_MAX)
        .collect();
    let mut online = vec![];
    for batch in ids.chunks(100) {
        let v = steam_get(
            cfg,
            &format!(
                "ISteamUser/GetPlayerSummaries/v2/?steamids={}",
                batch.join(",")
            ),
        )
        .await?;
        for p in v["response"]["players"].as_array().into_iter().flatten() {
            let playing = p["gameextrainfo"]
                .as_str()
                .filter(|g| !g.is_empty())
                .map(String::from);
            let state = match p["personastate"].as_u64().unwrap_or(0) {
                0 if playing.is_none() => continue,
                2 => "busy",
                3 => "away",
                4 => "snooze",
                _ => "online",
            };
            let name = p["personaname"].as_str().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            online.push(Friend {
                name: name.into(),
                avatar: p["avatarfull"]
                    .as_str()
                    .filter(|u| avatar_ok(u))
                    .unwrap_or("")
                    .into(),
                state: state.into(),
                appid: playing
                    .as_ref()
                    .and_then(|_| p["gameid"].as_str()?.parse().ok()),
                playing,
            });
        }
    }
    online.sort_by_cached_key(|f| (f.playing.is_none(), f.name.to_lowercase()));
    online.truncate(FRIENDS_MAX);
    Ok(Friends {
        available: true,
        online,
    })
}

/// https on a Steam avatar host (it becomes an `<img src>`).
fn avatar_ok(u: &str) -> bool {
    u.strip_prefix("https://")
        .and_then(|r| r.split('/').next())
        .is_some_and(|h| AVATAR_HOSTS.contains(&h))
}

/// Pause between games so a warm never bursts against the Web API.
const WARM_PAUSE: Duration = Duration::from_millis(300);
const WARM_MAX: usize = 60;

/// Fetches per-game data into the cache, one game at a time. Not spawned: tests await it.
pub async fn warm_games(cfg: &Config, appids: Vec<u32>, pause: Duration) {
    for (i, id) in appids.into_iter().take(WARM_MAX).enumerate() {
        if i > 0 {
            tokio::time::sleep(pause).await;
        }
        achievements(cfg, id, false).await;
        news(cfg, id, false).await;
    }
}

/// Clears `warming` on drop, so a panic in the task cannot wedge it.
struct Unwarm(Arc<AppState>);
impl Drop for Unwarm {
    fn drop(&mut self) {
        self.0.warming.store(false, Ordering::SeqCst);
    }
}

/// Background `warm_games`, at most one at a time; returns at once.
pub fn warm(state: Arc<AppState>, appids: Vec<u32>) {
    if state.warming.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        let guard = Unwarm(state);
        warm_games(&guard.0.cfg, appids, WARM_PAUSE).await;
    });
}

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
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<(SystemTime, String)> = rd
        .flatten()
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
                s.bytes += m.len();
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
pub fn local(cfg: &Config, appid: u32) -> Local {
    let Some(u) = user_dir(cfg) else {
        return Local::default();
    };
    Local {
        screenshots: list_shots(&u.join(format!("760/remote/{appid}/screenshots"))),
        saves: save_summary(&u.join(format!("{appid}/remote"))),
    }
}

/// `(content type, bytes)` for a screenshot `name` that is exactly one of `local()`'s names, else None.
/// The name is only joined onto the folder after it matched a listing entry; the open refuses a
/// symlink swapped in since (O_NOFOLLOW) and the read is capped. Blocking: call from `spawn_blocking`.
pub fn screenshot(cfg: &Config, appid: u32, name: &str) -> Option<(&'static str, Vec<u8>)> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let ct = image_type(name)?;
    let dir = shots_dir(cfg, appid)?;
    let listed = list_shots(&dir).into_iter().find(|n| n == name)?;
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

/// Store HTML to plain text: tags dropped (block ones become a space), then entities decoded, then
/// whitespace collapsed. Decoding last means `&lt;b&gt;` stays literal text; the UI never renders HTML.
fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        // only `<` + letter, `/` or `!` starts a tag; "a < b" stays text (`<` is ASCII, so i + 1 is a boundary)
        if !rest[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!') {
            out.push('<');
            rest = &rest[i + 1..];
            continue;
        }
        let Some(j) = rest[i..].find('>') else {
            rest = "";
            break;
        };
        let name = rest[i + 1..i + j]
            .trim_start_matches('/')
            .to_ascii_lowercase();
        let name = name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .next()
            .unwrap_or("");
        if matches!(
            name,
            "br" | "p" | "li" | "div" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) {
            out.push(' ');
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // bytes, not a str slice: byte 12 can fall inside a multi-byte char. `&` and `;` are ASCII, so
        // slicing at them below is always on a char boundary.
        let end = rest.bytes().take(12).position(|b| b == b';');
        let ch = end.and_then(|e| match &rest[1..e] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            n => n
                .strip_prefix("#x")
                .or_else(|| n.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        });
        match (ch, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(d: &std::path::Path) -> Config {
        let mut c = Config::from_env(d.join("dist"));
        c.data_dir = d.to_path_buf();
        c
    }

    /// Writes an entry `age` seconds old.
    fn seed(c: &Config, name: &str, age: u64, v: u32) {
        std::fs::create_dir_all(c.data_dir.join("steam")).unwrap();
        let e = Entry {
            fetched: now() - age,
            value: v,
        };
        std::fs::write(
            c.data_dir.join(format!("steam/{name}.json")),
            serde_json::to_vec(&e).unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn cached_returns_fresh_without_fetching() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(d.path());
        seed(&c, "a", 10, 1);
        let r = cached(&c, "a", 100, false, async { panic!("fetched") }).await;
        assert_eq!(r, Some(1));
    }

    #[tokio::test]
    async fn cached_refetches_when_stale_or_refresh() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(d.path());
        seed(&c, "a", 200, 1);
        assert_eq!(
            cached(&c, "a", 100, false, async { Ok(2u32) }).await,
            Some(2)
        );
        // Now fresh with 2; refresh still refetches.
        assert_eq!(
            cached(&c, "a", 100, false, async { Ok(9u32) }).await,
            Some(2)
        );
        assert_eq!(
            cached(&c, "a", 100, true, async { Ok(3u32) }).await,
            Some(3)
        );
        assert_eq!(
            cached(&c, "a", 100, false, async { Ok(9u32) }).await,
            Some(3)
        );
    }

    #[tokio::test]
    async fn cached_returns_stale_on_fetch_error() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(d.path());
        seed(&c, "a", 200, 1);
        let r = cached(&c, "a", 100, false, async { Err::<u32, _>("down".into()) }).await;
        assert_eq!(r, Some(1));
    }

    #[tokio::test]
    async fn cached_none_on_error_without_stale() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(d.path());
        let r = cached(&c, "a", 100, false, async { Err::<u32, _>("down".into()) }).await;
        assert_eq!(r, None);
    }

    #[test]
    fn plain_text_strips_and_decodes() {
        assert_eq!(
            plain_text("a<br/>b &lt;i&gt; &#x27;c&#39; &bogus; & d<b"),
            "a b <i> 'c' &bogus; & d"
        );
        // `&` followed by multi-byte text: the entity scan must never cut inside a character
        for t in [
            "& ééééééé;",
            "&éééééééééééé;",
            "Q&—A™",
            "&™ and &—;",
            "tail &",
        ] {
            assert_eq!(plain_text(t), t);
        }
        // a `<` that does not start a tag stays text
        assert_eq!(plain_text("a < b, 3<4 <i>x</i>"), "a < b, 3<4 x");
    }

    #[tokio::test]
    #[should_panic(expected = "bad cache name")]
    async fn cached_rejects_bad_name() {
        let d = tempfile::tempdir().unwrap();
        cached(&cfg(d.path()), "../x", 1, false, async { Ok(1u32) }).await;
    }
}
