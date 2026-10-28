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
const CLASSIC_INI: &str = include_str!("../../../deploy/dolphin/WiimoteNew.ini");
const GC_INI: &str = include_str!("../../../deploy/dolphin/GCPadNew.ini");
const CLASSIC_GET: &str = include_str!("fixtures/controls/v2-classic-get.json");
const CLASSIC_PUT: &str = include_str!("fixtures/controls/v2-classic-put.json");
const GC_GET: &str = include_str!("fixtures/controls/v2-gamecube-get.json");
const GC_PUT: &str = include_str!("fixtures/controls/v2-gamecube-put.json");
const MAIN: &str = "Dolphin.ini";
const ROMF: &str = "/games/mkw.iso";

#[derive(Clone, Copy)]
struct K {
    ty: &'static str,
    file: &'static str,
    ini: &'static str,
    get: &'static str,
    put: &'static str,
}
const CL: K = K {
    ty: "classic",
    file: "WiimoteNew.ini",
    ini: CLASSIC_INI,
    get: CLASSIC_GET,
    put: CLASSIC_PUT,
};
const GC: K = K {
    ty: "gamecube",
    file: "GCPadNew.ini",
    ini: GC_INI,
    get: GC_GET,
    put: GC_PUT,
};
const BOTH: [K; 2] = [CL, GC];

impl K {
    fn url(&self) -> String {
        format!("/api/emulators/dolphin/controls/{}", self.ty)
    }
    fn bk(&self) -> String {
        format!("{}.before-brisk-edit", self.file)
    }
    fn put_body(&self) -> Value {
        serde_json::from_str(self.put).unwrap()
    }
    fn no_rev(&self) -> Value {
        let mut v = self.put_body();
        v.as_object_mut().unwrap().remove("rev");
        v
    }
    /// A key of the dead zone pair.
    fn dz(&self) -> &'static str {
        if self.ty == "classic" { "ls" } else { "ms" }
    }
}

struct T {
    app: axum::Router,
    dir: tempfile::TempDir,
}

impl T {
    fn dol(&self) -> PathBuf {
        self.dir.path().join("dolphin")
    }
    fn write(&self, name: &str, text: &str) {
        std::fs::create_dir_all(self.dol()).unwrap();
        std::fs::write(self.dol().join(name), text).unwrap();
    }
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.dol().join(name)).unwrap()
    }
    fn exists(&self, name: &str) -> bool {
        self.dol().join(name).exists()
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
    async fn get(&self, k: K) -> (StatusCode, Value) {
        self.call("GET", &k.url(), true, None).await
    }
    async fn put(&self, k: K, v: &Value) -> (StatusCode, Value) {
        self.call("PUT", &k.url(), true, Some(v.to_string())).await
    }
    async fn reset(&self, k: K) -> (StatusCode, Value) {
        self.call("POST", &format!("{}/reset", k.url()), true, None)
            .await
    }
}

/// A Dolphin process on the game file when `running`; `emulators` is the emulators.json text.
fn rig_with(emulators: Option<&str>, running: bool) -> T {
    let d = tempfile::tempdir().unwrap();
    let data = d.path().join("data");
    let proc = d.path().join("proc");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&proc).unwrap();
    if let Some(e) = emulators {
        std::fs::write(data.join("emulators.json"), e).unwrap();
    }
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
    T {
        app: router(Arc::new(AppState::new(cfg))),
        dir: d,
    }
}

fn rig(running: bool) -> T {
    rig_with(
        Some(&format!(
            r#"[{{"name":"Mario Kart Wii","emulator":"dolphin","file":"{ROMF}"}},{{"name":"Odd","emulator":"mystery","file":"/x"}}]"#
        )),
        running,
    )
}

/// A rig with the type's file (the repo one) already there.
fn rig_file(k: K) -> T {
    let t = rig(false);
    t.write(k.file, k.ini);
    t
}

#[tokio::test]
async fn auth_required() {
    let t = rig_file(CL);
    for k in BOTH {
        for (m, u) in [
            ("GET", k.url()),
            ("PUT", k.url()),
            ("POST", format!("{}/reset", k.url())),
        ] {
            let (s, _) = t.call(m, &u, false, Some(k.no_rev().to_string())).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{m} {}", k.ty);
        }
        assert_eq!(t.get(k).await.0, StatusCode::OK);
    }
}

#[tokio::test]
async fn not_found_cases() {
    let t = rig(false);
    // Unknown type, however spelled.
    for ty in [
        "wii",
        "Classic",
        "..",
        "classic%0a",
        "%2e%2e",
        "gamecube2",
        "x",
    ] {
        let u = format!("/api/emulators/dolphin/controls/{ty}");
        for (m, u) in [
            ("GET", u.clone()),
            ("PUT", u.clone()),
            ("POST", format!("{u}/reset")),
        ] {
            let (s, v) = t.call(m, &u, true, Some(CL.no_rev().to_string())).await;
            assert_eq!(s, StatusCode::NOT_FOUND, "{ty} {m}");
            assert_eq!(v["error"], "no such controller type", "{ty} {m}");
        }
    }
    // Unknown emulator.
    let (s, _) = t
        .call("GET", "/api/emulators/yuzu/controls/classic", true, None)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // No dolphin entry, or no emulators.json at all.
    let only_other = rig_with(
        Some(r#"[{"name":"Odd","emulator":"mystery","file":"/x"}]"#),
        false,
    );
    for t in [only_other, rig_with(None, false)] {
        for k in BOTH {
            for (m, u) in [
                ("GET", k.url()),
                ("PUT", k.url()),
                ("POST", format!("{}/reset", k.url())),
            ] {
                let (s, v) = t.call(m, &u, true, Some(k.no_rev().to_string())).await;
                assert_eq!(s, StatusCode::NOT_FOUND, "{m}");
                assert_eq!(v["error"], "no controls on this machine");
            }
        }
        assert!(!t.dol().exists());
    }
}

#[tokio::test]
async fn old_per_game_route_is_gone() {
    let t = rig_file(CL);
    let id = brisk_hostd::emu::load(&t.dir.path().join("data"))[0]
        .id
        .to_string();
    for u in [
        format!("/api/games/{id}/controls"),
        "/api/games/4026531840/controls".into(),
        format!("/api/games/{id}/controls/reset"),
    ] {
        for m in ["GET", "PUT", "POST"] {
            let (s, _) = t.call(m, &u, true, Some("{}".into())).await;
            assert_eq!(s, StatusCode::NOT_FOUND, "{m} {u}");
        }
    }
}

#[tokio::test]
async fn get_shapes_and_fixtures() {
    for k in BOTH {
        let t = rig(false);
        let (s, v) = t.get(k).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            (v["source"].as_str(), v["rev"].as_str()),
            (Some("default"), Some("none"))
        );
        assert_eq!(v["type"], k.ty);
        assert!(!t.exists(k.file), "a GET never writes");
        assert!(!t.dol().exists());
        let t = rig_file(k);
        let (_, v) = t.get(k).await;
        assert_eq!(v["source"], "file");
        assert_ne!(v["rev"], "none");
        assert_eq!(t.read(k.file), k.ini);
        // The fixture is the contract for everything but source and rev.
        let strip = |mut v: Value| {
            let o = v.as_object_mut().unwrap();
            o.remove("source");
            o.remove("rev");
            v
        };
        assert_eq!(
            strip(v),
            strip(serde_json::from_str(k.get).unwrap()),
            "{}",
            k.ty
        );
    }
    // Classic carries no portReady.
    assert!(rig(false).get(CL).await.1.get("portReady").is_none());
}

#[tokio::test]
async fn port_ready_follows_dolphin_ini() {
    let t = rig(false);
    assert_eq!(t.get(GC).await.1["portReady"], false);
    t.write(MAIN, "[Core]\nSIDevice0 = 0\n");
    assert_eq!(t.get(GC).await.1["portReady"], false);
    t.write(MAIN, "[Core]\nSIDevice0 = 6\n");
    assert_eq!(t.get(GC).await.1["portReady"], true);
    assert_eq!(t.read(MAIN), "[Core]\nSIDevice0 = 6\n");
}

#[tokio::test]
async fn put_writes_file_and_matches_next_get() {
    for k in BOTH {
        let t = rig_file(k);
        let (_, cur) = t.get(k).await;
        let mut b = k.put_body();
        b["rev"] = cur["rev"].clone();
        let (s, v) = t.put(k, &b).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v, t.get(k).await.1);
        assert_eq!(v["bindings"], b["bindings"]);
        assert_eq!(v["deadZone"], b["deadZone"]);
        assert_eq!(v["appliesOn"], "next-launch");
        let text = t.read(k.file);
        let lines: &[&str] = if k.ty == "classic" {
            &[
                "Classic/Buttons/A = SOUTH",
                "Buttons/A = SOUTH",
                "Buttons/1 = WEST",
                "Classic/Left Stick/Dead Zone = 10.000000",
            ]
        } else {
            &[
                "Buttons/A = EAST",
                "Buttons/B = SOUTH",
                "Buttons/Z = TL",
                "Buttons/Start = SELECT",
                "Main Stick/Dead Zone = 8.000000",
                "C-Stick/Dead Zone = 8.000000",
            ]
        };
        for l in lines {
            assert!(text.lines().any(|x| x == *l), "{} {l}", k.ty);
        }
        assert!(text.contains("Device = evdev/0/Sunshine (libvirtualhid) X-Box Series Controller"));
        assert_eq!(
            std::fs::metadata(t.dol().join(k.file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(t.tmps(), 0);
    }
}

#[tokio::test]
async fn gamecube_put_sets_the_port_and_backs_up_dolphin_ini_once() {
    let t = rig_file(GC);
    let orig = "[Core]\nSIDevice0 = 0\nSIDevice1 = 0\n\n[Interface]\nConfirmStop = False\n";
    t.write(MAIN, orig);
    let (s, v) = t.put(GC, &GC.no_rev()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["portReady"], true);
    assert_eq!(t.read(MAIN), orig.replace("SIDevice0 = 0", "SIDevice0 = 6"));
    assert_eq!(t.read(&format!("{MAIN}.before-brisk-edit")), orig);
    // Dolphin changes the port again; the second save fixes it without touching the backup.
    t.write(MAIN, "[Core]\nSIDevice0 = 0\n");
    assert_eq!(t.put(GC, &GC.no_rev()).await.0, StatusCode::OK);
    assert_eq!(t.read(MAIN), "[Core]\nSIDevice0 = 6\n");
    assert_eq!(t.read(&format!("{MAIN}.before-brisk-edit")), orig);
    // A save with the port already right writes nothing (no new backup name, same bytes).
    let before = std::fs::metadata(t.dol().join(MAIN))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(t.put(GC, &GC.no_rev()).await.0, StatusCode::OK);
    assert_eq!(
        std::fs::metadata(t.dol().join(MAIN))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    // Without a Dolphin.ini it is skipped and never created.
    let t = rig_file(GC);
    let (s, v) = t.put(GC, &GC.no_rev()).await;
    assert_eq!((s, &v["portReady"]), (StatusCode::OK, &json!(false)));
    assert!(!t.exists(MAIN) && !t.exists(&format!("{MAIN}.before-brisk-edit")));
    // A classic save never touches it.
    let t = rig_file(CL);
    t.write(MAIN, "[Core]\nSIDevice0 = 0\n");
    assert_eq!(t.put(CL, &CL.no_rev()).await.0, StatusCode::OK);
    assert_eq!(t.read(MAIN), "[Core]\nSIDevice0 = 0\n");
    assert!(!t.exists(&format!("{MAIN}.before-brisk-edit")));
}

#[tokio::test]
async fn the_two_types_are_independent() {
    let t = rig(false);
    t.write(CL.file, CL.ini);
    t.write(GC.file, GC.ini);
    t.write(MAIN, "[Core]\nSIDevice0 = 0\n");
    assert_eq!(t.put(CL, &CL.no_rev()).await.0, StatusCode::OK);
    assert_eq!(t.read(GC.file), GC.ini);
    assert_eq!(t.read(MAIN), "[Core]\nSIDevice0 = 0\n");
    let classic_after = t.read(CL.file);
    assert_eq!(t.put(GC, &GC.no_rev()).await.0, StatusCode::OK);
    assert_eq!(t.read(CL.file), classic_after);
    assert_ne!(t.read(GC.file), GC.ini);
}

#[tokio::test]
async fn put_creates_missing_file_without_backup() {
    for k in BOTH {
        let t = rig(false);
        let (s, v) = t.put(k, &k.no_rev()).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["source"], "file");
        assert!(t.exists(k.file) && !t.exists(&k.bk()));
    }
}

#[tokio::test]
async fn backup_made_once_per_file() {
    for k in BOTH {
        let orig = format!("{}# mine\n", k.ini);
        let t = rig(false);
        t.write(k.file, &orig);
        t.put(k, &k.no_rev()).await;
        assert_eq!(t.read(&k.bk()), orig);
        let mut b = k.no_rev();
        let (x, y) = (b["bindings"]["a"].clone(), b["bindings"]["b"].clone());
        b["bindings"]["a"] = y;
        b["bindings"]["b"] = x;
        assert_eq!(t.put(k, &b).await.0, StatusCode::OK);
        assert_eq!(t.read(&k.bk()), orig);
        assert!(t.read(k.file).contains("# mine"));
    }
}

#[tokio::test]
async fn validation_and_limits() {
    for k in BOTH {
        let t = rig_file(k);
        let mut cases: Vec<(&str, String)> = vec![];
        let mut add = |n: &'static str, f: &dyn Fn(&mut Value)| {
            let mut v = k.no_rev();
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
        add("other type's id", &|v| {
            v["bindings"].as_object_mut().unwrap().remove("a");
            v["bindings"][if k.ty == "classic" { "z" } else { "zl" }] = "A".into();
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
            v["bindings"][k.dz()] = "A".into();
        });
        add("stick on button", &|v| {
            v["bindings"]["a"] = "Left stick".into();
        });
        add("dpad on a", &|v| {
            v["bindings"]["a"] = "D-pad".into();
        });
        add("dup", &|v| {
            let b = v["bindings"]["b"].clone();
            v["bindings"]["a"] = b;
        });
        add("injection", &|v| {
            v["bindings"]["a"] = "A\nDevice = x".into();
        });
        add("injection2", &|v| {
            v["bindings"]["a"] = "A\n[Core]".into();
        });
        add("equals", &|v| {
            v["bindings"]["a"] = "A=B".into();
        });
        add("tick", &|v| {
            v["bindings"]["a"] = "`A`".into();
        });
        add("case", &|v| {
            v["bindings"]["a"] = "a".into();
        });
        add("lookalike", &|v| {
            v["bindings"]["a"] = "Ａ".into();
        });
        add("dz 51", &|v| {
            v["deadZone"][k.dz()] = 51.into();
        });
        add("dz -1", &|v| {
            v["deadZone"][k.dz()] = (-1).into();
        });
        add("dz 1.5", &|v| {
            v["deadZone"][k.dz()] = json!(1.5);
        });
        add("dz extra", &|v| {
            v["deadZone"]["x"] = 1.into();
        });
        add("dz other type", &|v| {
            v["deadZone"] = if k.ty == "classic" {
                json!({"ms": 0, "cs": 0})
            } else {
                json!({"ls": 0, "rs": 0})
            };
        });
        add("rev long", &|v| {
            v["rev"] = "x".repeat(33).into();
        });
        cases.push(("bad json", "{".into()));
        for (n, body) in cases {
            let (s, v) = t.call("PUT", &k.url(), true, Some(body)).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{} {n}", k.ty);
            assert!(v["error"].is_string(), "{n}");
            assert_eq!(t.read(k.file), k.ini, "{n}");
        }
        let big = json!({"x": "y".repeat(5000)}).to_string();
        for (m, u) in [("PUT", k.url()), ("POST", format!("{}/reset", k.url()))] {
            let (s, _) = t.call(m, &u, true, Some(big.clone())).await;
            assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE, "{m}");
        }
        assert_eq!(t.read(k.file), k.ini);
        assert!(!t.exists(&k.bk()));
    }
}

#[tokio::test]
async fn custom_controls_kept_until_picked() {
    for (k, line, other) in [
        (CL, "Classic/Buttons/A = EAST", "Buttons/A = EAST\n"),
        (GC, "Buttons/A = SOUTH", "Buttons/Z = TR\n"),
    ] {
        let odd = k.ini.replace(line, &format!("{line} | TL"));
        assert_ne!(odd, k.ini);
        let t = rig(false);
        t.write(k.file, &odd);
        let (_, v) = t.get(k).await;
        assert_eq!(
            (v["custom"]["a"].as_bool(), v["bindings"]["a"].is_null()),
            (Some(true), true)
        );
        // Leave "a" alone; give it nothing else. Its custom line must survive, others are rewritten.
        let mut b = v.clone();
        b.as_object_mut()
            .unwrap()
            .retain(|n, _| n == "bindings" || n == "deadZone");
        assert_eq!(t.put(k, &b).await.0, StatusCode::OK);
        let text = t.read(k.file);
        assert!(text.contains(&format!("{line} | TL\n")) && text.contains(other));
        // Picking a name replaces it.
        let free = if k.ty == "classic" { "B" } else { "A" };
        let (s, v) = {
            let mut b = k.no_rev();
            b["bindings"]["a"] = free.into();
            let holder = b["bindings"]
                .as_object()
                .unwrap()
                .iter()
                .find(|(n, p)| n.as_str() != "a" && **p == json!(free))
                .map(|(n, _)| n.clone());
            if let Some(h) = holder {
                b["bindings"][h] = if k.ty == "classic" {
                    "A".into()
                } else {
                    "B".into()
                };
            }
            t.put(k, &b).await
        };
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["bindings"]["a"], free);
        assert_eq!(v["custom"]["a"], false);
    }
}

#[tokio::test]
async fn stale_rev_conflicts() {
    for k in BOTH {
        let t = rig_file(k);
        let mut b = k.put_body();
        b["rev"] = "stale".into();
        let (s, v) = t.put(k, &b).await;
        assert_eq!(s, StatusCode::CONFLICT);
        assert_eq!(v["error"], "changed elsewhere");
        assert_eq!(v["current"], t.get(k).await.1);
        assert_eq!(t.read(k.file), k.ini);
        // A rev for a missing file is "none".
        let t = rig(false);
        let mut b = k.put_body();
        b["rev"] = "none".into();
        assert_eq!(t.put(k, &b).await.0, StatusCode::OK);
        assert_eq!(t.put(k, &b).await.0, StatusCode::CONFLICT);
    }
}

#[tokio::test]
async fn concurrent_puts_leave_a_valid_file() {
    for k in BOTH {
        let t = Arc::new(rig_file(k));
        let (a, b) = (k.put_body(), k.no_rev());
        let (t1, t2) = (t.clone(), t.clone());
        let (r1, r2) = tokio::join!(
            tokio::spawn(async move { t1.put(k, &a).await.0 }),
            tokio::spawn(async move { t2.put(k, &b).await.0 })
        );
        assert!([r1.unwrap(), r2.unwrap()].contains(&StatusCode::OK));
        let (s, v) = t.get(k).await;
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
}

#[tokio::test]
async fn running_flag_from_process_scan() {
    for k in BOTH {
        let t = rig(true);
        t.write(k.file, k.ini);
        assert_eq!(t.get(k).await.1["running"], true);
        let (s, v) = t.put(k, &k.no_rev()).await;
        assert_eq!(
            (s, &v["running"], &v["appliesOn"]),
            (StatusCode::OK, &json!(true), &json!("next-launch"))
        );
        assert_ne!(t.read(k.file), k.ini);
        let t = rig_file(k);
        assert_eq!(t.get(k).await.1["running"], false);
    }
}

#[tokio::test]
async fn io_errors_are_500_and_leave_no_temp() {
    for k in BOTH {
        let t = rig(false);
        std::fs::create_dir_all(t.dol().join(k.file)).unwrap(); // the target is a directory
        for (s, v) in [t.put(k, &k.no_rev()).await, t.reset(k).await] {
            assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(v["error"], "could not save controls");
        }
        assert!(t.dol().join(k.file).is_dir() && t.tmps() == 0);
        let t = rig(false);
        t.write(k.file, "");
        std::fs::write(t.dol().join(k.file), [0xff, 0xfe, b'\n']).unwrap();
        assert_eq!(
            t.put(k, &k.no_rev()).await.0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            std::fs::read(t.dol().join(k.file)).unwrap(),
            [0xff, 0xfe, b'\n']
        );
    }
    // The pad file is written, then the port write fails: 500, and the next GET says not ready.
    let t = rig_file(GC);
    std::fs::write(t.dol().join(MAIN), [0xff, b'\n']).unwrap();
    assert_eq!(
        t.put(GC, &GC.no_rev()).await.0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(t.get(GC).await.1["portReady"], false);
    assert_eq!(std::fs::read(t.dol().join(MAIN)).unwrap(), [0xff, b'\n']);
}

#[tokio::test]
async fn reset_restores_default() {
    for (k, line) in [(CL, "Classic/Buttons/A = EAST"), (GC, "Buttons/A = SOUTH")] {
        let odd = k.ini.replace(line, &format!("{line} | TL"));
        let t = rig(false);
        t.write(k.file, &odd);
        t.write(MAIN, "[Core]\nSIDevice0 = 0\n");
        let mut b = k.no_rev();
        b["bindings"]["a"] = Value::Null;
        b["deadZone"][k.dz()] = 9.into();
        t.put(k, &b).await;
        assert!(t.read(k.file).contains("Dead Zone"));
        let (s, v) = t.reset(k).await;
        assert_eq!(s, StatusCode::OK);
        let mut want: Value = serde_json::from_str(k.get).unwrap();
        want["rev"] = v["rev"].clone();
        want["source"] = "file".into();
        if k.ty == "gamecube" {
            want["portReady"] = true.into();
        }
        assert_eq!(v, want);
        let text = t.read(k.file);
        assert!(!text.contains("Dead Zone") && !text.contains("| TL"));
        assert_eq!(t.read(&k.bk()), odd);
        if k.ty == "gamecube" {
            assert_eq!(t.read(MAIN), "[Core]\nSIDevice0 = 6\n");
        } else {
            assert_eq!(t.read(MAIN), "[Core]\nSIDevice0 = 0\n");
        }
    }
}
