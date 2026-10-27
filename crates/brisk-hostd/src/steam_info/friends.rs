use crate::config::Config;
use crate::http::AppState;
use crate::steam::{steam_get, steam_id};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
/// A failed fetch keeps a good list that was cached before: it is served again and the retry waits 60 s.
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
    let now = std::time::Instant::now();
    let (at, f) = match fetch_friends(&state.cfg).await {
        Ok(f) => (now, f),
        Err(err) => {
            eprintln!("warning: steam friends failed: {err}");
            let prev = state.friends.lock().unwrap().take();
            match prev {
                // the old list expires after 60 s more (TTL minus MISS_TTL already counted as age)
                Some((_, f)) if f.available => (
                    now.checked_sub(FRIENDS_TTL - FRIENDS_MISS_TTL)
                        .unwrap_or(now),
                    f,
                ),
                _ => (now, Friends::default()),
            }
        }
    };
    *state.friends.lock().unwrap() = Some((at, f.clone()));
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
