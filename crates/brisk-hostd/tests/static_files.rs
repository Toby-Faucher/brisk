//! Static serving (web/dist), written once and run against every backend: disk always, the
//! embedded build too with `--features embed` (built from the same fixture at runtime).
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use brisk_hostd::config::Config;
use brisk_hostd::http::{AppState, router};
use brisk_hostd::site::Site;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower::ServiceExt;

const H: &str = "localhost:8443";
const JS: &str = "console.log('brisk');";

/// `t/dist -> dist.1` like `brisk-service.sh update` leaves it, with escapes planted inside.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let b = t.path().join("dist.1");
    let w = |p: &str, s: &[u8]| {
        let p = b.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };
    w("index.html", b"<h1>home</h1>");
    w("library/index.html", b"<h1>lib</h1>");
    w("_astro/app.js", JS.as_bytes());
    w("_astro/app.js.br", b"BR");
    w("_astro/app.js.gz", b"GZ");
    w("_astro/geist.woff2", b"wOF2");
    w("icon.svg", b"<svg/>");
    w("data.json", b"{}");
    w("_astro/m.wasm", b"\0asm");
    w("_astro/w.mjs", b"export {}");
    std::fs::create_dir(b.join("empty")).unwrap();
    std::fs::write(t.path().join("outside.js"), "secret").unwrap();
    symlink(t.path().join("outside.js"), b.join("_astro/leak.js")).unwrap();
    symlink(t.path(), b.join("up")).unwrap();
    symlink(t.path().join("nowhere"), b.join("dangling.js")).unwrap();
    symlink(t.path().join("nowhere"), b.join("dangling")).unwrap();
    symlink("dist.1", t.path().join("dist")).unwrap();
    let dist = t.path().join("dist");
    (t, dist)
}

/// One router per backend, all serving the fixture.
fn apps(dist: &Path) -> Vec<(&'static str, axum::Router)> {
    let app = |site: Option<Site>| {
        let mut st = AppState::new(Config::from_env(dist.to_path_buf()));
        if let Some(s) = site {
            st.site = s;
        }
        router(Arc::new(st))
    };
    #[allow(unused_mut)]
    let mut v = vec![("disk", app(None))];
    #[cfg(feature = "embed")]
    v.push((
        "embed",
        app(Some(Site::Mem(brisk_hostd::site::Mem::from_dir(dist)))),
    ));
    v
}

async fn get(app: &axum::Router, method: &str, uri: &str, hdrs: &[(&str, &str)]) -> Response {
    let mut r = Request::builder().method(method).uri(uri).header("host", H);
    for (k, v) in hdrs {
        r = r.header(*k, *v);
    }
    app.clone()
        .oneshot(r.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn text(r: Response) -> String {
    let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    String::from_utf8_lossy(&b).into_owned()
}

fn h<'a>(r: &'a Response, k: &str) -> &'a str {
    r.headers().get(k).map_or("", |v| v.to_str().unwrap_or("?"))
}

#[tokio::test]
async fn pages_assets_and_the_spa_fallback_keep_their_old_rules() {
    let (_t, dist) = fixture();
    for (name, app) in apps(&dist) {
        for (p, want) in [
            ("/", "<h1>home</h1>"),
            ("/library", "<h1>lib</h1>"),
            ("/library/", "<h1>lib</h1>"),
            ("/library/index.html", "<h1>lib</h1>"),
            ("/_astro/app.js", JS),
            // unknown page (no extension): the root index, so client-side routes load
            ("/nope", "<h1>home</h1>"),
            ("/stream/deep", "<h1>home</h1>"),
            ("/Library", "<h1>home</h1>"),
            ("/empty", "<h1>home</h1>"),
            ("/dangling", "<h1>home</h1>"),
            ("//library", "<h1>lib</h1>"),
            ("/library//index.html", "<h1>lib</h1>"),
        ] {
            let r = get(&app, "GET", p, &[]).await;
            assert_eq!(r.status(), StatusCode::OK, "{name} {p}");
            assert_eq!(text(r).await, want, "{name} {p}");
        }
        // a missing file with an extension is a missing asset, never index.html
        for p in [
            "/_astro/gone.js",
            "/a/b.css",
            "/favicon.ico",
            "/INDEX.HTML",
            "/dangling.js",
        ] {
            let r = get(&app, "GET", p, &[]).await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{name} {p}");
        }
        for m in ["POST", "PUT", "DELETE"] {
            let r = get(&app, m, "/", &[]).await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{name} {m}");
        }
    }
}

#[tokio::test]
async fn nothing_outside_the_current_build_is_served() {
    let (_t, dist) = fixture();
    for (name, app) in apps(&dist) {
        for p in [
            "/../outside.js",
            "/_astro/../../outside.js",
            "/%2e%2e/outside.js",
            "/%2E%2E/%2e%2e/outside.js",
            "/..%2foutside.js",
            "/%2e%2e%2foutside.js",
            "/%2fetc%2fpasswd",
            "/_astro%2fapp.js",
            "/..%5coutside.js",
            "/%5c..%5coutside",
            "/a%00.js",
            "/%00",
            "/./index.html",
            "/%ff.js",
            // symlinks inside the build that point out of it
            "/_astro/leak.js",
            "/up/outside.js",
        ] {
            let r = get(&app, "GET", p, &[]).await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{name} {p}");
            assert!(!text(r).await.contains("secret"), "{name} {p}");
        }
        // An extensionless path that exists but escapes is refused on disk; the embedded build
        // never holds it (build.rs refuses such trees), so there it is an unknown page.
        let r = get(&app, "GET", "/up", &[]).await;
        let want = if name == "disk" { 404 } else { 200 };
        assert_eq!(r.status().as_u16(), want, "{name}");
        assert!(!text(r).await.contains("secret"), "{name}");
    }
}

#[tokio::test]
async fn cache_type_and_security_headers() {
    let (_t, dist) = fixture();
    for (name, app) in apps(&dist) {
        for (p, ct, cache) in [
            ("/", "text/html; charset=utf-8", "no-cache"),
            ("/nope", "text/html; charset=utf-8", "no-cache"),
            // the index served for an unknown page under _astro/ is not cached for a year
            ("/_astro/nope", "text/html; charset=utf-8", "no-cache"),
            (
                "/_astro/app.js",
                "text/javascript",
                "public, max-age=31536000, immutable",
            ),
            (
                "/_astro/w.mjs",
                "text/javascript",
                "public, max-age=31536000, immutable",
            ),
            (
                "/_astro/geist.woff2",
                "font/woff2",
                "public, max-age=31536000, immutable",
            ),
            (
                "/_astro/m.wasm",
                "application/wasm",
                "public, max-age=31536000, immutable",
            ),
            ("/icon.svg", "image/svg+xml", "no-cache"),
            ("/data.json", "application/json", "no-cache"),
        ] {
            let r = get(&app, "GET", p, &[]).await;
            assert_eq!(r.status(), StatusCode::OK, "{name} {p}");
            assert_eq!(h(&r, "content-type"), ct, "{name} {p}");
            assert_eq!(h(&r, "cache-control"), cache, "{name} {p}");
            assert_eq!(h(&r, "x-content-type-options"), "nosniff", "{name} {p}");
            assert_eq!(h(&r, "referrer-policy"), "no-referrer", "{name} {p}");
            assert_eq!(h(&r, "x-frame-options"), "DENY", "{name} {p}");
            assert_eq!(
                h(&r, "cross-origin-resource-policy"),
                "same-origin",
                "{name} {p}"
            );
            assert!(!h(&r, "etag").is_empty(), "{name} {p}");
            assert!(!h(&r, "last-modified").is_empty(), "{name} {p}");
            assert_eq!(h(&r, "accept-ranges"), "bytes", "{name} {p}");
            assert!(r.headers().get("content-security-policy").is_none());
        }
        // a 404 asset is not cached for a year
        let r = get(&app, "GET", "/_astro/gone.js", &[]).await;
        assert_eq!(h(&r, "cache-control"), "no-cache", "{name}");
        assert_eq!(h(&r, "x-content-type-options"), "nosniff", "{name}");
    }
}

#[tokio::test]
async fn etag_and_last_modified_give_304() {
    let (_t, dist) = fixture();
    for (name, app) in apps(&dist) {
        for p in ["/", "/_astro/app.js", "/library"] {
            let r = get(&app, "GET", p, &[]).await;
            let (etag, lm) = (
                h(&r, "etag").to_string(),
                h(&r, "last-modified").to_string(),
            );
            let r = get(&app, "GET", p, &[("if-none-match", &etag)]).await;
            assert_eq!(r.status(), StatusCode::NOT_MODIFIED, "{name} {p}");
            assert_eq!(h(&r, "etag"), etag, "{name} {p}");
            assert!(text(r).await.is_empty());
            let r = get(&app, "GET", p, &[("if-modified-since", &lm)]).await;
            assert_eq!(r.status(), StatusCode::NOT_MODIFIED, "{name} {p}");
            let r = get(&app, "GET", p, &[("if-none-match", "\"other\"")]).await;
            assert_eq!(r.status(), StatusCode::OK, "{name} {p}");
        }
    }
}

#[tokio::test]
async fn range_head_and_precompressed_siblings() {
    let (_t, dist) = fixture();
    for (name, app) in apps(&dist) {
        let r = get(&app, "GET", "/_astro/app.js", &[("range", "bytes=0-6")]).await;
        assert_eq!(r.status(), StatusCode::PARTIAL_CONTENT, "{name}");
        assert_eq!(
            h(&r, "content-range"),
            format!("bytes 0-6/{}", JS.len()),
            "{name}"
        );
        assert_eq!(text(r).await, "console", "{name}");
        let r = get(&app, "GET", "/_astro/app.js", &[("range", "bytes=999-")]).await;
        assert_eq!(r.status(), StatusCode::RANGE_NOT_SATISFIABLE, "{name}");

        let r = get(&app, "HEAD", "/_astro/app.js", &[]).await;
        assert_eq!(r.status(), StatusCode::OK, "{name}");
        assert_eq!(h(&r, "content-length"), JS.len().to_string(), "{name}");
        assert!(text(r).await.is_empty(), "{name}");

        for (ae, body, ce) in [("br", "BR", "br"), ("gzip", "GZ", "gzip"), ("", JS, "")] {
            let r = get(&app, "GET", "/_astro/app.js", &[("accept-encoding", ae)]).await;
            assert_eq!(r.status(), StatusCode::OK, "{name} {ae}");
            assert_eq!(h(&r, "content-encoding"), ce, "{name} {ae}");
            assert_eq!(h(&r, "content-type"), "text/javascript", "{name} {ae}");
            assert_eq!(text(r).await, body, "{name} {ae}");
        }
    }
}

#[tokio::test]
async fn api_answers_only_gain_nosniff() {
    let (_t, dist) = fixture();
    for (name, app) in apps(&dist) {
        for secret in [None, Some("wrong")] {
            let hd: Vec<(&str, &str)> = secret.map(|s| ("x-brisk-secret", s)).into_iter().collect();
            let r = get(&app, "GET", "/api/nope", &hd).await;
            assert_eq!(r.status(), StatusCode::FORBIDDEN, "{name}");
            assert_eq!(h(&r, "x-content-type-options"), "nosniff", "{name}");
            for k in [
                "cache-control",
                "x-frame-options",
                "referrer-policy",
                "cross-origin-resource-policy",
            ] {
                assert!(r.headers().get(k).is_none(), "{name} {k}");
            }
        }
    }
}

#[cfg(feature = "embed")]
#[test]
fn compiled_in_build_matches_its_source_dir() {
    let m = brisk_hostd::site::Mem::embedded();
    let src = brisk_hostd::site::Mem::embedded_dir();
    let want = std::fs::read(Path::new(src).join("index.html")).unwrap();
    assert_eq!(m.file("index.html").as_deref(), Some(&want[..]));
}

#[test]
fn brisk_static_picks_disk_unless_embed_is_asked_or_dist_is_missing() {
    let (_t, dist) = fixture();
    let gone = dist.join("missing");
    let disk = |s: Site| matches!(s, Site::Disk(_));
    assert!(disk(Site::choose(dist.clone(), None)));
    assert!(disk(Site::choose(dist.clone(), Some("disk"))));
    assert!(disk(Site::choose(gone.clone(), Some("disk"))));
    assert!(disk(Site::choose(dist.clone(), Some("bogus"))));
    // without the feature there is nothing to embed: always disk
    let compiled_in = cfg!(feature = "embed");
    assert_eq!(
        disk(Site::choose(dist.clone(), Some("embed"))),
        !compiled_in
    );
    assert_eq!(disk(Site::choose(gone, None)), !compiled_in);
}
