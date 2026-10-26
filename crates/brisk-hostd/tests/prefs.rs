use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use brisk_hostd::prefs::{Entry, MAX_ENTRIES, Prefs};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use tower::ServiceExt;

const SECRET: &str = "s3cret";

struct T {
    app: axum::Router,
    dir: tempfile::TempDir,
}

fn t_in(dir: tempfile::TempDir) -> T {
    let mut cfg = Config::from_env(dir.path().to_path_buf());
    cfg.secret = SECRET.into();
    cfg.data_dir = dir.path().to_path_buf();
    cfg.app_id = 870779643;
    T {
        app: router(Arc::new(AppState::new(cfg))),
        dir,
    }
}

fn t() -> T {
    t_in(tempfile::tempdir().unwrap())
}

impl T {
    async fn call(
        &self,
        m: &str,
        u: &str,
        secret: bool,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut r = Request::builder()
            .method(m)
            .uri(u)
            .header("host", "localhost:8443");
        if secret {
            r = r.header("x-brisk-secret", SECRET);
        }
        let b = body.map_or(Body::empty(), |v| Body::from(v.to_string()));
        let res = self.app.clone().oneshot(r.body(b).unwrap()).await.unwrap();
        let st = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (st, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }
    async fn put(&self, id: &str, v: Value) -> StatusCode {
        self.call("PUT", &format!("/api/prefs/{id}"), true, Some(v))
            .await
            .0
    }
    async fn all(&self) -> Value {
        self.call("GET", "/api/prefs", true, None).await.1["prefs"].clone()
    }
    fn file(&self) -> std::path::PathBuf {
        self.dir.path().join("prefs.json")
    }
}

#[tokio::test]
async fn validation_table() {
    let t = t();
    let ok = [
        json!({"preset":"fast","codec":"h264"}),
        json!({"preset":"crisp","codec":"hevc"}),
        json!({"preset":"custom","codec":"h264","width":320,"height":3840,"fps":24,"bitrateKbps":1000}),
        json!({"preset":"custom","codec":"hevc","width":3840,"height":320,"fps":144,"bitrateKbps":150000}),
        json!({"preset":"custom","codec":"h264"}),
    ];
    for b in ok {
        assert_eq!(t.put("100", b.clone()).await, StatusCode::OK, "{b}");
    }
    let bad = [
        json!({"preset":"ultra","codec":"h264"}),
        json!({"preset":"fast","codec":"av1"}),
        json!({"codec":"h264"}),
        json!({"preset":"custom","codec":"h264","width":319}),
        json!({"preset":"custom","codec":"h264","width":3841}),
        json!({"preset":"custom","codec":"h264","height":319}),
        json!({"preset":"custom","codec":"h264","fps":23}),
        json!({"preset":"custom","codec":"h264","fps":145}),
        json!({"preset":"custom","codec":"h264","bitrateKbps":999}),
        json!({"preset":"custom","codec":"h264","bitrateKbps":150001}),
        json!({"preset":"custom","codec":"h264","width":-1}),
        json!("nope"),
        json!({"preset":"fast","codec":"h264","colour":"red"}), // unknown field
    ];
    for b in bad {
        assert_eq!(
            t.put("100", b.clone()).await,
            StatusCode::BAD_REQUEST,
            "{b}"
        );
    }
    // appid must be a u32, and never the Desktop app
    for id in ["abc", "-1", "0", "4294967296", "870779643"] {
        assert_eq!(
            t.put(id, json!({"preset":"fast","codec":"h264"})).await,
            StatusCode::BAD_REQUEST,
            "{id}"
        );
    }
    assert_eq!(t.all().await.as_object().unwrap().len(), 1); // only the accepted appid 100
}

#[tokio::test]
async fn put_get_delete_roundtrip() {
    let t = t();
    assert_eq!(t.all().await, json!({}));
    // custom fields are kept; a named preset drops them
    t.put("7", json!({"preset":"custom","codec":"hevc","width":2560,"height":1440,"fps":120,"bitrateKbps":50000})).await;
    t.put("8", json!({"preset":"crisp","codec":"h264","width":2560}))
        .await;
    let all = t.all().await;
    assert_eq!(all["7"]["preset"], "custom");
    assert_eq!(all["7"]["codec"], "hevc");
    assert_eq!(all["7"]["width"], 2560);
    assert_eq!(all["7"]["bitrateKbps"], 50000);
    assert!(all["7"]["updated"].as_u64().unwrap() > 0);
    assert!(all["8"].get("width").is_none());
    // last write wins
    t.put("8", json!({"preset":"fast","codec":"hevc"})).await;
    assert_eq!(t.all().await["8"]["preset"], "fast");
    assert_eq!(
        t.call("DELETE", "/api/prefs/8", true, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.call("DELETE", "/api/prefs/8", true, None).await.0,
        StatusCode::NO_CONTENT
    ); // idempotent
    assert!(t.all().await.get("8").is_none());
    assert!(t.all().await.get("7").is_some());
}

#[tokio::test]
async fn named_preset_ignores_out_of_range_custom_numbers() {
    let t = t();
    let b = json!({"preset":"balanced","codec":"h264","width":99999,"fps":1,"bitrateKbps":5});
    assert_eq!(t.put("3", b).await, StatusCode::OK);
    let e = t.all().await["3"].clone();
    assert!(e.get("width").is_none() && e.get("fps").is_none() && e.get("bitrateKbps").is_none());
}

#[tokio::test]
async fn concurrent_puts_all_land_and_the_file_stays_valid() {
    let t = Arc::new(t());
    let mut js = vec![];
    for i in 1..=40u32 {
        let t = t.clone();
        js.push(tokio::spawn(async move {
            t.put(&i.to_string(), json!({"preset":"fast","codec":"h264"}))
                .await
        }));
    }
    for j in js {
        assert_eq!(j.await.unwrap(), StatusCode::OK);
    }
    assert_eq!(t.all().await.as_object().unwrap().len(), 40);
    let on_disk: Value = serde_json::from_slice(&std::fs::read(t.file()).unwrap()).unwrap();
    assert_eq!(on_disk.as_object().unwrap().len(), 40);
}

#[tokio::test]
async fn needs_the_secret() {
    let t = t();
    for (m, u) in [
        ("GET", "/api/prefs"),
        ("PUT", "/api/prefs/1"),
        ("DELETE", "/api/prefs/1"),
    ] {
        let (st, _) = t
            .call(m, u, false, Some(json!({"preset":"fast","codec":"h264"})))
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m} {u}");
    }
    assert!(!t.file().exists());
}

#[tokio::test]
async fn file_is_0600_atomic_and_survives_restart() {
    let t = t();
    t.put("5", json!({"preset":"crisp","codec":"hevc"})).await;
    let mode = std::fs::metadata(t.file()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    // no temp leftovers
    let names: Vec<_> = std::fs::read_dir(t.dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("prefs.json")]);
    // a new process reads it back
    let t2 = t_in(t.dir);
    assert_eq!(t2.all().await["5"]["codec"], "hevc");
}

#[tokio::test]
async fn corrupt_or_missing_file_starts_empty_and_recovers() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("prefs.json"), "{ not json").unwrap();
    let t = t_in(d);
    assert_eq!(t.all().await, json!({}));
    assert_eq!(
        t.put("9", json!({"preset":"fast","codec":"h264"})).await,
        StatusCode::OK
    );
    let on_disk: Value = serde_json::from_slice(&std::fs::read(t.file()).unwrap()).unwrap();
    assert_eq!(on_disk["9"]["preset"], "fast");
}

#[test]
fn entries_with_bad_values_in_the_file_are_dropped_not_fatal() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("prefs.json");
    std::fs::write(&p, r#"{"1":{"preset":"fast","codec":"h264","updated":1},"x":{"preset":"fast","codec":"h264","updated":1},"2":{"preset":"bogus","codec":"h264","updated":1}}"#).unwrap();
    let all = Prefs::load(p).all();
    assert_eq!(all.keys().copied().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn cap_drops_the_oldest_by_updated() {
    let d = tempfile::tempdir().unwrap();
    let p = Prefs::load(d.path().join("prefs.json"));
    let e = |u| Entry {
        preset: brisk_hostd::stream::Preset::Fast,
        codec: Default::default(),
        width: None,
        height: None,
        fps: None,
        bitrate_kbps: None,
        updated: u,
    };
    for i in 0..MAX_ENTRIES as u32 {
        p.put_entry(i, e(1000 + i as u64));
    }
    assert_eq!(p.all().len(), MAX_ENTRIES);
    p.put_entry(99_999, e(5000));
    let all = p.all();
    assert_eq!(all.len(), MAX_ENTRIES);
    assert!(!all.contains_key(&0), "oldest dropped");
    assert!(all.contains_key(&1) && all.contains_key(&99_999));
    // re-writing an existing appid never evicts
    p.put_entry(1, e(6000));
    assert_eq!(p.all().len(), MAX_ENTRIES);
}
