use crate::config::Config;
use crate::http::AppState;
use crate::steam::{is_refusal, read_key, steam_get, steam_id};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

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

/// Longest a good list is served after fetches started failing (transient errors only).
const FRIENDS_STALE_MAX: Duration = Duration::from_secs(600);

/// `state.friends`: when it was last checked, when the list was last fetched fine, the list.
pub type FriendsEntry = (Instant, Instant, Friends);

/// `friends` held in `state.friends`: 120 s, or 60 s for a failure (a private list must not hammer Steam).
/// A transient failure (timeout, 5xx, TLS) keeps a good list for at most 10 minutes and retries after 60 s;
/// a refusal (private list, bad key) or a local config error gives `available: false` at once.
pub async fn friends_cached(state: &AppState) -> Friends {
    if let Some((at, _, f)) = &*state.friends.lock().unwrap() {
        let ttl = if f.available {
            FRIENDS_TTL
        } else {
            FRIENDS_MISS_TTL
        };
        if at.elapsed() < ttl {
            return f.clone();
        }
    }
    let result = fetch_friends(&state.cfg).await;
    let now = Instant::now();
    // one lock for take, decide and store: a concurrent request cannot see a half-way state
    let mut slot = state.friends.lock().unwrap();
    let entry = match result {
        Ok(f) => (now, now, f),
        Err(Fail::Transient(err)) => {
            eprintln!("warning: steam friends failed: {err}");
            match slot.take() {
                Some((_, good_at, f)) if f.available && good_at.elapsed() < FRIENDS_STALE_MAX => (
                    // looks 60 s old already, so the next try is 60 s away
                    now.checked_sub(FRIENDS_TTL - FRIENDS_MISS_TTL)
                        .unwrap_or(now),
                    good_at,
                    f,
                ),
                _ => (now, now, Friends::default()),
            }
        }
        Err(Fail::Permanent(err)) => {
            eprintln!("warning: steam friends failed: {err}");
            (now, now, Friends::default())
        }
    };
    let f = entry.2.clone();
    *slot = Some(entry);
    f
}

/// Transient: worth waiting out. Permanent: a config error or a refusal (401/403).
enum Fail {
    Transient(String),
    Permanent(String),
}

async fn fetch_friends(cfg: &Config) -> Result<Friends, Fail> {
    let perm = |e: &str| Fail::Permanent(e.into());
    let id = steam_id(&cfg.steam_dir).ok_or_else(|| perm("no Steam account id"))?;
    read_key(cfg).map_err(perm)?;
    let sort = |e: String| {
        if is_refusal(&e) {
            Fail::Permanent(e)
        } else {
            Fail::Transient(e)
        }
    };
    let v = steam_get(
        cfg,
        &format!("ISteamUser/GetFriendList/v1/?steamid={id}&relationship=friend"),
    )
    .await
    .map_err(sort)?;
    // ids go into a query string: digits only
    let ids: Vec<&str> = v["friendslist"]["friends"]
        .as_array()
        .ok_or_else(|| perm("no friends list"))?
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
        .await
        .map_err(sort)?;
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
