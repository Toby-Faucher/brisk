use super::{Entry, cached_with_misses};
use crate::config::Config;
use crate::steam::{steam_get, steam_id};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
pub(super) const HOUR_S: u64 = 3600;

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
