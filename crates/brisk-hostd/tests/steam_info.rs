mod common;
use brisk_hostd::http::AppState;
use common::*;
use std::sync::Arc;

#[tokio::test]
async fn owned_still_works_through_steam_get() {
    let owned = std::fs::read_to_string(format!("{FIX}/owned.json")).unwrap();
    let base = mock_steam(vec![("/IPlayerService/GetOwnedGames/v1/", 200, owned)]).await;
    let s = steam(&base, "http://127.0.0.1:1", Some(KEY));
    let (v, _) = games_json(&s.cfg, "/api/games").await;
    let g = v["games"].as_array().unwrap();
    let portal = g.iter().find(|g| g["appid"] == 620).unwrap();
    assert_eq!(portal["name"], "Portal 2");
}

const NO_API: &str = "http://127.0.0.1:1";

fn appdetails() -> String {
    std::fs::read_to_string(format!("{FIX}/appdetails-620.json")).unwrap()
}

#[tokio::test]
async fn info_distils_store_details() {
    let store = mock_steam(vec![(
        "/api/appdetails?appids=620&l=english",
        200,
        appdetails(),
    )])
    .await;
    let s = steam(NO_API, &store, Some(KEY));
    let (st, v) = get_json(&s.cfg, "/api/games/620/info").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["available"], true);
    assert_eq!(v["genres"], serde_json::json!(["Action", "Puzzle"]));
    assert_eq!(v["controller"], "full");
    assert_eq!(v["coop"], true);
    assert_eq!(v["singleplayer"], true);
    assert_eq!(v["remotePlayTogether"], true);
    assert_eq!(v["metacritic"], 95);
    assert_eq!(v["release"], "18 Apr, 2011");
    let shots = v["screenshots"].as_array().unwrap();
    // the fixture's first thumbnail is http://, so it is dropped before the cap of 6
    assert_eq!(shots.len(), 6);
    assert!(
        shots
            .iter()
            .all(|s| s.as_str().unwrap().starts_with("https://"))
    );
    assert!(shots[0].as_str().unwrap().ends_with("ss_00.600x338.jpg"));
    let d = v["description"].as_str().unwrap();
    assert_eq!(
        d,
        "Invented blurb: portals & puzzles, a \"test\" chamber's second line."
    );
}

#[tokio::test]
async fn info_unknown_appid_404() {
    let (store, hits) = mock_steam_hits(vec![("/api/appdetails", 200, appdetails())]).await;
    let s = steam(NO_API, &store, Some(KEY));
    let (st, _) = get_json(&s.cfg, "/api/games/999999/info").await;
    assert_eq!(st, 404);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn info_store_failure_is_unavailable_not_error() {
    let store = mock_steam(vec![("/api/appdetails", 500, String::new())]).await;
    let s = steam(NO_API, &store, Some(KEY));
    let (st, v) = get_json(&s.cfg, "/api/games/620/info").await;
    assert_eq!(st, 200);
    assert_eq!(v["available"], false);
    assert!(!v.to_string().contains("TESTKEY"));
}

#[tokio::test]
async fn info_failure_is_cached_ten_minutes() {
    let (store, hits) = mock_steam_hits(vec![("/api/appdetails", 500, String::new())]).await;
    let s = steam(NO_API, &store, Some(KEY));
    let (_, v) = get_json(&s.cfg, "/api/games/620/info").await;
    assert_eq!(v["available"], false);
    let (_, v) = get_json(&s.cfg, "/api/games/620/info").await;
    assert_eq!(v["available"], false);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    // ?refresh=1 asks again
    get_json(&s.cfg, "/api/games/620/info?refresh=1").await;
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn info_success_false_is_unavailable() {
    let store = mock_steam(vec![(
        "/api/appdetails",
        200,
        r#"{"620":{"success":false}}"#.into(),
    )])
    .await;
    let s = steam(NO_API, &store, Some(KEY));
    let (st, v) = get_json(&s.cfg, "/api/games/620/info").await;
    assert_eq!(st, 200);
    assert_eq!(v["available"], false);
    assert_eq!(v["genres"], serde_json::json!([]));
}

fn owned() -> String {
    std::fs::read_to_string(format!("{FIX}/owned.json")).unwrap()
}

fn ach_api(player: String, schema: String) -> Vec<(&'static str, u16, String)> {
    vec![
        ("/ISteamUserStats/GetPlayerAchievements/", 200, player),
        ("/ISteamUserStats/GetSchemaForGame/", 200, schema),
    ]
}

fn ach_fixtures() -> Vec<(&'static str, u16, String)> {
    let rd = |n: &str| std::fs::read_to_string(format!("{FIX}/{n}")).unwrap();
    ach_api(rd("playerachievements-620.json"), rd("schema-620.json"))
}

#[tokio::test]
async fn achievements_merge_schema_and_player() {
    let api = mock_steam(ach_fixtures()).await;
    let s = steam(&api, NO_API, Some(KEY));
    let (st, v) = get_json(&s.cfg, "/api/games/620/achievements").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        (
            v["available"].clone(),
            v["unlocked"].clone(),
            v["total"].clone()
        ),
        (true.into(), 3.into(), 5.into())
    );
    let l = v["list"].as_array().unwrap();
    assert_eq!(l[0]["name"], "First Steps");
    assert_eq!(l[0]["unlockedAt"], 1700000100);
    assert_eq!(l[0]["icon"], "https://cdn.example.test/ach/one.jpg");
    // http:// icons are dropped
    assert_eq!(l[1]["icon"], "");
    assert_eq!(l[2]["unlockedAt"], serde_json::Value::Null);
    assert_eq!(l[2]["icon"], "https://cdn.example.test/ach/three_g.jpg");
    assert_eq!(l[3]["description"], "");
    assert!(!v.to_string().contains("TESTKEY"));
}

#[tokio::test]
async fn achievements_unknown_appid_404() {
    let mut routes = ach_fixtures();
    routes.push(("/IPlayerService/GetOwnedGames/", 200, owned()));
    let (api, hits) = mock_steam_hits(routes).await;
    let s = steam(&api, NO_API, Some(KEY));
    let (st, _) = get_json(&s.cfg, "/api/games/999999/achievements").await;
    assert_eq!(st, 404);
    // only the library lookup, nothing from ISteamUserStats
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn achievements_none_is_unavailable() {
    let body = r#"{"playerstats":{"success":false,"error":"Requested app has no stats"}}"#;
    let api = mock_steam(vec![("/ISteamUserStats/", 400, body.into())]).await;
    let s = steam(&api, NO_API, Some(KEY));
    let (st, v) = get_json(&s.cfg, "/api/games/620/achievements").await;
    assert_eq!(st, 200);
    assert_eq!(
        (v["available"].clone(), v["total"].clone()),
        (false.into(), 0.into())
    );
}

#[tokio::test]
async fn achievements_private_profile_is_unavailable_and_cached_briefly() {
    let (api, hits) = mock_steam_hits(vec![
        ("/IPlayerService/GetOwnedGames/", 200, owned()),
        ("/ISteamUserStats/", 403, String::new()),
    ])
    .await;
    let s = steam(&api, NO_API, Some(KEY));
    for _ in 0..2 {
        let (st, v) = get_json(&s.cfg, "/api/games/620/achievements").await;
        assert_eq!(st, 200);
        assert_eq!(
            (v["available"].clone(), v["total"].clone()),
            (false.into(), 0.into())
        );
        assert!(!v.to_string().contains("TESTKEY"));
    }
    // library lookup + one player call: the second request came from the cache, and the failing
    // player call stops before the schema one
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn warm_fills_cache_and_games_show_counts() {
    let api = mock_steam(ach_fixtures()).await;
    let s = steam(&api, NO_API, Some(KEY));
    brisk_hostd::steam_info::warm_games(&s.cfg, vec![620], std::time::Duration::ZERO).await;
    let api2 = mock_steam(vec![("/IPlayerService/GetOwnedGames/v1/", 200, owned())]).await;
    let mut cfg = s.cfg.clone();
    cfg.steam_api_base = api2;
    let (v, _) = games_json(&cfg, "/api/games").await;
    let g = |id: u64| {
        v["games"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["appid"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (g(620)["achUnlocked"].clone(), g(620)["achTotal"].clone()),
        (3.into(), 5.into())
    );
    assert!(g(730)["achUnlocked"].is_null());
}

#[tokio::test]
async fn warm_runs_once_at_a_time() {
    use std::sync::atomic::Ordering::SeqCst;
    let (api, hits) = mock_steam_hits(ach_fixtures()).await;
    let s = steam(&api, NO_API, Some(KEY));
    let st = Arc::new(AppState::new(s.cfg.clone()));
    st.warming.store(true, SeqCst);
    brisk_hostd::steam_info::warm(st.clone(), vec![620]);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(hits.load(SeqCst), 0);
    assert!(
        st.warming.load(SeqCst),
        "the other run's flag is not ours to clear"
    );
    // free: it runs, then clears the flag
    st.warming.store(false, SeqCst);
    brisk_hostd::steam_info::warm(st.clone(), vec![620]);
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if !st.warming.load(SeqCst) && hits.load(SeqCst) > 0 {
            return;
        }
    }
    panic!("warm never finished");
}

#[tokio::test]
async fn warm_caps_at_60_games_and_clears_the_flag() {
    use std::sync::atomic::Ordering::SeqCst;
    let (api, hits) = mock_steam_hits(vec![("/ISteamUserStats/", 403, String::new())]).await;
    let s = steam(&api, NO_API, Some(KEY));
    brisk_hostd::steam_info::warm_games(&s.cfg, (1000..1061).collect(), std::time::Duration::ZERO)
        .await;
    // per game: a failing player call, plus one news call (404 here)
    assert_eq!(hits.load(SeqCst), 120);
    let st = Arc::new(AppState::new(s.cfg.clone()));
    brisk_hostd::steam_info::warm(st.clone(), vec![]);
    for _ in 0..100 {
        if !st.warming.load(SeqCst) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("flag never cleared");
}

#[tokio::test]
async fn duplicate_achievement_ids_keep_the_first() {
    let dup = r#"{"game":{"availableGameStats":{"achievements":[
{"name":"A","displayName":"First","hidden":0,"description":"","icon":"","icongray":""},
{"name":"A","displayName":"Again","hidden":0,"description":"","icon":"","icongray":""}]}}}"#;
    let player = r#"{"playerstats":{"success":true,"achievements":[]}}"#;
    let api = mock_steam(ach_api(player.into(), dup.into())).await;
    let s = steam(&api, NO_API, Some(KEY));
    let (_, v) = get_json(&s.cfg, "/api/games/620/achievements").await;
    assert_eq!(v["total"], 1);
    assert_eq!(v["list"][0]["name"], "First");
}

fn news_fixture() -> String {
    std::fs::read_to_string(format!("{FIX}/news-620.json")).unwrap()
}

fn news_of(items: &[(u64, bool)]) -> brisk_hostd::steam_info::News {
    use brisk_hostd::steam_info::{News, NewsItem};
    News {
        items: items
            .iter()
            .map(|&(date, patch)| NewsItem {
                title: String::new(),
                url: String::new(),
                date,
                patch,
            })
            .collect(),
    }
}

#[test]
fn news_marks_updated_when_patch_after_last_played() {
    use brisk_hostd::steam_info::is_updated;
    // fixture patch 1700001000, Portal 2 last played 1700000500
    assert!(is_updated(
        &news_of(&[(1700001000, true)]),
        1700000500,
        1700002000
    ));
}

#[test]
fn news_not_updated_when_played_after() {
    use brisk_hostd::steam_info::is_updated;
    assert!(!is_updated(
        &news_of(&[(1700001000, true)]),
        1700001500,
        1700002000
    ));
    // a non-patch item never counts, and a never-played game is never "updated"
    assert!(!is_updated(
        &news_of(&[(1700001000, false)]),
        1700000500,
        1700002000
    ));
    assert!(!is_updated(&news_of(&[(1700001000, true)]), 0, 1700002000));
}

#[test]
fn news_old_patch_not_updated() {
    use brisk_hostd::steam_info::is_updated;
    let n = news_of(&[(1700001000, true)]);
    let day = 86_400;
    assert!(is_updated(&n, 1700000500, 1700001000 + 30 * day));
    assert!(!is_updated(&n, 1700000500, 1700001000 + 30 * day + 1));
}

#[tokio::test]
async fn news_non_https_url_is_dropped() {
    let api = mock_steam(vec![
        ("/IPlayerService/GetOwnedGames/", 200, owned()),
        ("/ISteamNews/", 200, news_fixture()),
    ])
    .await;
    let s = steam(&api, NO_API, Some(KEY));
    let (st, v) = get_json(&s.cfg, "/api/games/620/news").await;
    assert_eq!(st, 200, "{v}");
    let l = v["items"].as_array().unwrap();
    assert_eq!(l.len(), 4);
    assert_eq!(l[0]["title"], "Update notes & fixes");
    assert_eq!(l[0]["url"], "https://news.example.test/a");
    assert_eq!(
        (l[0]["patch"].clone(), l[1]["patch"].clone()),
        (true.into(), false.into())
    );
    assert_eq!(l[2]["url"], "");
    assert_eq!(l[3]["url"], "");
    assert!(!v.to_string().contains("TESTKEY"));
}

#[tokio::test]
async fn news_unknown_appid_404_and_failure_is_empty_and_cached() {
    let (api, hits) = mock_steam_hits(vec![
        ("/IPlayerService/GetOwnedGames/", 200, owned()),
        ("/ISteamNews/", 500, String::new()),
    ])
    .await;
    let s = steam(&api, NO_API, Some(KEY));
    let (st, _) = get_json(&s.cfg, "/api/games/999999/news").await;
    assert_eq!(st, 404);
    for _ in 0..2 {
        let (st, v) = get_json(&s.cfg, "/api/games/620/news").await;
        assert_eq!(
            (st.as_u16(), v["items"].clone()),
            (200, serde_json::json!([]))
        );
    }
    // at most the library lookups plus ONE news call: the failure was cached
    let n = hits.load(std::sync::atomic::Ordering::SeqCst);
    assert!(n <= 4, "news failure was not cached: {n} hits");
}

#[tokio::test]
async fn warm_fetches_news_and_games_show_updated() {
    let recent = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 3600;
    let body = format!(
        r#"{{"appnews":{{"newsitems":[{{"title":"Patch","url":"https://n.example.test/p","feedname":"steam_community_announcements","date":{recent}}}]}}}}"#
    );
    let api = mock_steam(vec![("/ISteamNews/", 200, body)]).await;
    let s = steam(&api, NO_API, Some(KEY));
    brisk_hostd::steam_info::warm_games(&s.cfg, vec![620, 400], std::time::Duration::ZERO).await;
    let api2 = mock_steam(vec![("/IPlayerService/GetOwnedGames/v1/", 200, owned())]).await;
    let mut cfg = s.cfg.clone();
    cfg.steam_api_base = api2;
    let (v, _) = games_json(&cfg, "/api/games").await;
    let g = |id: u64| {
        v["games"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["appid"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(g(620)["updated"], true);
    // 400 has the same news but is not installed
    assert_eq!(g(400)["updated"], false);
}
