// Each test binary uses a different subset.
#![allow(dead_code)]
use axum::body::Body;
use axum::http::{Request, StatusCode};
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use std::path::Path;
use std::sync::Arc;
use tower::ServiceExt;

pub const SECRET: &str = "s3cret";
pub const H: &str = "localhost:8443";
pub const KEY: &str = "TESTKEY-0123456789abcdef";
pub const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/steam");

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

/// Mock Steam: the first route whose path substring is in the request line answers with its
/// status and body. Routes under `/I…/` (the Web API) need `key=KEY` in the query, else 403.
pub async fn mock_steam(routes: Vec<(&'static str, u16, String)>) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let (mut c, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = c.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = head.lines().next().unwrap_or("");
            let (code, body) = match routes.iter().find(|(p, _, _)| line.contains(p)) {
                Some(_) if line.starts_with("GET /I") && !line.contains(&format!("key={KEY}")) => {
                    (403, String::new())
                }
                Some((_, code, body)) => (*code, body.clone()),
                None => (404, String::new()),
            };
            let r = format!(
                "HTTP/1.1 {code} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = c.write_all(r.as_bytes()).await;
        }
    });
    base
}

pub struct Steam {
    pub _t: tempfile::TempDir,
    pub cfg: Config,
}

pub fn steam(base: &str, store_base: &str, key: Option<&str>) -> Steam {
    let t = tempfile::tempdir().unwrap();
    let (s, l2) = (t.path().join("steam"), t.path().join("lib2"));
    copy_dir(
        Path::new(FIX).join("lib1/steamapps").as_path(),
        &s.join("steamapps"),
    );
    copy_dir(
        Path::new(FIX).join("lib2/steamapps").as_path(),
        &l2.join("steamapps"),
    );
    let vdf = std::fs::read_to_string(format!("{FIX}/libraryfolders.vdf.tmpl"))
        .unwrap()
        .replace("@LIB1@", s.to_str().unwrap())
        .replace("@LIB2@", l2.to_str().unwrap());
    std::fs::write(s.join("steamapps/libraryfolders.vdf"), vdf).unwrap();
    std::fs::create_dir_all(s.join("config")).unwrap();
    std::fs::write(
        s.join("config/loginusers.vdf"),
        "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"MostRecent\"\t\"1\"\n\t}\n}\n",
    )
    .unwrap();
    let mut cfg = Config::from_env(t.path().join("dist"));
    cfg.secret = SECRET.into();
    cfg.steam_dir = s;
    cfg.data_dir = t.path().join("data");
    cfg.steam_api_base = base.into();
    cfg.steam_store_base = store_base.into();
    cfg.steam_key_file = t.path().join("key");
    if let Some(k) = key {
        std::fs::write(&cfg.steam_key_file, format!("{k}\n")).unwrap();
    }
    Steam { _t: t, cfg }
}

pub async fn get_json(cfg: &Config, uri: &str) -> (StatusCode, serde_json::Value) {
    let app = router(Arc::new(AppState::new(cfg.clone())));
    let r = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("host", H)
                .header("x-brisk-secret", cfg.secret.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let st = r.status();
    let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    (st, serde_json::from_slice(&b).unwrap_or_default())
}

pub async fn games_json(cfg: &Config, uri: &str) -> (serde_json::Value, String) {
    let app = router(Arc::new(AppState::new(cfg.clone())));
    let r = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("host", H)
                .header("x-brisk-secret", cfg.secret.as_str())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    let b = String::from_utf8(b.to_vec()).unwrap();
    (serde_json::from_str(&b).unwrap(), b)
}
