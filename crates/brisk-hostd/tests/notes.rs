use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use brisk_hostd::notes::NotesStore;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use tower::ServiceExt;

const SECRET: &str = "s3cret";

struct T {
    app: axum::Router,
    dir: tempfile::TempDir,
}

fn cfg(dir: &tempfile::TempDir) -> Config {
    let mut cfg = Config::from_env(dir.path().to_path_buf());
    cfg.secret = SECRET.into();
    cfg.data_dir = dir.path().to_path_buf();
    cfg
}

fn t_in(dir: tempfile::TempDir) -> T {
    T {
        app: router(Arc::new(AppState::new(cfg(&dir)))),
        dir,
    }
}

fn t() -> T {
    t_in(tempfile::tempdir().unwrap())
}

/// A store with small caps, so a full store is quick to reach.
fn t_small(max_tags: usize, max_games: usize) -> T {
    let dir = tempfile::tempdir().unwrap();
    let mut st = AppState::new(cfg(&dir));
    st.notes = NotesStore::with_limits(dir.path().join("notes.json"), max_tags, max_games);
    T {
        app: router(Arc::new(st)),
        dir,
    }
}

impl T {
    async fn call(
        &self,
        m: &str,
        u: &str,
        secret: bool,
        body: Option<String>,
    ) -> (StatusCode, Value) {
        let mut r = Request::builder()
            .method(m)
            .uri(u)
            .header("host", "localhost:8443");
        if secret {
            r = r.header("x-brisk-secret", SECRET);
        }
        let b = body.map_or(Body::empty(), Body::from);
        let res = self.app.clone().oneshot(r.body(b).unwrap()).await.unwrap();
        let st = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (st, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
    async fn tag(&self, id: &str, v: Value) -> (StatusCode, Value) {
        self.call(
            "PUT",
            &format!("/api/notes/tags/{id}"),
            true,
            Some(v.to_string()),
        )
        .await
    }
    async fn game(&self, appid: u32, v: Value) -> (StatusCode, Value) {
        self.call(
            "PUT",
            &format!("/api/notes/games/{appid}"),
            true,
            Some(v.to_string()),
        )
        .await
    }
    async fn all(&self) -> Value {
        self.call("GET", "/api/notes", true, None).await.1
    }
    fn file(&self) -> std::path::PathBuf {
        self.dir.path().join("notes.json")
    }
}

fn coop() -> Value {
    json!({"name":"Co-op night","color":"teal","symbol":"users","note":"Needs 4 controllers","pinned":false})
}

#[tokio::test]
async fn put_tag_roundtrip_and_persists_across_load() {
    let t = t();
    assert_eq!(t.all().await, json!({"tags":{},"games":{}}));
    let (st, body) = t
        .tag("coop", json!({"name":"  Co-op night ","color":"teal","symbol":"users","note":"Needs 4 controllers\n\n"}))
        .await;
    assert_eq!(st, StatusCode::OK);
    let mut sent = coop();
    sent["note"] = json!("Needs 4 controllers\n\n");
    assert_eq!(body, sent); // name trimmed, the note kept as sent
    // PUT of an existing id replaces it
    let (st, _) = t
        .tag(
            "back",
            json!({"name":"Backlog","color":"gray","symbol":"clock"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    t.game(620, json!({"note":"line 1\nline 2","tags":["coop","back"]}))
        .await;
    let t2 = t_in(t.dir);
    let all = t2.all().await;
    assert_eq!(all["tags"]["coop"], sent);
    assert_eq!(all["tags"]["back"]["note"], "");
    assert_eq!(
        all["games"]["steam:620"],
        json!({"note":"line 1\nline 2","tags":["coop","back"]})
    );
}

#[tokio::test]
async fn tag_rejects_bad_color() {
    let t = t();
    for c in ["", "Red", "black", "#ff0000", "teal "] {
        let (st, _) = t
            .tag("a", json!({"name":"A","color":c,"symbol":"star"}))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{c}");
    }
    for c in [
        "gray", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
    ] {
        let (st, _) = t
            .tag("a", json!({"name":"A","color":c,"symbol":"star"}))
            .await;
        assert_eq!(st, StatusCode::OK, "{c}");
    }
}

#[tokio::test]
async fn tag_rejects_bad_id_and_empty_name() {
    let t = t();
    let ok = json!({"name":"A","color":"red","symbol":"star"});
    for id in ["Abc", "a_b", "a%20b", "%C3%A9", "aaaaaaaaaaaaaaaaa"] {
        assert_eq!(
            t.tag(id, ok.clone()).await.0,
            StatusCode::BAD_REQUEST,
            "{id}"
        );
    }
    assert_eq!(
        t.tag("aaaaaaaaaaaaaaaa", ok.clone()).await.0,
        StatusCode::OK
    );
    for b in [
        json!({"name":"   ","color":"red","symbol":"star"}),
        json!({"name":"","color":"red","symbol":"star"}),
        json!({"name":"x".repeat(25),"color":"red","symbol":"star"}),
        json!({"name":"A","color":"red","symbol":"Star"}),
        json!({"name":"A","color":"red","symbol":""}),
        json!({"name":"A","color":"red","symbol":"a".repeat(17)}),
        json!({"name":"A","color":"red","symbol":"star","extra":1}),
        json!({"color":"red","symbol":"star"}),
    ] {
        assert_eq!(
            t.tag("b", b.clone()).await.0,
            StatusCode::BAD_REQUEST,
            "{b}"
        );
    }
    let (st, _) = t
        .call("PUT", "/api/notes/tags/b", true, Some("{ nope".into()))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // a 24-char name of multi-byte chars is fine
    assert_eq!(
        t.tag(
            "c",
            json!({"name":"é".repeat(24),"color":"red","symbol":"star"})
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn tag_note_over_500_chars_is_400() {
    let t = t();
    let b = |n: usize| json!({"name":"A","color":"red","symbol":"star","note":"n".repeat(n)});
    assert_eq!(t.tag("a", b(500)).await.0, StatusCode::OK);
    assert_eq!(t.tag("a", b(501)).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn game_note_over_4000_chars_is_400() {
    let t = t();
    assert_eq!(
        t.game(1, json!({"note":"x".repeat(4000)})).await.0,
        StatusCode::OK
    );
    assert_eq!(
        t.game(1, json!({"note":"x".repeat(4001)})).await.0,
        StatusCode::BAD_REQUEST
    );
    // trailing whitespace does not count, and is kept
    let long = format!("{}\n  \n", "x".repeat(4000));
    let (st, body) = t.game(1, json!({ "note": long })).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["note"], long.as_str());
    for b in [json!({"note":"a","x":1}), json!("a"), json!({"note":5})] {
        assert_eq!(t.game(1, b.clone()).await.0, StatusCode::BAD_REQUEST, "{b}");
    }
}

#[tokio::test]
async fn game_note_unicode_counts_chars_not_bytes() {
    let t = t();
    let (st, body) = t.game(2, json!({"note":"🎮".repeat(4000)})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["note"].as_str().unwrap().chars().count(), 4000);
    assert_eq!(
        t.game(2, json!({"note":"🎮".repeat(4001)})).await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn put_game_drops_unknown_tag_ids() {
    let t = t();
    t.tag("coop", coop()).await;
    let (st, body) = t
        .game(7, json!({"note":"","tags":["nope","coop","coop","Bad Id"]}))
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body, json!({"note":"","tags":["coop"]}));
    // 16 tags is the most a game carries
    for i in 0..17 {
        t.tag(&format!("t{i}"), coop()).await;
    }
    let ids = |n: usize| (0..n).map(|i| format!("t{i}")).collect::<Vec<_>>();
    assert_eq!(t.game(7, json!({"tags":ids(16)})).await.0, StatusCode::OK);
    assert_eq!(
        t.game(7, json!({"tags":ids(17)})).await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn empty_game_note_without_tags_deletes_entry() {
    let t = t();
    t.game(
        9,
        json!({"note":"keep the <img src=x onerror=alert(1)> as text"}),
    )
    .await;
    assert!(t.all().await["games"].get("steam:9").is_some());
    let (st, body) = t.game(9, json!({"note":"  \n ","tags":[]})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body, json!({"note":"  \n ","tags":[]}));
    assert!(t.all().await["games"].get("steam:9").is_none());
}

#[tokio::test]
async fn delete_tag_removes_it_from_games() {
    let t = t();
    t.tag("coop", coop()).await;
    t.tag(
        "back",
        json!({"name":"Backlog","color":"gray","symbol":"clock"}),
    )
    .await;
    t.game(1, json!({"tags":["coop"]})).await;
    t.game(2, json!({"note":"hi","tags":["coop","back"]})).await;
    let (st, _) = t.call("DELETE", "/api/notes/tags/coop", true, None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let all = t.all().await;
    assert!(all["tags"].get("coop").is_none());
    assert!(all["games"].get("steam:1").is_none(), "left empty, so gone");
    assert_eq!(
        all["games"]["steam:2"],
        json!({"note":"hi","tags":["back"]})
    );
    // unknown tag: still 204
    let (st, _) = t.call("DELETE", "/api/notes/tags/coop", true, None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    // and it is on disk
    let on_disk: Value = serde_json::from_slice(&std::fs::read(t.file()).unwrap()).unwrap();
    assert_eq!(on_disk["games"]["steam:2"]["tags"], json!(["back"]));
}

#[tokio::test]
async fn store_full_is_409() {
    let t = t_small(2, 2);
    assert_eq!(t.tag("a", coop()).await.0, StatusCode::OK);
    assert_eq!(t.tag("b", coop()).await.0, StatusCode::OK);
    assert_eq!(t.tag("c", coop()).await.0, StatusCode::CONFLICT);
    assert_eq!(
        t.tag("a", coop()).await.0,
        StatusCode::OK,
        "replacing is fine"
    );
    assert_eq!(t.game(1, json!({"note":"a"})).await.0, StatusCode::OK);
    assert_eq!(t.game(2, json!({"note":"b"})).await.0, StatusCode::OK);
    assert_eq!(t.game(3, json!({"note":"c"})).await.0, StatusCode::CONFLICT);
    assert_eq!(t.game(2, json!({"note":"b2"})).await.0, StatusCode::OK);
    // clearing one makes room
    assert_eq!(t.game(2, json!({"note":""})).await.0, StatusCode::OK);
    assert_eq!(t.game(3, json!({"note":"c"})).await.0, StatusCode::OK);
}

#[tokio::test]
async fn corrupt_file_loads_empty_and_next_save_works() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("notes.json"), "{ not json").unwrap();
    let t = t_in(d);
    assert_eq!(t.all().await, json!({"tags":{},"games":{}}));
    assert_eq!(
        std::fs::read_to_string(t.file()).unwrap(),
        "{ not json",
        "kept until a save replaces it"
    );
    assert_eq!(t.game(5, json!({"note":"after"})).await.0, StatusCode::OK);
    let on_disk: Value = serde_json::from_slice(&std::fs::read(t.file()).unwrap()).unwrap();
    assert_eq!(on_disk["games"]["steam:5"]["note"], "after");
}

#[tokio::test]
async fn file_is_0600() {
    let t = t();
    t.game(5, json!({"note":"private"})).await;
    let mode = std::fs::metadata(t.file()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let names: Vec<_> = std::fs::read_dir(t.dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        vec![std::ffi::OsString::from("notes.json")],
        "no temp leftovers"
    );
}

#[tokio::test]
async fn body_limits() {
    let t = t();
    // 64 KiB for a game note, 16 KiB for a tag
    let big = json!({"note":"x".repeat(70 << 10)}).to_string();
    let (st, _) = t.call("PUT", "/api/notes/games/1", true, Some(big)).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
    let tag =
        json!({"name":"A","color":"red","symbol":"star","note":"x".repeat(17 << 10)}).to_string();
    let (st, _) = t.call("PUT", "/api/notes/tags/a", true, Some(tag)).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn notes_require_guard() {
    let t = t();
    for (m, u) in [
        ("GET", "/api/notes"),
        ("PUT", "/api/notes/tags/a"),
        ("DELETE", "/api/notes/tags/a"),
        ("PUT", "/api/notes/games/1"),
    ] {
        let (st, _) = t.call(m, u, false, Some(coop().to_string())).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m} {u}");
    }
    assert!(!t.file().exists());
}

#[tokio::test]
async fn tag_pinned_roundtrips_and_defaults_false() {
    let t = t();
    let (st, body) = t
        .tag(
            "a",
            json!({"name":"A","color":"red","symbol":"star","pinned":true}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["pinned"], true);
    let (_, body) = t
        .tag("b", json!({"name":"B","color":"red","symbol":"star"}))
        .await;
    assert_eq!(body["pinned"], false);
    let t2 = t_in(t.dir);
    assert_eq!(t2.all().await["tags"]["a"]["pinned"], true);
    // an old file without the field loads, unpinned
    std::fs::write(
        t2.file(),
        r#"{"tags":{"o":{"name":"Old","color":"red","symbol":"star","note":"x"}},"games":{}}"#,
    )
    .unwrap();
    let t3 = t_in(t2.dir);
    assert_eq!(t3.all().await["tags"]["o"]["pinned"], false);
}

#[tokio::test]
async fn game_notes_take_new_ids_in_any_form_and_a_bare_number_as_steam() {
    let t = t();
    for u in ["steam:620", "steam%3A620", "620"] {
        let (st, _) = t
            .call(
                "PUT",
                &format!("/api/notes/games/{u}"),
                true,
                Some(json!({"note": u}).to_string()),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "{u}");
    }
    // all three are one entry, under the new id
    let all = t.all().await;
    assert_eq!(all["games"].as_object().unwrap().len(), 1);
    assert_eq!(all["games"]["steam:620"]["note"], "620");
    let (st, _) = t
        .call(
            "PUT",
            "/api/notes/games/emu:dolphin:mario-kart-wii",
            true,
            Some(json!({"note":"x"}).to_string()),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    for bad in [
        "nope",
        "steam:0620",
        "emu:dolphin",
        "3758096385",
        "4294967296",
        "-1",
    ] {
        let (st, _) = t
            .call(
                "PUT",
                &format!("/api/notes/games/{bad}"),
                true,
                Some(json!({"note":"x"}).to_string()),
            )
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(t.all().await["games"].as_object().unwrap().len(), 2);
}

#[tokio::test]
async fn an_old_emulator_number_reaches_the_game_when_the_library_has_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("emulators.json"),
        r#"[{"name":"Mario Kart Wii","emulator":"dolphin","file":"/x.iso"}]"#,
    )
    .unwrap();
    let t = t_in(dir);
    let (st, _) = t
        .call(
            "PUT",
            "/api/notes/games/3907440760",
            true,
            Some(json!({"note":"hi"}).to_string()),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        t.all().await["games"]["emu:dolphin:mario-kart-wii"]["note"],
        "hi"
    );
}

#[tokio::test]
async fn remap_moves_emulator_keys_the_hub_could_not_map_and_keeps_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let old = r#"{"tags":{},"games":{"3907440760":{"note":"hub note","tags":[]},"3907440999":{"note":"other","tags":[]}}}"#;
    std::fs::write(dir.path().join("notes.json"), old).unwrap();
    std::fs::write(
        dir.path().join("prefs.json"),
        r#"{"3907440760":{"preset":"fast","codec":"h264","updated":1}}"#,
    )
    .unwrap();
    let t = t_in(dir);
    let body = |m: Value| Some(json!({ "map": m }).to_string());
    let go = |m: Value| t.call("POST", "/api/ids/remap", true, body(m));
    let (st, v) = go(json!({"3907440760": "emu:dolphin:mario-kart-wii"})).await;
    assert_eq!((st, v), (StatusCode::OK, json!({"notes": 1, "prefs": 1})));
    let all = t.all().await;
    assert_eq!(
        all["games"]["emu:dolphin:mario-kart-wii"]["note"],
        "hub note"
    );
    assert!(
        all["games"].get("3907440999").is_some(),
        "an unlisted key stays"
    );
    assert_eq!(
        std::fs::read_to_string(t.dir.path().join("notes.json.pre-ids")).unwrap(),
        old
    );
    assert!(t.dir.path().join("prefs.json.pre-ids").exists());
    // again: nothing left to move, the backup is still the original
    let (_, v) = go(json!({"3907440760": "emu:dolphin:mario-kart-wii"})).await;
    assert_eq!(v, json!({"notes": 0, "prefs": 0}));
    assert_eq!(
        std::fs::read_to_string(t.dir.path().join("notes.json.pre-ids")).unwrap(),
        old
    );
    // only old emulator numbers to new emu ids
    for bad in [
        json!({"620": "emu:dolphin:x"}),
        json!({"3907440760": "steam:620"}),
        json!({"3907440760": "nope"}),
        json!({"x": "emu:dolphin:x"}),
    ] {
        assert_eq!(go(bad.clone()).await.0, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(
        t.call("POST", "/api/ids/remap", true, Some("{}".into()))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.call("POST", "/api/ids/remap", false, body(json!({})))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn remap_on_a_machine_with_a_library_ignores_unknown_targets_and_ambiguous_numbers() {
    use brisk_hostd::emu::{Record, library_json};
    let dir = tempfile::tempdir().unwrap();
    let rec = |id: &str, legacy: u32| Record {
        id: brisk_hostd::game::GameId::parse(id).unwrap(),
        emulator: "dolphin".into(),
        system: None,
        file: format!("/g/{id}.iso").replace(':', "_").into(),
        name: id.into(),
        added_at: None,
        source: "scan".into(),
        legacy_id: Some(legacy),
        aliases: vec![],
    };
    let recs = [
        rec("emu:dolphin:AAAAAA", 0xE000_0001),
        rec("emu:dolphin:BBBBBB", 0xE000_0001),
        rec("emu:dolphin:CCCCCC", 0xE000_0002),
    ];
    std::fs::write(dir.path().join("library.json"), library_json(&recs)).unwrap();
    let game = r#"{"note":"n","tags":[]}"#;
    std::fs::write(
        dir.path().join("notes.json"),
        format!(r#"{{"tags":{{}},"games":{{"3758096385":{game},"3758096386":{game},"3758096387":{game}}}}}"#),
    )
    .unwrap();
    let t = t_in(dir);
    let (st, v) = t
        .call(
            "POST",
            "/api/ids/remap",
            true,
            Some(
                json!({"map": {
                    "3758096385": "emu:dolphin:AAAAAA",
                    "3758096386": "emu:dolphin:CCCCCC",
                    "3758096387": "emu:dolphin:ZZZZZZ"
                }})
                .to_string(),
            ),
        )
        .await;
    assert_eq!((st, v), (StatusCode::OK, json!({"notes": 1, "prefs": 0})));
    let g = t.all().await["games"].clone();
    assert!(g.get("emu:dolphin:CCCCCC").is_some());
    for k in ["3758096385", "3758096387"] {
        assert!(g.get(k).is_some(), "{k} stays");
    }
}
