//! Clipboard sync and file drop: /api/clipboard and /api/files/<name>.
use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use brisk_hostd::clipboard::{Backend, Clip, Clipboard, Kind, MAX};
use brisk_hostd::config::Config;
use brisk_hostd::files::{candidate, sanitize};
use brisk_hostd::http::{router, AppState};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tower::ServiceExt;

const SECRET: &str = "s3cret";
const H: &str = "localhost:8443";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfakepng";

/// A clipboard in memory. Like the compositor, every change (ours or "external") wakes the watcher.
struct Fake {
    now: Mutex<Option<Clip>>,
    writes: Mutex<Vec<Clip>>,
    tx: Mutex<mpsc::Sender<()>>,
    rx: Mutex<mpsc::Receiver<()>>,
    waits: AtomicUsize,
}

impl Fake {
    fn new() -> Arc<Fake> {
        let (tx, rx) = mpsc::channel();
        Arc::new(Fake {
            now: Mutex::new(None),
            writes: Mutex::new(vec![]),
            tx: Mutex::new(tx),
            rx: Mutex::new(rx),
            waits: AtomicUsize::new(0),
        })
    }
    fn external(&self, c: Clip) {
        *self.now.lock().unwrap() = Some(c);
        let _ = self.tx.lock().unwrap().send(());
    }
}

impl Backend for Fake {
    fn read(&self) -> Option<Clip> {
        self.now.lock().unwrap().clone()
    }
    fn write(&self, c: &Clip) -> bool {
        self.writes.lock().unwrap().push(c.clone());
        self.external(c.clone());
        true
    }
    fn wait_change(&self) {
        self.waits.fetch_add(1, Ordering::SeqCst);
        if self.rx.lock().unwrap().recv().is_err() {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
}

struct T {
    app: axum::Router,
    fake: Arc<Fake>,
    dl: PathBuf,
    _d: tempfile::TempDir,
}

fn t_with(f: impl FnOnce(&mut AppState)) -> T {
    let d = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env(d.path().to_path_buf());
    cfg.secret = SECRET.into();
    cfg.downloads_dir = d.path().join("Downloads/brisk");
    let dl = cfg.downloads_dir.clone();
    let fake = Fake::new();
    let mut st = AppState::new(cfg);
    st.clip = Arc::new(Clipboard::new(fake.clone()));
    f(&mut st);
    let state = Arc::new(st);
    state.clip.start_watch();
    T {
        app: router(state),
        fake,
        dl,
        _d: d,
    }
}

fn t() -> T {
    t_with(|_| {})
}

fn req(method: &str, uri: &str, ct: Option<&str>, body: Body) -> Request<Body> {
    let mut r = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", H)
        .header("x-brisk-secret", SECRET);
    if let Some(c) = ct {
        r = r.header("content-type", c);
    }
    r.body(body).unwrap()
}

impl T {
    async fn send(&self, r: Request<Body>) -> Response {
        self.app.clone().oneshot(r).await.unwrap()
    }
    async fn put(&self, name: &str, b: impl Into<Body>) -> (StatusCode, serde_json::Value) {
        let r = self
            .send(req("PUT", &format!("/api/files/{name}"), None, b.into()))
            .await;
        let st = r.status();
        let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
        (st, serde_json::from_slice(&b).unwrap_or_default())
    }
    async fn clip(&self, since: Option<u64>) -> (StatusCode, u64, Bytes, String) {
        let q = since.map_or(String::new(), |s| format!("?since={s}"));
        let r = self
            .send(req(
                "GET",
                &format!("/api/clipboard{q}"),
                None,
                Body::empty(),
            ))
            .await;
        let seq = r.headers()["x-brisk-seq"]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        let ct = r
            .headers()
            .get("content-type")
            .map_or(String::new(), |v| v.to_str().unwrap().to_string());
        let st = r.status();
        (
            st,
            seq,
            axum::body::to_bytes(r.into_body(), 64 << 20).await.unwrap(),
            ct,
        )
    }
    fn files(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.dl)
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
    /// Waits until the watcher has handled every change so far and is waiting again.
    async fn settle(&self, waits_before: usize) {
        for _ in 0..100 {
            if self.fake.waits.load(Ordering::SeqCst) > waits_before {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("watcher never woke");
    }
}

fn text(s: &str) -> Clip {
    Clip {
        kind: Kind::Text,
        data: Bytes::from(s.to_string()),
    }
}

// ---- file names ----

#[test]
fn sanitize_table() {
    let long = format!("{}.png", "a".repeat(300));
    let cases: &[(&str, Option<&str>)] = &[
        ("a.txt", Some("a.txt")),
        ("../../etc/passwd", Some("passwd")),
        ("/etc/shadow", Some("shadow")),
        ("dir\\win.exe", Some("win.exe")),
        ("..", None),
        (".", None),
        ("...", None),
        ("", None),
        ("   ", None),
        ("x/", None),
        ("a/..", None),
        (".bashrc", Some("bashrc")),
        ("..hidden.txt", Some("hidden.txt")),
        ("a\0b\nc\u{7f}.txt", Some("abc.txt")),
        ("\0", None),
        ("café ☕.png", Some("café ☕.png")),
        ("foo..bar", Some("foo..bar")),
    ];
    for (raw, want) in cases {
        assert_eq!(sanitize(raw).as_deref(), *want, "{raw:?}");
    }
    let s = sanitize(&long).unwrap();
    assert!(s.len() <= 200 && s.ends_with(".png"), "{s}");
    let u = sanitize(&"é".repeat(150)).unwrap(); // 300 bytes, cut on a char boundary
    assert!(u.len() <= 200 && u.chars().all(|c| c == 'é'));
}

#[test]
fn dedupe_names() {
    assert_eq!(candidate("a.txt", 0), "a.txt");
    assert_eq!(candidate("a.txt", 1), "a (1).txt");
    assert_eq!(candidate("noext", 2), "noext (2)");
    assert_eq!(candidate("archive.tar.gz", 1), "archive.tar (1).gz");
}

// ---- uploads ----

#[tokio::test]
async fn upload_saves_and_dedupes() {
    let t = t();
    let (st, v) = t.put("hello.txt", "hi").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["size"], 2);
    let p = PathBuf::from(v["path"].as_str().unwrap());
    assert_eq!(p, t.dl.join("hello.txt"));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "hi");
    let (_, v2) = t.put("hello.txt", "again").await;
    assert_eq!(
        v2["path"].as_str().unwrap(),
        t.dl.join("hello (1).txt").to_str().unwrap()
    );
    // Traversal is cut to the basename; a name that is only dots is refused.
    let (_, v3) = t.put("..%2F..%2Fevil.sh", "x").await;
    assert_eq!(
        v3["path"].as_str().unwrap(),
        t.dl.join("evil.sh").to_str().unwrap()
    );
    assert_eq!(t.put("..", "x").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(t.files(), ["evil.sh", "hello (1).txt", "hello.txt"]);
}

#[tokio::test]
async fn upload_bigger_than_default_body_limit_streams() {
    let t = t();
    let big = vec![7u8; 5 << 20]; // axum's default limit is 2 MiB
    let (st, v) = t.put("big.bin", big.clone()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["size"], 5 << 20);
    assert_eq!(std::fs::read(t.dl.join("big.bin")).unwrap(), big);
}

#[tokio::test]
async fn upload_over_8_gib_is_413_and_no_space_is_507() {
    let t = t_with(|s| s.files.free = |_| Some(1000));
    let mut r = req("PUT", "/api/files/huge.iso", None, Body::from("x"));
    r.headers_mut()
        .insert("content-length", (9u64 << 30).to_string().parse().unwrap());
    assert_eq!(t.send(r).await.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let mut r = req("PUT", "/api/files/a.bin", None, Body::from(vec![0u8; 2000]));
    r.headers_mut()
        .insert("content-length", "2000".parse().unwrap());
    assert_eq!(t.send(r).await.status(), StatusCode::INSUFFICIENT_STORAGE);
    assert!(t.files().is_empty(), "{:?}", t.files());
}

#[tokio::test]
async fn aborted_upload_leaves_no_temp_file() {
    let t = t();
    let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
        Ok(Bytes::from(vec![1u8; 100_000])),
        Err(std::io::Error::other("client went away")),
    ];
    let (st, _) = t
        .put(
            "cut.bin",
            Body::from_stream(futures_util::stream::iter(chunks)),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    assert!(t.files().is_empty(), "{:?}", t.files());
}

#[tokio::test]
async fn same_name_twice_at_once_is_409() {
    let t = t();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    let stream =
        futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|c| (c, rx)) });
    let app = t.app.clone();
    let first = tokio::spawn(app.oneshot(req(
        "PUT",
        "/api/files/x.bin",
        None,
        Body::from_stream(stream),
    )));
    tx.send(Ok(Bytes::from_static(b"part"))).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(t.put("x.bin", "other").await.0, StatusCode::CONFLICT);
    assert_eq!(t.put("y.bin", "other").await.0, StatusCode::OK);
    drop(tx);
    assert_eq!(first.await.unwrap().unwrap().status(), StatusCode::OK);
    assert_eq!(std::fs::read(t.dl.join("x.bin")).unwrap(), b"part");
}

#[tokio::test]
async fn clipboard_and_files_need_the_secret() {
    let t = t();
    for (m, u) in [
        ("PUT", "/api/files/a.txt"),
        ("GET", "/api/clipboard"),
        ("POST", "/api/clipboard"),
    ] {
        let r = Request::builder()
            .method(m)
            .uri(u)
            .header("host", H)
            .header("content-type", "text/plain")
            .body(Body::from("x"))
            .unwrap();
        assert_eq!(t.send(r).await.status(), StatusCode::FORBIDDEN, "{m} {u}");
    }
    assert!(t.files().is_empty());
    assert!(t.fake.writes.lock().unwrap().is_empty());
}

// ---- clipboard ----

#[tokio::test]
async fn bawn_changes_bump_seq_and_are_served() {
    let t = t();
    t.settle(0).await;
    let (st, s0, _, _) = t.clip(None).await;
    assert_eq!(st, StatusCode::NO_CONTENT); // no `since`: just the current seq
    assert_eq!(t.clip(Some(s0)).await.0, StatusCode::NO_CONTENT);

    let w = t.fake.waits.load(Ordering::SeqCst);
    t.fake.external(text("hello"));
    t.settle(w).await;
    let (st, s1, b, ct) = t.clip(Some(s0)).await;
    assert_eq!(st, StatusCode::OK);
    assert!(s1 > s0 && ct.starts_with("application/json"));
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(
        v,
        serde_json::json!({ "seq": s1, "kind": "text", "text": "hello" })
    );
    assert_eq!(t.clip(Some(s1)).await.0, StatusCode::NO_CONTENT);

    let w = t.fake.waits.load(Ordering::SeqCst);
    t.fake.external(Clip {
        kind: Kind::Image,
        data: Bytes::from_static(PNG),
    });
    t.settle(w).await;
    let (st, s2, b, ct) = t.clip(Some(s1)).await;
    assert_eq!(
        (st, ct.as_str(), &b[..]),
        (StatusCode::OK, "image/png", PNG)
    );
    assert!(s2 > s1);

    // The same content again is no change.
    let w = t.fake.waits.load(Ordering::SeqCst);
    t.fake.external(Clip {
        kind: Kind::Image,
        data: Bytes::from_static(PNG),
    });
    t.settle(w).await;
    assert_eq!(t.clip(Some(s2)).await.0, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn post_sets_clipboard_without_echo() {
    let t = t();
    t.settle(0).await;
    let (_, s0, _, _) = t.clip(None).await;
    let w = t.fake.waits.load(Ordering::SeqCst);
    let r = t
        .send(req(
            "POST",
            "/api/clipboard",
            Some("text/plain;charset=utf-8"),
            Body::from("from laptop"),
        ))
        .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(*t.fake.writes.lock().unwrap(), [text("from laptop")]);
    t.settle(w).await; // the watcher saw our own write
    let (st, s1, _, _) = t.clip(Some(s0)).await;
    assert_eq!(
        (st, s1),
        (StatusCode::NO_CONTENT, s0),
        "own write echoed back"
    );

    let r = t
        .send(req(
            "POST",
            "/api/clipboard",
            Some("image/png"),
            Body::from(PNG),
        ))
        .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(t.fake.writes.lock().unwrap()[1].kind, Kind::Image);
    // Over axum's 2 MiB default, under the 16 MiB cap.
    let big = vec![b'a'; 3 << 20];
    let r = t
        .send(req(
            "POST",
            "/api/clipboard",
            Some("text/plain"),
            Body::from(big),
        ))
        .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn post_refuses_other_types_bad_png_and_oversize() {
    let t = t();
    let post =
        |ct: &'static str, b: Vec<u8>| req("POST", "/api/clipboard", Some(ct), Body::from(b));
    assert_eq!(
        t.send(post("text/html", b"<b>".to_vec())).await.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        t.send(post("image/png", b"not a png".to_vec()))
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.send(post("text/plain", vec![0xff, 0xfe])).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        t.send(post("text/plain", vec![b'a'; MAX + 1]))
            .await
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(t.fake.writes.lock().unwrap().is_empty());
}
