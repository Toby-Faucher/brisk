use super::{Entry, cached_with_misses, plain_text};
use crate::config::Config;
use crate::steam::steam_get;
use serde::{Deserialize, Serialize};

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

pub(super) const NEWS_TTL_S: u64 = 6 * 3600;
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
