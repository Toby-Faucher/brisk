//! Per-game Steam data cached under `data_dir/steam/<name>.json` as `{fetched, value}`.
use crate::config::Config;
use crate::steam::{now, store_get};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::future::Future;

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

/// Two TTLs from the one-TTL `cached()`: the outer entry (`info-<id>`, 10 minutes) holds whatever the
/// last answer was, success or `available: false`. When it is stale its fetch asks the inner entry
/// (`info-<id>-ok`, 7 days), which only ever stores successes because its fetch errs on failure. So
/// a success is reused for a week (the inner hit costs no upstream call), a failure for 10 minutes,
/// and a stale success beats `available: false`.
pub async fn info(cfg: &Config, appid: u32, refresh: bool) -> Info {
    let inner = async {
        Ok(cached(
            cfg,
            &format!("info-{appid}-ok"),
            WEEK_S,
            refresh,
            fetch_info(cfg, appid),
        )
        .await
        .unwrap_or_default())
    };
    cached(cfg, &format!("info-{appid}"), MISS_S, refresh, inner)
        .await
        .unwrap_or_default()
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
    let mut screenshots = strs("screenshots", "path_thumbnail");
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

/// Store HTML to plain text: tags dropped (block ones become a space), then entities decoded, then
/// whitespace collapsed. Decoding last means `&lt;b&gt;` stays literal text; the UI never renders HTML.
fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
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
        let end = rest[..rest.len().min(12)].find(';');
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
    }

    #[tokio::test]
    #[should_panic(expected = "bad cache name")]
    async fn cached_rejects_bad_name() {
        let d = tempfile::tempdir().unwrap();
        cached(&cfg(d.path()), "../x", 1, false, async { Ok(1u32) }).await;
    }
}
