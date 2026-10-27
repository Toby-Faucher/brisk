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
const Z: std::time::Duration = std::time::Duration::ZERO;

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
    // the full-size image for each thumbnail, same order and filter
    let full = v["screenshotsFull"].as_array().unwrap();
    assert_eq!(full.len(), 6);
    assert!(full[0].as_str().unwrap().ends_with("ss_00.1920x1080.jpg"));
    assert!(full[5].as_str().unwrap().ends_with("ss_05.1920x1080.jpg"));
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
    brisk_hostd::steam_info::warm_games(&s.cfg, vec![620], vec![], Z, Z).await;
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
    brisk_hostd::steam_info::warm(st.clone(), vec![620], vec![]);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(hits.load(SeqCst), 0);
    assert!(
        st.warming.load(SeqCst),
        "the other run's flag is not ours to clear"
    );
    // free: it runs, then clears the flag
    st.warming.store(false, SeqCst);
    brisk_hostd::steam_info::warm(st.clone(), vec![620], vec![]);
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
    brisk_hostd::steam_info::warm_games(&s.cfg, (1000..1061).collect(), vec![], Z, Z).await;
    // per game: a failing player call, plus one news call (404 here)
    assert_eq!(hits.load(SeqCst), 120);
    let st = Arc::new(AppState::new(s.cfg.clone()));
    brisk_hostd::steam_info::warm(st.clone(), vec![], vec![]);
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

#[test]
fn news_future_equal_and_empty_are_not_updates() {
    use brisk_hostd::steam_info::is_updated;
    // bogus future date, date == last played, no items
    assert!(!is_updated(
        &news_of(&[(1700003000, true)]),
        1700000500,
        1700002000
    ));
    assert!(!is_updated(
        &news_of(&[(1700000500, true)]),
        1700000500,
        1700002000
    ));
    assert!(!is_updated(&news_of(&[]), 1700000500, 1700002000));
}

#[tokio::test]
async fn news_empty_titles_are_dropped() {
    let body = r#"{"appnews":{"newsitems":[
{"title":"","url":"https://n.example.test/a","feedname":"x","date":1},
{"title":"<br/>","url":"https://n.example.test/b","feedname":"x","date":2},
{"url":"https://n.example.test/c","feedname":"x","date":3},
{"title":"Kept","url":"https://n.example.test/d","feedname":"x","date":4}]}}"#;
    let api = mock_steam(vec![
        ("/IPlayerService/GetOwnedGames/", 200, owned()),
        ("/ISteamNews/", 200, body.into()),
    ])
    .await;
    let s = steam(&api, NO_API, Some(KEY));
    let (_, v) = get_json(&s.cfg, "/api/games/620/news").await;
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert_eq!(v["items"][0]["title"], "Kept");
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
    brisk_hostd::steam_info::warm_games(&s.cfg, vec![620, 400], vec![], Z, Z).await;
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

fn friends_routes() -> Vec<(&'static str, u16, String)> {
    let r = |n: &str| std::fs::read_to_string(format!("{FIX}/{n}.json")).unwrap();
    vec![
        ("/ISteamUser/GetFriendList/", 200, r("friends")),
        ("/ISteamUser/GetPlayerSummaries/", 200, r("summaries")),
    ]
}

async fn friends_names(api: &str) -> (Steam, serde_json::Value) {
    let s = steam(api, NO_API, Some(KEY));
    let (st, v) = get_json(&s.cfg, "/api/friends").await;
    assert_eq!(st, 200, "{v}");
    (s, v)
}

#[tokio::test]
async fn friends_online_sorted_playing_first() {
    let api = mock_steam(friends_routes()).await;
    let (_s, v) = friends_names(&api).await;
    assert_eq!(v["available"], true);
    let names: Vec<_> = v["online"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "TestFriend_A",
            "TestGhost",
            "TestAway",
            "TestEvil",
            "testfriend_b"
        ]
    );
    let a = &v["online"][0];
    assert_eq!(a["playing"], "Portal 2");
    assert_eq!(a["appid"], 620);
    assert_eq!(a["state"], "online");
    assert_eq!(a["avatar"], "https://avatars.steamstatic.com/invented.jpg");
    assert_eq!(v["online"][1]["state"], "online"); // offline persona but in a game
    assert_eq!(v["online"][2]["state"], "away");
    assert_eq!(v["online"][3]["state"], "busy");
    assert_eq!(v["online"][3]["avatar"], ""); // foreign host dropped
}

#[tokio::test]
async fn friends_offline_are_dropped() {
    let api = mock_steam(friends_routes()).await;
    let (_s, v) = friends_names(&api).await;
    assert!(!v.to_string().contains("TestOffline"));
}

#[tokio::test]
async fn friends_private_list_is_unavailable() {
    let api = mock_steam(vec![("/ISteamUser/GetFriendList/", 401, String::new())]).await;
    let (_s, v) = friends_names(&api).await;
    assert_eq!(v, serde_json::json!({"available": false, "online": []}));
    // no key at all: same answer
    let s = steam(&api, NO_API, None);
    let (st, v) = get_json(&s.cfg, "/api/friends").await;
    assert_eq!(st, 200);
    assert_eq!(v["available"], false);
}

#[tokio::test]
async fn friends_cached_in_memory_not_on_disk() {
    let (api, hits) = mock_steam_hits(friends_routes()).await;
    let s = steam(&api, NO_API, Some(KEY));
    let st = Arc::new(AppState::new(s.cfg.clone()));
    let a = brisk_hostd::steam_info::friends_cached(&st).await;
    let n = hits.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(n, 2);
    let b = brisk_hostd::steam_info::friends_cached(&st).await;
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), n);
    assert_eq!(a.online.len(), b.online.len());
    fn walk(d: &std::path::Path, bad: &mut Vec<String>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, bad);
            } else if std::fs::read_to_string(&p).is_ok_and(|t| t.contains("TestFriend_A")) {
                bad.push(p.display().to_string());
            }
        }
    }
    let mut bad = vec![];
    walk(s._t.path(), &mut bad);
    assert!(bad.is_empty(), "names on disk: {bad:?}");
}

#[tokio::test]
async fn friends_failure_is_cached_briefly() {
    let (api, hits) =
        mock_steam_hits(vec![("/ISteamUser/GetFriendList/", 401, String::new())]).await;
    let s = steam(&api, NO_API, Some(KEY));
    let st = Arc::new(AppState::new(s.cfg.clone()));
    for _ in 0..3 {
        assert!(!brisk_hostd::steam_info::friends_cached(&st).await.available);
    }
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn friends_failed_refetch_keeps_the_good_list_and_backs_off() {
    let (api, hits) =
        mock_steam_hits(vec![("/ISteamUser/GetFriendList/", 500, String::new())]).await;
    let s = steam(&api, NO_API, Some(KEY));
    let st = Arc::new(AppState::new(s.cfg.clone()));
    // a good list that has just gone stale (older than the 120 s TTL)
    let good = brisk_hostd::steam_info::Friends {
        available: true,
        online: vec![brisk_hostd::steam_info::Friend {
            name: "TestFriend_A".into(),
            avatar: String::new(),
            state: "online".into(),
            playing: None,
            appid: None,
        }],
    };
    let old = std::time::Instant::now() - std::time::Duration::from_secs(130);
    *st.friends.lock().unwrap() = Some((old, good));
    for _ in 0..3 {
        let f = brisk_hostd::steam_info::friends_cached(&st).await;
        assert!(f.available);
        assert_eq!(f.online[0].name, "TestFriend_A");
    }
    // one failed call, then the backoff holds Steam off
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    // after the backoff it asks again, and still keeps the list
    st.friends.lock().unwrap().as_mut().unwrap().0 = old;
    assert!(brisk_hostd::steam_info::friends_cached(&st).await.available);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test]
async fn friends_batches_over_100() {
    let ids: Vec<_> = (0..150)
        .map(|n| serde_json::json!({"steamid": format!("7656119800001{n:04}")}))
        .collect();
    let list = serde_json::json!({"friendslist": {"friends": ids}}).to_string();
    let (api, hits) = mock_steam_hits(vec![
        ("/ISteamUser/GetFriendList/", 200, list),
        (
            "/ISteamUser/GetPlayerSummaries/",
            200,
            r#"{"response":{"players":[]}}"#.into(),
        ),
    ])
    .await;
    let s = steam(&api, NO_API, Some(KEY));
    let (st, v) = get_json(&s.cfg, "/api/friends").await;
    assert_eq!(st, 200);
    assert_eq!(v["available"], true);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 3);
}

// Account id of the fixture loginusers.vdf's 76561198000000000.
const ACCT: &str = "userdata/39734272";

fn set_mtime(p: &std::path::Path, secs: u64) {
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .unwrap();
}

/// A screenshots folder for appid 620 with `files` as (name, bytes, mtime).
fn shots(s: &Steam, files: &[(&str, &[u8], u64)]) -> std::path::PathBuf {
    let d = s
        .cfg
        .steam_dir
        .join(ACCT)
        .join("760/remote/620/screenshots");
    std::fs::create_dir_all(d.join("thumbnails")).unwrap();
    std::fs::write(d.join("thumbnails/t.jpg"), b"thumb").unwrap();
    for (n, b, t) in files {
        std::fs::write(d.join(n), b).unwrap();
        set_mtime(&d.join(n), *t);
    }
    d
}

async fn get_raw(cfg: &brisk_hostd::config::Config, uri: &str) -> axum::response::Response {
    use tower::ServiceExt;
    brisk_hostd::http::router(Arc::new(AppState::new(cfg.clone())))
        .oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .header("host", H)
                .header("x-brisk-secret", cfg.secret.as_str())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn local_lists_screenshots_newest_first_and_saves_summary() {
    let s = steam(NO_API, NO_API, None);
    shots(
        &s,
        &[
            ("old.jpg", b"a", 1_000),
            ("new.PNG", b"b", 3_000),
            ("mid.jpeg", b"c", 2_000),
            ("notes.txt", b"d", 4_000),
        ],
    );
    let saves = s.cfg.steam_dir.join(ACCT).join("620/remote");
    std::fs::create_dir_all(saves.join("slot1/deep")).unwrap();
    std::fs::write(saves.join("a.sav"), b"12345").unwrap();
    std::fs::write(saves.join("slot1/deep/b.sav"), b"123").unwrap();
    set_mtime(&saves.join("a.sav"), 5_000);
    set_mtime(&saves.join("slot1/deep/b.sav"), 7_000);
    let (st, v) = get_json(&s.cfg, "/api/games/620/local").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        v["screenshots"],
        serde_json::json!(["new.PNG", "mid.jpeg", "old.jpg"])
    );
    assert_eq!(
        v["saves"],
        serde_json::json!({"files": 2, "bytes": 8, "modified": 7000})
    );
    let (st, _) = get_json(&s.cfg, "/api/games/999999/local").await;
    assert_eq!(st, 404);
}

#[tokio::test]
async fn local_caps_screenshots_at_60() {
    let s = steam(NO_API, NO_API, None);
    let names: Vec<String> = (0..70).map(|i| format!("s{i:02}.jpg")).collect();
    let files: Vec<(&str, &[u8], u64)> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), &b"x"[..], 1_000 + i as u64))
        .collect();
    shots(&s, &files);
    let (_, v) = get_json(&s.cfg, "/api/games/620/local").await;
    let l = v["screenshots"].as_array().unwrap();
    assert_eq!(l.len(), 60);
    assert_eq!(l[0], "s69.jpg");
}

#[tokio::test]
async fn local_empty_when_no_userdata() {
    let s = steam(NO_API, NO_API, None);
    let (st, v) = get_json(&s.cfg, "/api/games/620/local").await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v, serde_json::json!({"screenshots": [], "saves": null}));
}

#[tokio::test]
async fn screenshot_serves_listed_file() {
    let s = steam(NO_API, NO_API, None);
    shots(
        &s,
        &[("a.jpg", b"JPEGDATA", 1_000), ("b.PNG", b"PNGDATA", 2_000)],
    );
    let r = get_raw(&s.cfg, "/api/games/620/screenshots/a.jpg").await;
    assert_eq!(r.status(), 200);
    let h = r.headers();
    assert_eq!(h["content-type"], "image/jpeg");
    assert_eq!(h["cache-control"], "private, max-age=3600");
    assert_eq!(h["x-content-type-options"], "nosniff");
    let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    assert_eq!(&b[..], b"JPEGDATA");
    let r = get_raw(&s.cfg, "/api/games/620/screenshots/b.PNG").await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    for uri in [
        "/api/games/620/screenshots/missing.jpg",
        "/api/games/999999/screenshots/a.jpg",
        "/api/games/620/screenshots/t.jpg",
    ] {
        assert_eq!(get_raw(&s.cfg, uri).await.status(), 404, "{uri}");
    }
}

#[tokio::test]
async fn screenshot_rejects_traversal() {
    let s = steam(NO_API, NO_API, None);
    let d = shots(&s, &[("a.jpg", b"x", 1_000)]);
    // a file literally named like the decoded attack must not be reachable either
    std::fs::write(d.join("..jpg"), b"x").unwrap();
    for uri in [
        "/api/games/620/screenshots/..%2F..%2Fconfig%2Floginusers.vdf",
        "/api/games/620/screenshots/..%2F..%2F..%2F..%2F..%2F..%2Fconfig%2Floginusers.vdf",
        "/api/games/620/screenshots//etc/passwd",
        "/api/games/620/screenshots/%2Fetc%2Fpasswd",
        "/api/games/620/screenshots/%00x.jpg",
        "/api/games/620/screenshots/a.jpg%00",
        "/api/games/620/screenshots/..jpg",
        "/api/games/620/screenshots/thumbnails%2Ft.jpg",
        "/api/games/620/screenshots/..%5Ca.jpg",
    ] {
        let r = get_raw(&s.cfg, uri).await;
        assert_eq!(r.status(), 404, "{uri}");
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert!(!String::from_utf8_lossy(&b).contains("users"), "{uri}");
    }
}

#[tokio::test]
async fn screenshot_rejects_symlink_out_of_dir() {
    let s = steam(NO_API, NO_API, None);
    let d = shots(&s, &[("real.jpg", b"x", 1_000)]);
    std::os::unix::fs::symlink(
        s.cfg.steam_dir.join("config/loginusers.vdf"),
        d.join("a.jpg"),
    )
    .unwrap();
    // even a link to a file inside the folder is refused
    std::os::unix::fs::symlink(d.join("real.jpg"), d.join("b.jpg")).unwrap();
    for n in ["a.jpg", "b.jpg"] {
        let r = get_raw(&s.cfg, &format!("/api/games/620/screenshots/{n}")).await;
        assert_eq!(r.status(), 404, "{n}");
    }
    let (_, v) = get_json(&s.cfg, "/api/games/620/local").await;
    assert_eq!(v["screenshots"], serde_json::json!(["real.jpg"]));
}

#[tokio::test]
async fn screenshot_wrong_extension_404() {
    let s = steam(NO_API, NO_API, None);
    let d = shots(&s, &[("a.jpg", b"x", 1_000)]);
    std::fs::write(d.join("a.gif"), b"x").unwrap();
    std::fs::write(d.join("a.vdf"), b"x").unwrap();
    std::fs::create_dir(d.join("dir.jpg")).unwrap();
    for n in ["a.gif", "a.vdf", "dir.jpg", "a.jpg.txt"] {
        let r = get_raw(&s.cfg, &format!("/api/games/620/screenshots/{n}")).await;
        assert_eq!(r.status(), 404, "{n}");
    }
}

#[tokio::test]
async fn screenshot_over_25_mib_404() {
    let s = steam(NO_API, NO_API, None);
    let d = shots(&s, &[("a.jpg", b"x", 1_000)]);
    let big = std::fs::File::create(d.join("big.jpg")).unwrap();
    big.set_len(25 * 1024 * 1024 + 1).unwrap(); // sparse: no real disk use
    let r = get_raw(&s.cfg, "/api/games/620/screenshots/big.jpg").await;
    assert_eq!(r.status(), 404);
}

fn seed_info(cfg: &brisk_hostd::config::Config, name: &str, body: &str) {
    let d = cfg.data_dir.join("steam");
    std::fs::create_dir_all(&d).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        d.join(format!("{name}.json")),
        format!(r#"{{"fetched":{now},"value":{body}}}"#),
    )
    .unwrap();
}

async fn game_of(cfg: &brisk_hostd::config::Config, id: u64) -> serde_json::Value {
    let api = mock_steam(vec![("/IPlayerService/GetOwnedGames/v1/", 200, owned())]).await;
    let mut cfg = cfg.clone();
    cfg.steam_api_base = api;
    let (v, _) = games_json(&cfg, "/api/games").await;
    v["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["appid"] == id)
        .unwrap()
        .clone()
}

#[tokio::test]
async fn games_carry_controller_and_coop_from_info_cache() {
    let s = steam(NO_API, NO_API, Some(KEY));
    seed_info(
        &s.cfg,
        "info-620",
        r#"{"available":true,"description":"","genres":[],"controller":"full","singleplayer":true,"coop":true,"remotePlayTogether":false,"release":null,"metacritic":null,"screenshots":[]}"#,
    );
    let g = game_of(&s.cfg, 620).await;
    assert_eq!(
        (g["controller"].clone(), g["coop"].clone()),
        ("full".into(), true.into())
    );
    let g = game_of(&s.cfg, 730).await;
    assert!(g["controller"].is_null());
    assert_eq!(g["coop"], false);
}

#[tokio::test]
async fn remote_play_counts_as_coop_and_unavailable_entries_are_ignored() {
    let s = steam(NO_API, NO_API, Some(KEY));
    seed_info(
        &s.cfg,
        "info-620-ok",
        r#"{"available":true,"description":"","genres":[],"controller":"partial","singleplayer":true,"coop":false,"remotePlayTogether":true,"release":null,"metacritic":null,"screenshots":[]}"#,
    );
    // the outer entry says "unavailable" (a recent failure): the success entry still counts
    seed_info(
        &s.cfg,
        "info-620",
        r#"{"available":false,"description":"","genres":[],"controller":null,"singleplayer":false,"coop":false,"remotePlayTogether":false,"release":null,"metacritic":null,"screenshots":[]}"#,
    );
    let g = game_of(&s.cfg, 620).await;
    assert_eq!(
        (g["controller"].clone(), g["coop"].clone()),
        ("partial".into(), true.into())
    );
    // only an unavailable entry: nothing
    seed_info(
        &s.cfg,
        "info-730",
        r#"{"available":false,"description":"","genres":[],"controller":"full","singleplayer":false,"coop":true,"remotePlayTogether":false,"release":null,"metacritic":null,"screenshots":[]}"#,
    );
    let g = game_of(&s.cfg, 730).await;
    assert!(g["controller"].is_null());
    assert_eq!(g["coop"], false);
}

#[tokio::test]
async fn games_without_info_cache_have_no_controller() {
    let s = steam(NO_API, NO_API, Some(KEY));
    let g = game_of(&s.cfg, 620).await;
    assert!(g["controller"].is_null());
    assert_eq!(g["coop"], false);
}

#[tokio::test]
async fn warm_fetches_info_for_unplayed_games() {
    let store = mock_steam(vec![("/api/appdetails", 200, appdetails())]).await;
    let s = steam(NO_API, &store, Some(KEY));
    brisk_hostd::steam_info::warm_games(&s.cfg, vec![], vec![220], Z, Z).await;
    assert!(s.cfg.data_dir.join("steam/info-220.json").exists());
}

#[tokio::test]
async fn warm_caps_info_at_200() {
    use std::sync::atomic::Ordering::SeqCst;
    let (store, hits) = mock_steam_hits(vec![("/api/appdetails", 500, String::new())]).await;
    let s = steam(NO_API, &store, Some(KEY));
    brisk_hostd::steam_info::warm_games(&s.cfg, vec![], (1000..1250).collect(), Z, Z).await;
    assert_eq!(hits.load(SeqCst), 200);
}

#[tokio::test]
async fn warm_skips_the_pause_for_cached_info() {
    use std::sync::atomic::Ordering::SeqCst;
    let (store, hits) = mock_steam_hits(vec![("/api/appdetails", 200, appdetails())]).await;
    let s = steam(NO_API, &store, Some(KEY));
    let ok = r#"{"available":true,"description":"","genres":[],"controller":null,"singleplayer":false,"coop":false,"remotePlayTogether":false,"release":null,"metacritic":null,"screenshots":[]}"#;
    for id in 1..=5 {
        seed_info(&s.cfg, &format!("info-{id}-ok"), ok);
    }
    let pause = std::time::Duration::from_millis(200);
    let t = std::time::Instant::now();
    brisk_hostd::steam_info::warm_games(&s.cfg, vec![], (1..=5).collect(), pause, pause).await;
    assert!(
        t.elapsed() < std::time::Duration::from_millis(400),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(hits.load(SeqCst), 0);
}

#[tokio::test]
async fn warm_fetches_every_uncached_game() {
    use std::sync::atomic::Ordering::SeqCst;
    let (store, hits) = mock_steam_hits(vec![("/api/appdetails", 200, appdetails())]).await;
    let s = steam(NO_API, &store, Some(KEY));
    brisk_hostd::steam_info::warm_games(&s.cfg, vec![], (1..=5).collect(), Z, Z).await;
    assert_eq!(hits.load(SeqCst), 5);
}
