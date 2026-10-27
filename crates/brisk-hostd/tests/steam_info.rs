mod common;
use common::*;

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
    assert!(!shots.is_empty() && shots.len() <= 6);
    assert!(shots[0].as_str().unwrap().contains("600x338"));
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
