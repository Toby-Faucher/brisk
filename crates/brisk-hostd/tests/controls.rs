use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceExt;

const SECRET: &str = "s3cret";
const REPO_INI: &str = include_str!("../../../deploy/dolphin/WiimoteNew.ini");
const GET_FIXTURE: &str = include_str!("fixtures/controls/get.json");
const PUT_FIXTURE: &str = include_str!("fixtures/controls/put.json");
const FILE: &str = "WiimoteNew.ini";
const ROMF: &str = "/games/mkw.iso";

struct T {
    app: axum::Router,
    dir: tempfile::TempDir,
    id: u32,
}

impl T {
    fn dol(&self) -> PathBuf {
        self.dir.path().join("dolphin")
    }
    fn ini(&self) -> PathBuf {
        self.dol().join(FILE)
    }
    fn tmps(&self) -> usize {
        std::fs::read_dir(self.dol())
            .map(|d| {
                d.flatten()
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                    .count()
            })
            .unwrap_or(0)
    }
    fn url(&self) -> String {
        format!("/api/games/{}/controls", self.id)
    }
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
    async fn get(&self) -> (StatusCode, Value) {
        self.call("GET", &self.url(), true, None).await
    }
    async fn put(&self, v: &Value) -> (StatusCode, Value) {
        self.call("PUT", &self.url(), true, Some(v.to_string()))
            .await
    }
    async fn reset(&self) -> (StatusCode, Value) {
        self.call("POST", &format!("{}/reset", self.url()), true, None)
            .await
    }
}

/// `procs`: a Dolphin process on the game file when true.
fn rig(ini: Option<&str>, running: bool) -> T {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("data");
    let proc = d.path().join("proc");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&proc).unwrap();
    std::fs::write(
        data.join("emulators.json"),
        format!(
            r#"[{{"name":"Mario Kart Wii","emulator":"dolphin","file":"{ROMF}"}},{{"name":"Odd","emulator":"mystery","file":"/x"}}]"#
        ),
    )
    .unwrap();
    if running {
        let p = proc.join("900");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("cmdline"), format!("dolphin-emu\0-b\0-e\0{ROMF}\0")).unwrap();
        std::fs::write(
            p.join("stat"),
            "900 (dolphin-emu) S 1 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 1900 0 0",
        )
        .unwrap();
    }
    let mut cfg = Config::from_env(d.path().to_path_buf());
    cfg.secret = SECRET.into();
    cfg.data_dir = data;
    cfg.dolphin_dir = d.path().join("dolphin");
    cfg.proc_dir = proc;
    cfg.steam_dir = d.path().join("nosteam");
    cfg.steam_key_file = d.path().join("nokey");
    cfg.steam_api_base = "http://127.0.0.1:1".into();
    if let Some(t) = ini {
        std::fs::create_dir_all(&cfg.dolphin_dir).unwrap();
        std::fs::write(cfg.dolphin_dir.join(FILE), t).unwrap();
    }
    let id = brisk_hostd::emu::load(&cfg.data_dir)[0].id();
    T {
        app: router(Arc::new(AppState::new(cfg))),
        dir: d,
        id,
    }
}

fn put_body() -> Value {
    serde_json::from_str(PUT_FIXTURE).unwrap()
}

fn no_rev() -> Value {
    let mut v = put_body();
    v.as_object_mut().unwrap().remove("rev");
    v
}

#[tokio::test]
async fn auth_required() {
    let t = rig(Some(REPO_INI), false);
    for (m, u) in [
        ("GET", t.url()),
        ("PUT", t.url()),
        ("POST", format!("{}/reset", t.url())),
    ] {
        let (s, _) = t.call(m, &u, false, Some(no_rev().to_string())).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{m}");
    }
    assert_eq!(t.get().await.0, StatusCode::OK);
}

#[tokio::test]
async fn not_found_for_other_games() {
    let t = rig(Some(REPO_INI), false);
    let odd = brisk_hostd::emu::load(&t.dir.path().join("data"))[1].id();
    for id in [730, 0xE000_0001, odd] {
        let u = format!("/api/games/{id}/controls");
        for (m, u) in [
            ("GET", u.clone()),
            ("PUT", u.clone()),
            ("POST", format!("{u}/reset")),
        ] {
            let (s, v) = t.call(m, &u, true, Some(no_rev().to_string())).await;
            assert_eq!(s, StatusCode::NOT_FOUND, "{id} {m}");
            assert_eq!(v["error"], "no controls for this game");
        }
    }
}

#[tokio::test]
async fn get_shapes_and_fixture() {
    let t = rig(None, false);
    let (s, v) = t.get().await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        (v["source"].as_str(), v["rev"].as_str()),
        (Some("default"), Some("none"))
    );
    assert!(!t.ini().exists(), "a GET never writes");
    let t = rig(Some(REPO_INI), false);
    let (_, v) = t.get().await;
    let mut want: Value = serde_json::from_str(GET_FIXTURE).unwrap();
    assert_eq!(v["source"], "file");
    assert_ne!(v["rev"], "none");
    want["rev"] = v["rev"].clone();
    assert_eq!(v, want);
    assert_eq!(std::fs::read_to_string(t.ini()).unwrap(), REPO_INI);
}

#[tokio::test]
async fn put_writes_file_and_matches_next_get() {
    let t = rig(Some(REPO_INI), false);
    let (_, cur) = t.get().await;
    let mut b = put_body();
    b["rev"] = cur["rev"].clone();
    let (s, v) = t.put(&b).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v, t.get().await.1);
    assert_eq!(v["bindings"]["a"], "A");
    assert_eq!(v["deadZone"], json!({"ls": 10, "rs": 10}));
    assert_eq!(v["appliesOn"], "next-launch");
    let text = std::fs::read_to_string(t.ini()).unwrap();
    for l in [
        "Classic/Buttons/A = SOUTH",
        "Buttons/A = SOUTH",
        "Buttons/1 = WEST",
        "Classic/Left Stick/Dead Zone = 10.000000",
        "Device = evdev/0/Sunshine (libvirtualhid) X-Box Series Controller",
    ] {
        assert!(text.lines().any(|x| x == l), "{l}");
    }
    assert_eq!(
        std::fs::metadata(t.ini()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(t.tmps(), 0);
}

#[tokio::test]
async fn put_creates_missing_file_without_backup() {
    let t = rig(None, false);
    let (s, v) = t.put(&no_rev()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["source"], "file");
    assert!(t.ini().exists() && !t.dol().join("WiimoteNew.ini.before-brisk-edit").exists());
}

#[tokio::test]
async fn backup_made_once() {
    let orig = format!("{REPO_INI}# mine\n");
    let t = rig(Some(&orig), false);
    t.put(&no_rev()).await;
    let bk = t.dol().join("WiimoteNew.ini.before-brisk-edit");
    assert_eq!(std::fs::read_to_string(&bk).unwrap(), orig);
    let mut b = no_rev();
    b["bindings"]["a"] = "B".into();
    b["bindings"]["b"] = "A".into();
    assert_eq!(t.put(&b).await.0, StatusCode::OK);
    assert_eq!(std::fs::read_to_string(&bk).unwrap(), orig);
    assert!(std::fs::read_to_string(t.ini()).unwrap().contains("# mine"));
}

#[tokio::test]
async fn validation_and_limits() {
    let t = rig(Some(REPO_INI), false);
    let mut cases: Vec<(&str, String)> = vec![];
    let mut add = |n: &'static str, f: &dyn Fn(&mut Value)| {
        let mut v = no_rev();
        f(&mut v);
        cases.push((n, v.to_string()));
    };
    add("unknown id", &|v| {
        v["bindings"]["q"] = "A".into();
    });
    add("missing id", &|v| {
        v["bindings"].as_object_mut().unwrap().remove("a");
    });
    add("swapped id", &|v| {
        v["bindings"].as_object_mut().unwrap().remove("a");
        v["bindings"]["../x"] = "A".into();
    });
    add("extra key", &|v| {
        v["x"] = 1.into();
    });
    add("wrong type", &|v| {
        v["bindings"]["a"] = 5.into();
    });
    add("null non-custom", &|v| {
        v["bindings"]["a"] = Value::Null;
    });
    add("button on stick", &|v| {
        v["bindings"]["ls"] = "A".into();
    });
    add("dpad on a", &|v| {
        v["bindings"]["a"] = "D-pad".into();
    });
    add("dup", &|v| {
        v["bindings"]["a"] = "B".into();
    });
    add("injection", &|v| {
        v["bindings"]["a"] = "A\nDevice = x".into();
    });
    add("injection2", &|v| {
        v["bindings"]["a"] = "A\n[Wiimote2]".into();
    });
    add("tick", &|v| {
        v["bindings"]["a"] = "`A`".into();
    });
    add("case", &|v| {
        v["bindings"]["a"] = "a".into();
    });
    add("dz 51", &|v| {
        v["deadZone"]["ls"] = 51.into();
    });
    add("dz -1", &|v| {
        v["deadZone"]["rs"] = (-1).into();
    });
    add("dz 1.5", &|v| {
        v["deadZone"]["ls"] = json!(1.5);
    });
    add("dz extra", &|v| {
        v["deadZone"]["x"] = 1.into();
    });
    add("rev long", &|v| {
        v["rev"] = "x".repeat(33).into();
    });
    cases.push(("bad json", "{".into()));
    for (n, body) in cases {
        let (s, v) = t.call("PUT", &t.url(), true, Some(body)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{n}");
        assert!(v["error"].is_string(), "{n}");
        assert_eq!(std::fs::read_to_string(t.ini()).unwrap(), REPO_INI, "{n}");
    }
    let big = json!({"x": "y".repeat(5000)}).to_string();
    let (s, _) = t.call("PUT", &t.url(), true, Some(big)).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(std::fs::read_to_string(t.ini()).unwrap(), REPO_INI);
    assert!(!t.dol().join("WiimoteNew.ini.before-brisk-edit").exists());
}

#[tokio::test]
async fn custom_controls_kept_until_picked() {
    let odd = REPO_INI.replace("Classic/Buttons/A = EAST", "Classic/Buttons/A = EAST | TL");
    let t = rig(Some(&odd), false);
    let (_, v) = t.get().await;
    assert_eq!(
        (v["custom"]["a"].as_bool(), v["bindings"]["a"].is_null()),
        (Some(true), true)
    );
    let mut b = no_rev();
    b["bindings"]["a"] = Value::Null;
    b["bindings"]["b"] = "B".into();
    b["bindings"]["x"] = "X".into();
    assert_eq!(t.put(&b).await.0, StatusCode::OK);
    let text = std::fs::read_to_string(t.ini()).unwrap();
    assert!(
        text.contains("Classic/Buttons/A = EAST | TL\n") && text.contains("Buttons/A = EAST\n")
    );
    b["bindings"]["a"] = "A".into();
    b["bindings"]["b"] = "B".into();
    let (s, v) = t.put(&b).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["bindings"]["a"], "A");
    assert_eq!(v["custom"]["a"], false);
}

#[tokio::test]
async fn stale_rev_conflicts() {
    let t = rig(Some(REPO_INI), false);
    let mut b = put_body();
    b["rev"] = "stale".into();
    let (s, v) = t.put(&b).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"], "changed elsewhere");
    assert_eq!(v["current"], t.get().await.1);
    assert_eq!(std::fs::read_to_string(t.ini()).unwrap(), REPO_INI);
}

#[tokio::test]
async fn concurrent_puts_leave_a_valid_file() {
    let t = Arc::new(rig(Some(REPO_INI), false));
    let (a, b) = (put_body(), no_rev());
    let (t1, t2) = (t.clone(), t.clone());
    let (r1, r2) = tokio::join!(
        tokio::spawn(async move { t1.put(&a).await.0 }),
        tokio::spawn(async move { t2.put(&b).await.0 })
    );
    // Both carry the same fixture rev only for the first; the other has none. The winner is one of them.
    assert!([r1.unwrap(), r2.unwrap()].contains(&StatusCode::OK));
    let (s, v) = t.get().await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        v["custom"]
            .as_object()
            .unwrap()
            .values()
            .all(|c| c == false)
    );
    assert_eq!(t.tmps(), 0);
}

#[tokio::test]
async fn running_flag_from_process_scan() {
    let t = rig(Some(REPO_INI), true);
    assert_eq!(t.get().await.1["running"], true);
    let (s, v) = t.put(&no_rev()).await;
    assert_eq!(
        (s, &v["running"], &v["appliesOn"]),
        (StatusCode::OK, &json!(true), &json!("next-launch"))
    );
    assert!(
        std::fs::read_to_string(t.ini())
            .unwrap()
            .contains("Classic/Buttons/A = SOUTH")
    );
    assert_eq!(rig(Some(REPO_INI), false).get().await.1["running"], false);
}

#[tokio::test]
async fn io_errors_are_500_and_leave_no_temp() {
    let t = rig(None, false);
    std::fs::create_dir_all(t.ini()).unwrap(); // the target is a directory
    for (s, v) in [t.put(&no_rev()).await, t.reset().await] {
        assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(v["error"], "could not save controls");
    }
    assert!(t.ini().is_dir() && t.tmps() == 0);
    let t = rig(None, false);
    std::fs::create_dir_all(t.dol()).unwrap();
    std::fs::write(t.ini(), [0xff, 0xfe, b'\n']).unwrap();
    assert_eq!(t.put(&no_rev()).await.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(std::fs::read(t.ini()).unwrap(), [0xff, 0xfe, b'\n']);
}

#[tokio::test]
async fn reset_restores_default() {
    let odd = REPO_INI.replace("Classic/Buttons/A = EAST", "Classic/Buttons/A = EAST | TL");
    let t = rig(Some(&odd), false);
    let mut b = no_rev();
    b["bindings"]["a"] = Value::Null;
    t.put(&b).await;
    let (s, v) = t.reset().await;
    assert_eq!(s, StatusCode::OK);
    let mut want: Value = serde_json::from_str(GET_FIXTURE).unwrap();
    want["rev"] = v["rev"].clone();
    assert_eq!(v, want);
    let text = std::fs::read_to_string(t.ini()).unwrap();
    assert!(!text.contains("Dead Zone") && !text.contains("EAST | TL"));
    let bk = t.dol().join("WiimoteNew.ini.before-brisk-edit");
    assert_eq!(std::fs::read_to_string(bk).unwrap(), odd);
}
