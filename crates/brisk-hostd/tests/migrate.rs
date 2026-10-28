//! Stage 1 migration on a realistic old data dir (tests/fixtures/stage1/old-data), against the
//! committed expected files in new-data.
use axum::body::Body;
use axum::http::Request;
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use brisk_hostd::migrate;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/stage1");
const FILES: [&str; 4] = [
    "notes.json",
    "prefs.json",
    "sessions.jsonl",
    "emu-played.json",
];

fn old_dir() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for e in std::fs::read_dir(format!("{FIX}/old-data")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), d.path().join(e.file_name())).unwrap();
    }
    d
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

/// A file as JSON, or for `.jsonl` one JSON value per line.
fn json(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => Value::Array(
            text.lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect(),
        ),
    }
}

fn snapshot(dir: &Path) -> Vec<(String, String)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let n = e.unwrap().file_name().to_string_lossy().into_owned();
            let t = read(dir, &n);
            (n, t)
        })
        .collect();
    v.sort();
    v
}

#[test]
fn old_data_dir_becomes_the_new_one_with_backups() {
    let d = old_dir();
    let old = snapshot(d.path());
    migrate::run(d.path());
    for f in FILES {
        assert_eq!(
            json(&read(d.path(), f)),
            json(&read(Path::new(&format!("{FIX}/new-data")), f)),
            "{f}"
        );
        // the backup is the original, byte for byte
        let before = old.iter().find(|(n, _)| n == f).unwrap();
        assert_eq!(read(d.path(), &format!("{f}.pre-ids")), before.1, "{f}");
    }
    assert_eq!(
        json(&read(d.path(), "library.json")),
        json(&read(Path::new(&format!("{FIX}/new-data")), "library.json"))
    );
    // the old list is kept, untouched
    let before = old.iter().find(|(n, _)| n == "emulators.json").unwrap();
    assert_eq!(read(d.path(), "emulators.json"), before.1);
    // no temp files left behind
    assert!(snapshot(d.path()).iter().all(|(n, _)| !n.ends_with(".tmp")));
}

#[test]
fn a_second_run_changes_nothing() {
    let d = old_dir();
    migrate::run(d.path());
    let once = snapshot(d.path());
    migrate::run(d.path());
    assert_eq!(snapshot(d.path()), once);
}

#[test]
fn the_backup_is_made_once_even_when_a_later_run_rewrites_again() {
    let d = old_dir();
    migrate::run(d.path());
    let backup = read(d.path(), "notes.json.pre-ids");
    // an old client (or a restored file) adds a bare key; the next start migrates it too
    let mut n = json(&read(d.path(), "notes.json"));
    n["games"]["440"] = serde_json::json!({"note":"late","tags":[]});
    std::fs::write(d.path().join("notes.json"), n.to_string()).unwrap();
    migrate::run(d.path());
    assert_eq!(read(d.path(), "notes.json.pre-ids"), backup);
    assert_eq!(
        json(&read(d.path(), "notes.json"))["games"]["steam:440"]["note"],
        "late"
    );
}

#[test]
fn a_corrupt_file_is_left_alone_and_the_others_still_migrate() {
    let d = old_dir();
    std::fs::write(d.path().join("notes.json"), "{ not json").unwrap();
    std::fs::write(d.path().join("sessions.jsonl"), "garbage\n").unwrap();
    migrate::run(d.path());
    assert_eq!(read(d.path(), "notes.json"), "{ not json");
    assert_eq!(read(d.path(), "sessions.jsonl"), "garbage\n");
    assert!(!d.path().join("notes.json.pre-ids").exists());
    assert!(!d.path().join("sessions.jsonl.pre-ids").exists());
    assert!(read(d.path(), "prefs.json").contains("steam:620"));
}

#[test]
fn an_existing_library_file_is_not_replaced_by_the_old_list() {
    let d = old_dir();
    let lib = r#"{"v":1,"games":[{"id":"emu:dolphin:only","emulator":"dolphin","file":"/x","name":"Only"}]}"#;
    std::fs::write(d.path().join("library.json"), lib).unwrap();
    migrate::run(d.path());
    assert_eq!(read(d.path(), "library.json"), lib);
    // and the emulator-range keys have no game to map to: kept, not dropped
    let n = json(&read(d.path(), "notes.json"));
    assert!(n["games"].get("3907440760").is_some() && n["games"].get("steam:620").is_some());
}

#[test]
fn a_corrupt_old_list_makes_no_library_and_leaves_numbers_in_place() {
    let d = old_dir();
    std::fs::write(d.path().join("emulators.json"), "[{").unwrap();
    migrate::run(d.path());
    assert!(!d.path().join("library.json").exists());
    assert_eq!(read(d.path(), "emulators.json"), "[{");
    assert!(
        json(&read(d.path(), "emu-played.json"))
            .get("3907440760")
            .is_some()
    );
}

#[test]
fn nothing_to_migrate_in_an_empty_dir() {
    let d = tempfile::tempdir().unwrap();
    migrate::run(d.path());
    assert!(snapshot(d.path()).is_empty());
}

#[tokio::test]
async fn the_server_serves_the_migrated_data() {
    let d = old_dir();
    migrate::run(d.path());
    let mut cfg = Config::from_env(d.path().to_path_buf());
    cfg.secret = "s3cret".into();
    cfg.data_dir = d.path().to_path_buf();
    cfg.steam_dir = d.path().join("nosteam");
    cfg.steam_api_base = "http://127.0.0.1:1".into();
    cfg.proc_dir = d.path().join("noproc");
    let app = router(Arc::new(AppState::new(cfg)));
    let get = |u: &'static str| {
        let app = app.clone();
        async move {
            let r = app
                .oneshot(
                    Request::builder()
                        .uri(u)
                        .header("host", "localhost:8443")
                        .header("x-brisk-secret", "s3cret")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
            serde_json::from_slice::<Value>(&b).unwrap()
        }
    };
    let notes = get("/api/notes").await;
    assert_eq!(notes["games"]["steam:620"]["note"], "co-op with a friend");
    assert_eq!(
        notes["games"]["emu:dolphin:mario-kart-wii"]["tags"][0],
        "fav"
    );
    let prefs = get("/api/prefs").await;
    assert_eq!(
        prefs["prefs"]["emu:dolphin:super-smash-bros-melee"]["preset"],
        "fast"
    );
    let games = get("/api/games").await;
    let mk = games["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "emu:dolphin:mario-kart-wii")
        .unwrap();
    // the stamp followed the game to its new id
    assert_eq!(mk["lastPlayed"], 1700002000);
    assert_eq!(mk["appid"], 3907440760u64);
    let s = get("/api/sessions").await;
    assert!(
        s["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["gameId"] == "emu:dolphin:mario-kart-wii")
    );
}
