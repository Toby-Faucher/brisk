//! Per-game Steam data cached under `data_dir/steam/<name>.json` as `{fetched, value}`.

mod achievements;
mod friends;
mod local;
mod news;
mod store;
mod warm;

use crate::config::Config;
use crate::steam::now;
pub use achievements::{Ach, Achievements, ach_counts, achievements};
pub use friends::{Friend, Friends, friends, friends_cached};
pub use local::{Local, SaveInfo, ShotCache, local, screenshot};
pub use news::{News, NewsItem, cached_updated, is_updated, news};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::future::Future;
pub use store::{Info, cached_controls, info};
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

    #[tokio::test]
    #[should_panic(expected = "bad cache name")]
    async fn cached_rejects_bad_name() {
        let d = tempfile::tempdir().unwrap();
        cached(&cfg(d.path()), "../x", 1, false, async { Ok(1u32) }).await;
    }
}
