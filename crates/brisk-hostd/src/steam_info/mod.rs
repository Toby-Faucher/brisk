//! Per-game Steam data cached under `data_dir/steam/<name>.json` as `{fetched, value}`.

mod friends;
mod local;
mod news;
mod warm;

use crate::config::Config;
use crate::steam::{now, steam_get, steam_id, store_get};
pub use friends::{Friend, Friends, friends, friends_cached};
pub use local::{Local, SaveInfo, local, screenshot};
pub use news::{News, NewsItem, cached_updated, is_updated, news};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::future::Future;
pub use warm::{warm, warm_games};

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
    /// The full-size image for each thumbnail, same order (the thumbnail again when the Store's
    /// full-size URL is not https). Empty in a cache entry written before this field existed.
    #[serde(default)]
    pub screenshots_full: Vec<String>,
}

const WEEK_S: u64 = 7 * 86_400;
const MISS_S: u64 = 600;

/// True when `cached_with_misses(name, ok_ttl_s)` would answer from disk with no upstream call.
fn answers_fresh(cfg: &Config, name: &str, ok_ttl_s: u64) -> bool {
    let fresh = |n: String, ttl: u64| {
        std::fs::read(cfg.data_dir.join(format!("steam/{n}.json")))
            .ok()
            .and_then(|b| serde_json::from_slice::<Entry<serde::de::IgnoredAny>>(&b).ok())
            .is_some_and(|e| now().saturating_sub(e.fetched) < ttl)
    };
    fresh(name.to_string(), MISS_S) || fresh(format!("{name}-ok"), ok_ttl_s)
}

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

/// (controller, coop) from the info cache alone, no network. Reads the success entry, then the
/// outer one; an entry that is not `available` never counts. None when neither has data.
pub fn cached_controls(cfg: &Config, appid: u32) -> Option<(Option<String>, bool)> {
    ["info-{}-ok", "info-{}"].iter().find_map(|n| {
        let b = std::fs::read(cfg.data_dir.join(format!(
            "steam/{}.json",
            n.replace("{}", &appid.to_string())
        )))
        .ok()?;
        let e: Entry<Info> = serde_json::from_slice(&b).ok()?;
        let i = e.value;
        i.available
            .then_some((i.controller, i.coop || i.remote_play_together))
    })
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
    let (screenshots, screenshots_full) = shots(&d["screenshots"]);
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
        screenshots_full,
    })
}

/// (thumbnails, full-size) from the Store's `screenshots` array, at most 6 pairs. They become
/// `<img src>` in the UI: https only; a pair without an https thumbnail is dropped, a full-size URL
/// that is not https falls back to the thumbnail.
fn shots(list: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    let https = |v: &serde_json::Value| {
        v.as_str()
            .filter(|u| u.starts_with("https://"))
            .map(String::from)
    };
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| {
            let t = https(&x["path_thumbnail"])?;
            Some((https(&x["path_full"]).unwrap_or_else(|| t.clone()), t))
        })
        .take(6)
        .map(|(f, t)| (t, f))
        .unzip()
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
    fn shots_pairs_thumbnails_with_full_size_https_only() {
        let (t, f) = shots(&serde_json::json!([
            {"path_thumbnail": "http://a/0.jpg", "path_full": "https://a/0f.jpg"},
            {"path_thumbnail": "https://a/1.jpg", "path_full": "https://a/1f.jpg"},
            {"path_thumbnail": "https://a/2.jpg", "path_full": "http://a/2f.jpg"},
            {"path_thumbnail": "https://a/3.jpg"},
        ]));
        assert_eq!(t, ["https://a/1.jpg", "https://a/2.jpg", "https://a/3.jpg"]);
        assert_eq!(
            f,
            ["https://a/1f.jpg", "https://a/2.jpg", "https://a/3.jpg"]
        );
        let many: Vec<_> = (0..9)
            .map(|i| serde_json::json!({"path_thumbnail": format!("https://a/{i}.jpg")}))
            .collect();
        assert_eq!(shots(&serde_json::Value::from(many)).0.len(), 6);
        assert_eq!(shots(&serde_json::Value::Null), (vec![], vec![]));
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
