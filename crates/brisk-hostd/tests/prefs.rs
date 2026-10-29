mod common;
use axum::http::StatusCode;
use brisk_hostd::http::{AppState, router};
use brisk_hostd::prefs::{Entry, MAX_ENTRIES, Prefs};
use common::{call, data_cfg};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

struct T {
    app: axum::Router,
    dir: tempfile::TempDir,
}

fn t_in(dir: tempfile::TempDir) -> T {
    let mut cfg = data_cfg(dir.path());
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
        call(&self.app, m, u, secret, body.map(|v| v.to_string())).await
    }
    async fn put(&self, id: &str, v: Value) -> StatusCode {
        self.call("PUT", &format!("/api/prefs/{id}"), true, Some(v))
            .await
            .0
    }
    async fn all(&self) -> Value {
        self.call("GET", "/api/prefs", true, None).await.1["prefs"].clone()
    }
    async fn home(&self) -> Value {
        self.call("GET", "/api/prefs", true, None).await.1["homeOrder"].clone()
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
    assert_eq!(all["steam:7"]["preset"], "custom");
    assert_eq!(all["steam:7"]["codec"], "hevc");
    assert_eq!(all["steam:7"]["width"], 2560);
    assert_eq!(all["steam:7"]["bitrateKbps"], 50000);
    assert!(all["steam:7"]["updated"].as_u64().unwrap() > 0);
    assert!(all["steam:8"].get("width").is_none());
    // last write wins
    t.put("8", json!({"preset":"fast","codec":"hevc"})).await;
    assert_eq!(t.all().await["steam:8"]["preset"], "fast");
    assert_eq!(
        t.call("DELETE", "/api/prefs/8", true, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.call("DELETE", "/api/prefs/8", true, None).await.0,
        StatusCode::NO_CONTENT
    ); // idempotent
    assert!(t.all().await.get("steam:8").is_none());
    assert!(t.all().await.get("steam:7").is_some());
}

#[tokio::test]
async fn named_preset_ignores_out_of_range_custom_numbers() {
    let t = t();
    let b = json!({"preset":"balanced","codec":"h264","width":99999,"fps":1,"bitrateKbps":5});
    assert_eq!(t.put("3", b).await, StatusCode::OK);
    let e = t.all().await["steam:3"].clone();
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
    assert_eq!(t2.all().await["steam:5"]["codec"], "hevc");
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
    assert_eq!(on_disk["steam:9"]["preset"], "fast");
}

#[test]
fn entries_with_bad_values_in_the_file_are_dropped_not_fatal() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("prefs.json");
    std::fs::write(&p, r#"{"1":{"preset":"fast","codec":"h264","updated":1},"x":{"preset":"fast","codec":"h264","updated":1},"2":{"preset":"bogus","codec":"h264","updated":1}}"#).unwrap();
    let all = Prefs::load(p).all();
    assert_eq!(
        all.keys().cloned().collect::<Vec<_>>(),
        vec!["1".to_string()]
    );
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
        p.put_entry(&format!("steam:{i}"), e(1000 + i as u64));
    }
    assert_eq!(p.all().len(), MAX_ENTRIES);
    p.put_entry("steam:99999", e(5000));
    let all = p.all();
    assert_eq!(all.len(), MAX_ENTRIES);
    assert!(!all.contains_key("steam:0"), "oldest dropped");
    assert!(all.contains_key("steam:1") && all.contains_key("steam:99999"));
    // re-writing an existing appid never evicts
    p.put_entry("steam:1", e(6000));
    assert_eq!(p.all().len(), MAX_ENTRIES);
}

#[tokio::test]
async fn home_order_put_get_delete_and_survives_restart() {
    let t = t();
    assert_eq!(t.home().await, json!([]));
    let (st, body) = t
        .call(
            "PUT",
            "/api/prefs/layout",
            true,
            Some(json!({"homeOrder":["recent","host","recent","Bad Id","library"]})),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        body,
        json!({"homeOrder":["recent","host","library"],"machinesOrder":[],"machineColors":{}})
    );
    assert_eq!(t.home().await, json!(["recent", "host", "library"]));
    // per-game prefs are untouched by it and still work next to it
    assert_eq!(
        t.put("5", json!({"preset":"fast","codec":"h264"})).await,
        StatusCode::OK
    );
    assert_eq!(t.all().await.as_object().unwrap().len(), 1);
    let on_disk: Value = serde_json::from_slice(&std::fs::read(t.file()).unwrap()).unwrap();
    assert_eq!(
        on_disk["layout"],
        json!({"v":1,"homeOrder":["recent","host","library"]})
    );
    let t2 = t_in(t.dir);
    assert_eq!(t2.home().await, json!(["recent", "host", "library"]));
    assert_eq!(t2.all().await["steam:5"]["preset"], "fast");
    assert_eq!(
        t2.call("DELETE", "/api/prefs/layout", true, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(t2.home().await, json!([]));
    assert_eq!(t2.all().await["steam:5"]["preset"], "fast");
}

#[tokio::test]
async fn home_order_rejects_bad_bodies_and_needs_the_secret() {
    let t = t();
    for b in [
        json!({"homeOrder":"host"}),
        json!({"homeOrder":[1,2]}),
        json!({"homeOrder":["host"],"x":1}),
        json!({}),
        json!(["host"]),
    ] {
        let (st, _) = t
            .call("PUT", "/api/prefs/layout", true, Some(b.clone()))
            .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{b}");
    }
    for m in ["PUT", "DELETE"] {
        let (st, _) = t
            .call(
                m,
                "/api/prefs/layout",
                false,
                Some(json!({"homeOrder":["host"]})),
            )
            .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m}");
    }
    assert!(!t.file().exists());
}

#[tokio::test]
async fn machines_order_partial_put_empty_put_and_delete() {
    let t = t();
    let put = |b: Value| t.call("PUT", "/api/prefs/layout", true, Some(b));
    let (st, body) = put(json!({"machinesOrder":["vm-1","Bad","vm-1","pc"]})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        body,
        json!({"homeOrder":[],"machinesOrder":["vm-1","pc"],"machineColors":{}})
    );
    let (_, body) = put(json!({"homeOrder":["host"]})).await;
    assert_eq!(
        body,
        json!({"homeOrder":["host"],"machinesOrder":["vm-1","pc"],"machineColors":{}})
    );
    let got = t.call("GET", "/api/prefs", true, None).await.1;
    assert_eq!(got["machinesOrder"], json!(["vm-1", "pc"]));
    assert_eq!(put(json!({})).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        put(json!({"machinesOrder":"x"})).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.call("DELETE", "/api/prefs/layout", true, None).await.0,
        StatusCode::NO_CONTENT
    );
    let got = t.call("GET", "/api/prefs", true, None).await.1;
    assert_eq!(
        (&got["homeOrder"], &got["machinesOrder"]),
        (&json!([]), &json!([]))
    );
}

#[tokio::test]
async fn machine_colors_round_trip_merge_and_reject() {
    let t = t();
    let put = |b: Value| t.call("PUT", "/api/prefs/layout", true, Some(b));
    let got = t.call("GET", "/api/prefs", true, None).await.1;
    assert_eq!(got["machineColors"], json!({}));
    let (st, body) = put(json!({"machineColors":{"vm-1":"#ABC","pc":"#00FF7f"}})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        body,
        json!({"homeOrder":[],"machinesOrder":[],"machineColors":{"vm-1":"#aabbcc","pc":"#00ff7f"}})
    );
    // merge: set one, remove one, remove an unknown id
    let (_, body) = put(json!({"machineColors":{"pc":null,"nope":null,"nas":"#123456"}})).await;
    assert_eq!(
        body["machineColors"],
        json!({"vm-1":"#aabbcc","nas":"#123456"})
    );
    // bad bodies change nothing
    for b in [
        json!({"machineColors":{"ok":"#fff","Bad":"#fff"}}),
        json!({"machineColors":{"ok":"#fff","x":"red"}}),
        json!({"machineColors":{"ok":"#fff","x":"javascript:1"}}),
        json!({"machineColors":{"x":5}}),
        json!({"machineColors":[]}),
    ] {
        assert_eq!(put(b.clone()).await.0, StatusCode::BAD_REQUEST, "{b}");
    }
    let over: serde_json::Map<String, Value> =
        (0..64).map(|i| (format!("m{i}"), json!("#000"))).collect();
    assert_eq!(
        put(json!({"machineColors":over})).await.0,
        StatusCode::BAD_REQUEST
    );
    let got = t.call("GET", "/api/prefs", true, None).await.1;
    assert_eq!(
        got["machineColors"],
        json!({"vm-1":"#aabbcc","nas":"#123456"})
    );
    let on_disk: Value = serde_json::from_slice(&std::fs::read(t.file()).unwrap()).unwrap();
    assert_eq!(on_disk["layout"]["machineColors"], got["machineColors"]);
    assert_eq!(
        t.call("DELETE", "/api/prefs/layout", true, None).await.0,
        StatusCode::NO_CONTENT
    );
    let got = t.call("GET", "/api/prefs", true, None).await.1;
    assert_eq!(got["machineColors"], json!({}));
    assert!(
        !std::fs::read_to_string(t.file())
            .unwrap()
            .contains("layout")
    );
}

#[tokio::test]
async fn prefs_take_new_ids_in_any_form_and_key_them_by_the_new_id() {
    let t = t();
    let b = json!({"preset":"fast","codec":"h264"});
    for u in [
        "steam:620",
        "steam%3A621",
        "622",
        "emu:dolphin:mario-kart-wii",
    ] {
        assert_eq!(t.put(u, b.clone()).await, StatusCode::OK, "{u}");
    }
    let all = t.all().await;
    let mut keys: Vec<_> = all.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "emu:dolphin:mario-kart-wii",
            "steam:620",
            "steam:621",
            "steam:622"
        ]
    );
    for u in ["steam%3A620", "622", "emu%3Adolphin%3Amario-kart-wii"] {
        assert_eq!(
            t.call("DELETE", &format!("/api/prefs/{u}"), true, None)
                .await
                .0,
            StatusCode::NO_CONTENT,
            "{u}"
        );
    }
    assert_eq!(t.all().await.as_object().unwrap().len(), 1);
    // the Desktop app is refused in the new form too (BRISK_APP defaults to 870779643)
    for bad in [
        "steam:0",
        "steam:870779643",
        "steam:0620",
        "emu:x",
        "3758096385",
    ] {
        assert_eq!(
            t.put(bad, b.clone()).await,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
}
