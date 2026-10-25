use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use brisk_hostd::config::Config;
use brisk_hostd::http::{router, AppState};
use std::sync::Arc;
use tower::ServiceExt;

const SECRET: &str = "s3cret";

fn app(dist: &std::path::Path) -> axum::Router {
    let mut cfg = Config::from_env(dist.to_path_buf());
    cfg.secret = SECRET.into();
    router(Arc::new(AppState { cfg }))
}

async fn send(
    dist: &std::path::Path,
    method: &str,
    uri: &str,
    host: &str,
    secret: Option<&str>,
) -> Response {
    let mut r = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", host);
    if let Some(s) = secret {
        r = r.header("x-brisk-secret", s);
    }
    app(dist)
        .oneshot(r.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body(r: Response) -> String {
    let b = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    String::from_utf8(b.to_vec()).unwrap()
}

fn dist() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("index.html"), "<h1>home</h1>").unwrap();
    std::fs::create_dir(d.path().join("library")).unwrap();
    std::fs::write(d.path().join("library/index.html"), "<h1>lib</h1>").unwrap();
    d
}

const H: &str = "localhost:8443";

#[tokio::test]
async fn serves_index_from_dist() {
    let d = dist();
    let r = send(d.path(), "GET", "/", H, None).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(body(r).await, "<h1>home</h1>");
    assert_eq!(
        body(send(d.path(), "GET", "/library", "127.0.0.1:8443", None).await).await,
        "<h1>lib</h1>"
    );
    // unknown non-API path falls back to the index; unknown API path is a 404
    assert_eq!(
        body(send(d.path(), "GET", "/nope", H, None).await).await,
        "<h1>home</h1>"
    );
    assert_eq!(
        send(d.path(), "GET", "/api/nope", H, None).await.status(),
        StatusCode::NOT_FOUND
    );
    // no escaping dist
    assert_ne!(
        send(d.path(), "GET", "/../../etc/passwd", H, None)
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn foreign_host_is_403() {
    let d = dist();
    for h in ["evil.test", "localhost:9999", "localhost"] {
        assert_eq!(
            send(d.path(), "GET", "/", h, Some(SECRET)).await.status(),
            StatusCode::FORBIDDEN,
            "{h}"
        );
    }
}

#[tokio::test]
async fn post_without_secret_is_403() {
    let d = dist();
    for m in ["POST", "DELETE"] {
        assert_eq!(
            send(d.path(), m, "/api/stream", H, None).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(d.path(), m, "/api/stream", H, Some("wrong"))
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
}

#[tokio::test]
async fn post_with_secret_passes_auth() {
    let d = dist();
    assert_eq!(
        send(d.path(), "POST", "/api/unknown", H, Some(SECRET))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn get_needs_no_secret() {
    let d = dist();
    assert_eq!(
        send(d.path(), "GET", "/api/unknown", H, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}
