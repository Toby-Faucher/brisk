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
