use super::achievements::HOUR_S;
use super::news::NEWS_TTL_S;
use super::store::WEEK_S;
use super::{achievements, answers_fresh, info, news};
use crate::config::Config;
use crate::http::AppState;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Pause between games so a warm never bursts against the Web API.
const WARM_PAUSE: Duration = Duration::from_millis(300);
const WARM_INFO_PAUSE: Duration = Duration::from_millis(500);
const WARM_MAX: usize = 60;
const WARM_INFO_MAX: usize = 200;

/// Fetches per-game data into the cache, one game at a time. Not spawned: tests await it.
/// First achievements and news for `played` (at most 60), then store info for every `owned` game
/// (at most 200; a fresh cache entry makes no upstream call).
pub async fn warm_games(
    cfg: &Config,
    played: Vec<u32>,
    owned: Vec<u32>,
    pause: Duration,
    info_pause: Duration,
) {
    // Pause only after a game that went upstream; cached ones cost nothing.
    let mut went = false;
    for id in played.into_iter().take(WARM_MAX) {
        let (a, n) = (format!("ach-{id}"), format!("news-{id}"));
        if answers_fresh(cfg, &a, HOUR_S) && answers_fresh(cfg, &n, NEWS_TTL_S) {
            continue;
        }
        if went {
            tokio::time::sleep(pause).await;
        }
        went = true;
        achievements(cfg, id, false).await;
        news(cfg, id, false).await;
    }
    went = false;
    for id in owned.into_iter().take(WARM_INFO_MAX) {
        if answers_fresh(cfg, &format!("info-{id}"), WEEK_S) {
            continue;
        }
        if went {
            tokio::time::sleep(info_pause).await;
        }
        went = true;
        info(cfg, id, false).await;
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
pub fn warm(state: Arc<AppState>, played: Vec<u32>, owned: Vec<u32>) {
    if state.warming.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        let guard = Unwarm(state);
        warm_games(&guard.0.cfg, played, owned, WARM_PAUSE, WARM_INFO_PAUSE).await;
    });
}
