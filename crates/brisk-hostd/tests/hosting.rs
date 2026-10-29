//! Running as a service: persistent secret, Host allowlist, tailnet identity, and the bridge
//! address and certificate hostd sets up itself. Everything runs against fakes and temp dirs.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use brisk_hostd::auth::Hosts;
use brisk_hostd::config::{Config, list, load_secret, load_users, load_users_as};
use brisk_hostd::host::{Clock, Runner};
use brisk_hostd::http::{AppState, router};
use brisk_hostd::stream::{ensure_cert, resolve_listen};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tower::ServiceExt;

const SECRET: &str = "0123456789abcdef0123";
const STATUS: &str = r#"{"Self":{"DNSName":"bawn.tail0.ts.net.","TailscaleIPs":["100.64.0.9"]}}"#;

/// Answers `tailscale status --json` / `tailscale ip -4` from `ts` (None = tailscale down);
/// `openssl` writes PEM files to its -keyout/-out unless `openssl_ok` is false.
struct Fake {
    calls: Mutex<Vec<String>>,
    ts: Mutex<Option<String>>,
    ip: Option<String>,
    openssl_ok: bool,
}
impl Fake {
    fn new(ts: Option<&str>) -> Arc<Fake> {
        Arc::new(Fake {
            calls: Default::default(),
            ts: Mutex::new(ts.map(Into::into)),
            ip: Some("100.64.0.9\nfd7a::9\n".into()),
            openssl_ok: true,
        })
    }
    fn calls(&self, prog: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.starts_with(prog))
            .count()
    }
}
impl Runner for Fake {
    fn run(&self, p: &str, a: &[&str]) -> Option<String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{p} {}", a.join(" ")));
        match (p, a) {
            ("tailscale", ["status", "--json"]) => self.ts.lock().unwrap().clone(),
            ("tailscale", ["ip", "-4"]) => self.ip.clone(),
            ("openssl", _) if self.openssl_ok => {
                let at = |f: &str| a[a.iter().position(|x| *x == f).unwrap() + 1];
                std::fs::write(at("-keyout"), "-----BEGIN PRIVATE KEY-----\n").unwrap();
                std::fs::write(at("-out"), "-----BEGIN CERTIFICATE-----\n").unwrap();
                Some(String::new())
            }
            _ => None,
        }
    }
}
struct Clock0;
impl Clock for Clock0 {
    fn now_s(&self) -> u64 {
        0
    }
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

// ---- 1. persistent secret ----

#[test]
fn secret_is_created_once_0600_and_reused() {
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("conf/brisk/secret");
    let a = load_secret(None, &f).unwrap();
    assert_eq!(a.len(), 32);
    assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(mode(&f), 0o600);
    assert_eq!(mode(f.parent().unwrap()), 0o700);
    assert_eq!(
        load_secret(None, &f).unwrap(),
        a,
        "same secret after a restart"
    );
    // the env override wins and does not touch the file
    assert_eq!(load_secret(Some(SECRET.into()), &f).unwrap(), SECRET);
    assert_eq!(std::fs::read_to_string(&f).unwrap().trim(), a);
}

#[test]
fn bad_secret_file_is_an_error_and_loose_mode_is_tightened() {
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("secret");
    std::fs::write(&f, "short\n").unwrap();
    assert!(load_secret(None, &f).is_err());
    std::fs::write(&f, format!("{SECRET}\n")).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(load_secret(None, &f).unwrap(), SECRET);
    assert_eq!(mode(&f), 0o600);
    assert!(
        load_secret(Some("x".into()), &f).is_err(),
        "a too short override"
    );
}

#[test]
fn users_file_comments_and_permissions() {
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("allowed-users");
    assert!(load_users(&f).is_empty(), "missing file = feature off");
    std::fs::write(&f, "# who\ntoby@example.com  # me\n\n  other@x.y\n").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(load_users(&f), ["toby@example.com", "other@x.y"]);
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(load_users(&f).is_empty(), "group/world writable is ignored");
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = t.path().join("link");
    std::os::unix::fs::symlink(&f, &link).unwrap();
    assert!(load_users(&link).is_empty(), "a symlink is ignored");
    // owned by someone else: the same file checked against another uid than its owner's
    use std::os::unix::fs::MetadataExt;
    let owner = std::fs::metadata(&f).unwrap().uid();
    assert_eq!(load_users_as(&f, owner), ["toby@example.com", "other@x.y"]);
    assert!(load_users_as(&f, owner + 1).is_empty(), "not our uid");
    // a FIFO is not a regular file and must not block the open
    let fifo = t.path().join("fifo");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: c is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    assert!(load_users(&fifo).is_empty(), "a FIFO is ignored");
    assert_eq!(
        list("a.ts.net:8443, a.ts.net,,"),
        ["a.ts.net:8443", "a.ts.net"]
    );
}

#[test]
fn list_handles_crlf_comments_and_mixed_separators() {
    assert_eq!(list("a\r\nb\r\n"), ["a", "b"]);
    assert_eq!(list("a # one, b\nc,d # e\n# only\n"), ["a", "c", "d"]);
    assert_eq!(list("a,b\nc,\n,d"), ["a", "b", "c", "d"]);
}

#[test]
fn secret_of_15_chars_or_non_ascii_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("secret");
    assert!(load_secret(Some("a".repeat(15)), &f).is_err());
    assert_eq!(
        load_secret(Some("a".repeat(16)), &f).unwrap(),
        "a".repeat(16)
    );
    assert!(load_secret(Some("é".repeat(16)), &f).is_err());
    std::fs::write(&f, format!("{}\n", "é".repeat(16))).unwrap();
    assert!(load_secret(None, &f).is_err());
}

#[test]
fn downloads_dir_env_overrides_default() {
    let c = Config::from_vars(".".into(), |n| {
        (n == "BRISK_DOWNLOADS_DIR").then(|| "/x/dl".into())
    });
    assert_eq!(c.downloads_dir, Path::new("/x/dl"));
}

// ---- 2 + 3. guard: Host allowlist and tailnet identity ----

struct Req<'a> {
    host: &'a str,
    secret: Option<&'a str>,
    login: Option<&'a str>,
    csrf: bool,
    peer: Option<&'a str>,
}
const R: Req = Req {
    host: "localhost:8443",
    secret: None,
    login: None,
    csrf: false,
    peer: Some("127.0.0.1:5000"),
};

fn app(
    fake: Arc<Fake>,
    hosts: &[&str],
    users: &[&str],
) -> (axum::Router, Arc<AppState>, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("index.html"), "home").unwrap();
    let mut cfg = Config::from_env(d.path().to_path_buf());
    cfg.secret = SECRET.into();
    cfg.allowed_hosts = hosts.iter().map(|s| s.to_string()).collect();
    cfg.allowed_users = users.iter().map(|s| s.to_string()).collect();
    cfg.tls_dir = d.path().join("tls");
    let st = Arc::new(AppState::with_host(cfg, fake, Arc::new(Clock0)));
    (router(st.clone()), st, d)
}

async fn status(app: &axum::Router, uri: &str, r: Req<'_>) -> StatusCode {
    let mut b = Request::builder().uri(uri).header("host", r.host);
    if let Some(s) = r.secret {
        b = b.header("x-brisk-secret", s);
    }
    if let Some(l) = r.login {
        b = b
            .header("tailscale-user-login", l)
            .header("tailscale-user-name", "Toby");
    }
    if r.csrf {
        b = b.header("x-brisk-csrf", "1");
    }
    let mut req = b.body(Body::empty()).unwrap();
    if let Some(p) = r.peer {
        req.extensions_mut()
            .insert(ConnectInfo(p.parse::<SocketAddr>().unwrap()));
    }
    app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn extra_and_tailnet_hosts_are_allowed_foreign_ones_are_not() {
    let fake = Fake::new(Some(STATUS));
    let (app, st, _d) = app(fake.clone(), &["brisk.example:8443"], &[]);
    st.hosts.refresh().await; // what main does at startup
    for h in [
        "localhost:8443",
        "127.0.0.1:8443",
        "brisk.example:8443",
        "BAWN.tail0.ts.net:8443",
    ] {
        assert_eq!(
            status(&app, "/", Req { host: h, ..R }).await,
            StatusCode::OK,
            "{h}"
        );
    }
    let n = fake.calls("tailscale status");
    for h in [
        "evil.test",
        "localhost",
        "localhost:9999",
        "bawn.tail0.ts.net:9999",
        "brisk.example",
        "evil.ts.net:8443",
        "bawn.tail0.ts.net.evil.test:8443",
        "other.tail0.ts.net",
        "bawn.tail0.ts.net",
        "bawn.tail0.ts.net.:8443",
        "localhost.:8443",
        "[::1]:8443",
    ] {
        assert_eq!(
            status(
                &app,
                "/",
                Req {
                    host: h,
                    secret: Some(SECRET),
                    ..R
                }
            )
            .await,
            StatusCode::FORBIDDEN,
            "{h}"
        );
    }
    assert_eq!(
        fake.calls("tailscale status"),
        n,
        "within the cache, unknown ts.net names make no new lookup"
    );
}

#[tokio::test(start_paused = true)]
async fn tailnet_name_is_learned_lazily_when_tailscale_comes_up_later() {
    let fake = Fake::new(None); // tailscale down at startup
    let hosts = Hosts::new(&[], fake.clone());
    hosts.refresh().await;
    let h = "bawn.tail0.ts.net:8443";
    assert!(!hosts.allowed(h).await);
    *fake.ts.lock().unwrap() = Some(STATUS.into());
    assert!(!hosts.allowed(h).await, "within the cache: no new lookup");
    assert!(
        !hosts.allowed("evil.example").await,
        "never a lookup for a non-ts.net name"
    );
    assert_eq!(fake.calls("tailscale status"), 1);
    tokio::time::advance(hosts.recheck).await;
    assert!(hosts.allowed(h).await, "cache expired: learned the name");
    assert!(
        !hosts.allowed("evil.ts.net").await,
        "a lookup does not let other ts.net names in"
    );
    assert_eq!(fake.calls("tailscale status"), 2);
}

#[tokio::test]
async fn tailnet_identity_lets_allowed_logins_in_from_loopback_only() {
    let fake = Fake::new(Some(STATUS));
    let (app, _st, _d) = app(fake.clone(), &[], &["toby@example.com"]);
    let ok = Req {
        login: Some("Toby@Example.com"),
        csrf: true,
        ..R
    };
    assert_eq!(
        status(&app, "/api/nope", Req { ..ok }).await,
        StatusCode::NOT_FOUND,
        "authenticated"
    );
    for (why, r) in [
        (
            "not loopback",
            Req {
                peer: Some("100.64.0.2:5000"),
                ..ok
            },
        ),
        ("no peer info", Req { peer: None, ..ok }),
        (
            "unknown login",
            Req {
                login: Some("eve@example.com"),
                ..ok
            },
        ),
        (
            "empty login",
            Req {
                login: Some(" "),
                ..ok
            },
        ),
        ("no csrf header", Req { csrf: false, ..ok }),
        ("no login", Req { login: None, ..ok }),
    ] {
        assert_eq!(
            status(&app, "/api/nope", r).await,
            StatusCode::FORBIDDEN,
            "{why}"
        );
    }
    // the secret still works on its own, and the Host check still applies to identity
    assert_eq!(
        status(
            &app,
            "/api/nope",
            Req {
                secret: Some(SECRET),
                ..R
            }
        )
        .await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status(
            &app,
            "/api/nope",
            Req {
                host: "evil.test",
                ..ok
            }
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn empty_allowlist_means_secret_required() {
    let (app, _st, _d) = app(Fake::new(Some(STATUS)), &[], &[]);
    let r = Req {
        login: Some("toby@example.com"),
        csrf: true,
        ..R
    };
    assert_eq!(status(&app, "/api/nope", r).await, StatusCode::FORBIDDEN);
}

/// What a cross-site page could send with the laptop's identity attached by tailscale serve.
#[tokio::test]
async fn cross_site_requests_with_a_login_are_refused_and_get_no_cors() {
    let (app, _st, _d) = app(Fake::new(Some(STATUS)), &[], &["toby@example.com"]);
    let send = |m: &str, extra: &[(&str, &str)]| {
        let mut b = Request::builder()
            .method(m)
            .uri("/api/stream")
            .header("host", "localhost:8443")
            .header("origin", "https://evil.example")
            .header("tailscale-user-login", "toby@example.com");
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        let mut req = b.body(Body::from(r#"{"preset":"balanced"}"#)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()));
        app.clone().oneshot(req)
    };
    // preflight: refused, no access-control-* headers, so the browser never sends x-brisk-csrf
    let r = send(
        "OPTIONS",
        &[
            ("access-control-request-method", "POST"),
            ("access-control-request-headers", "x-brisk-csrf"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    assert!(
        r.headers()
            .keys()
            .all(|k| !k.as_str().starts_with("access-control-")),
        "{:?}",
        r.headers()
    );
    // a "simple" text/plain POST (no preflight) carries the login but cannot carry the csrf header
    let r = send("POST", &[("content-type", "text/plain")])
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

/// `update` swaps web/dist as a symlink between build dirs: hostd follows it per request but never
/// serves anything outside the current build.
#[tokio::test]
async fn dist_symlink_is_followed_and_escapes_are_refused() {
    let t = tempfile::tempdir().unwrap();
    let (a, b) = (t.path().join("dist.1"), t.path().join("dist.2"));
    for (d, s) in [(&a, "one"), (&b, "two")] {
        std::fs::create_dir_all(d.join("_astro")).unwrap();
        std::fs::write(d.join("index.html"), s).unwrap();
        std::fs::write(d.join("_astro/x.js"), s).unwrap();
    }
    std::fs::write(t.path().join("outside.js"), "secret").unwrap();
    std::os::unix::fs::symlink(t.path().join("outside.js"), a.join("_astro/leak.js")).unwrap();
    std::os::unix::fs::symlink(t.path(), a.join("up")).unwrap();
    let dist = t.path().join("dist");
    std::os::unix::fs::symlink("dist.1", &dist).unwrap();
    let mut cfg = Config::from_env(dist.clone());
    cfg.secret = SECRET.into();
    let app = router(Arc::new(AppState::with_host(
        cfg,
        Fake::new(None),
        Arc::new(Clock0),
    )));
    let get = |p: &str| {
        let req = Request::builder()
            .uri(p)
            .header("host", "localhost:8443")
            .body(Body::empty())
            .unwrap();
        let app = app.clone();
        async move {
            let r = app.oneshot(req).await.unwrap();
            let st = r.status();
            let b = axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap();
            (st, String::from_utf8_lossy(&b).to_string())
        }
    };
    assert_eq!(get("/_astro/x.js").await, (StatusCode::OK, "one".into()));
    for p in ["/_astro/leak.js", "/up/outside.js", "/_astro/gone.js"] {
        assert_eq!(get(p).await.0, StatusCode::NOT_FOUND, "{p}");
    }
    // the swap: a new symlink renamed over the old one
    std::os::unix::fs::symlink("dist.2", t.path().join("dist.tmp")).unwrap();
    std::fs::rename(t.path().join("dist.tmp"), &dist).unwrap();
    assert_eq!(get("/").await, (StatusCode::OK, "two".into()));
    assert_eq!(get("/_astro/x.js").await, (StatusCode::OK, "two".into()));
}

#[test]
fn bridge_lines_with_the_session_token_are_redacted_in_service_mode_only() {
    use brisk_hostd::stream::bridge_line;
    let page = "page\thttp://localhost:8443/?wt=https://100.64.0.9:4433/0123456789abcdef&hash=ab12"
        .to_string();
    let tok = Some("0123456789abcdef");
    assert_eq!(
        bridge_line(page.clone(), false, tok),
        page,
        "manual runs keep it"
    );
    assert_eq!(bridge_line(page, true, tok), "page <redacted>");
    for l in [
        "listening https://100.64.0.9:4433/0123456789abcdef",
        "x ?wt=https://a/b",
    ] {
        assert_eq!(bridge_line(l.into(), true, tok), "page <redacted>", "{l}");
    }
    for l in ["tap\tconnected", "hash=ab12", "bridge_p50_us 812"] {
        assert_eq!(bridge_line(l.into(), true, tok), l, "{l}");
    }
}

#[test]
fn service_page_line_has_no_secret() {
    use brisk_hostd::config::page_line;
    assert_eq!(
        page_line(SECRET, false),
        format!("page\thttp://localhost:8443/?s={SECRET}")
    );
    let l = page_line(SECRET, true);
    assert!(!l.contains("?s=") && !l.contains(SECRET), "{l}");
    assert!(l.starts_with("page\thttp://localhost:8443/") && l.contains("brisk-service.sh link"));
}

// ---- 4. bridge address and certificate ----

#[test]
fn listen_is_the_tailscale_ip_or_brisk_listen_never_all_interfaces() {
    let fake = Fake::new(None);
    assert_eq!(resolve_listen(&*fake, None).unwrap(), "100.64.0.9:4433");
    assert_eq!(
        resolve_listen(&*fake, Some("100.64.0.1:5000")).unwrap(),
        "100.64.0.1:5000"
    );
    assert!(
        resolve_listen(&*fake, Some("0.0.0.0:4433"))
            .unwrap_err()
            .contains("all interfaces")
    );
    assert!(
        resolve_listen(&*fake, Some("bawn:4433"))
            .unwrap_err()
            .contains("IPv4:PORT")
    );
    let down = Arc::new(Fake {
        ip: None,
        ..Arc::into_inner(Fake::new(None)).unwrap()
    });
    assert!(
        resolve_listen(&*down, None)
            .unwrap_err()
            .contains("tailscale")
    );
}

fn age(p: &Path, days: f64) {
    let d = Duration::from_secs_f64(days.abs() * 86400.0);
    let t = if days < 0.0 {
        SystemTime::now() + d
    } else {
        SystemTime::now() - d
    };
    std::fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn cert_is_created_0600_kept_while_fresh_and_renewed_after_10_days() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("tls");
    let fake = Fake::new(None);
    let (c, k) = ensure_cert(&*fake, &dir, SystemTime::now()).unwrap();
    assert_eq!(
        (c.file_name().unwrap(), k.file_name().unwrap()),
        ("bridge.pem".as_ref(), "bridge.key.pem".as_ref())
    );
    assert_eq!((mode(&c), mode(&k), mode(&dir)), (0o600, 0o600, 0o700));
    let call = fake.calls.lock().unwrap()[0].clone();
    assert!(call.starts_with("openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 13 -subj /CN=brisk-bridge"), "{call}");
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        2,
        "no temp files left"
    );
    let pair = |d: f64| {
        age(&k, d);
        age(&c, d);
    };
    pair(9.9);
    ensure_cert(&*fake, &dir, SystemTime::now()).unwrap();
    assert_eq!(fake.calls("openssl"), 1, "fresh: kept");
    pair(10.01);
    ensure_cert(&*fake, &dir, SystemTime::now()).unwrap();
    assert_eq!(fake.calls("openssl"), 2, "older than 10 days: renewed");
    std::fs::remove_file(&k).unwrap();
    ensure_cert(&*fake, &dir, SystemTime::now()).unwrap();
    assert_eq!(fake.calls("openssl"), 3, "missing key: renewed");
    // a key newer than its cert is from another run (a renewal cut off between the renames)
    pair(2.0);
    age(&k, 1.0);
    ensure_cert(&*fake, &dir, SystemTime::now()).unwrap();
    assert_eq!(fake.calls("openssl"), 4, "key newer than cert: renewed");
    // an mtime in the future (clock jumped) counts as stale, not as fresh
    pair(-1.0);
    ensure_cert(&*fake, &dir, SystemTime::now()).unwrap();
    assert_eq!(fake.calls("openssl"), 5, "future mtime: renewed");
    pair(1.0);
    ensure_cert(&*fake, &dir, SystemTime::now()).unwrap();
    assert_eq!(fake.calls("openssl"), 5, "same-age pair: kept");
}

#[test]
fn openssl_failure_is_a_clean_error_and_keeps_the_old_pair() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("tls");
    let broken = Arc::new(Fake {
        openssl_ok: false,
        ..Arc::into_inner(Fake::new(None)).unwrap()
    });
    let e = ensure_cert(&*broken, &dir, SystemTime::now()).unwrap_err();
    assert!(
        e.contains("couldn't make the bridge certificate") && e.contains("openssl"),
        "{e}"
    );
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "temp files removed"
    );
    std::fs::write(dir.join("bridge.pem"), "old cert").unwrap();
    std::fs::write(dir.join("bridge.key.pem"), "old key").unwrap();
    age(&dir.join("bridge.pem"), 11.0);
    assert!(ensure_cert(&*broken, &dir, SystemTime::now()).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("bridge.key.pem")).unwrap(),
        "old key"
    );
}

#[tokio::test]
async fn stream_start_with_tailscale_down_is_a_clear_error() {
    let down = Arc::new(Fake {
        ip: None,
        ..Arc::into_inner(Fake::new(None)).unwrap()
    });
    let (app, _st, _d) = app(down, &[], &[]);
    let req = Request::builder()
        .method("POST")
        .uri("/api/stream")
        .header("host", "localhost:8443")
        .header("x-brisk-secret", SECRET)
        .body(Body::from(r#"{"preset":"balanced"}"#))
        .unwrap();
    let r = app.oneshot(req).await.unwrap();
    assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let b = axum::body::to_bytes(r.into_body(), 1 << 16).await.unwrap();
    assert!(String::from_utf8_lossy(&b).contains("tailscale"), "{b:?}");
}
