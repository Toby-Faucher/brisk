use crate::config::Config;
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Later tasks add fields (Steam cache, supervisor) next to `cfg`.
pub struct AppState {
    pub cfg: Config,
}

/// Mount API routes on `api` below; everything else falls through to `dist/`. The guard wraps all.
pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new();
    Router::new()
        .nest("/api", api.fallback(|| async { StatusCode::NOT_FOUND }))
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Host check on everything (DNS rebinding), secret on every non-GET /api request.
async fn guard(State(s): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == "localhost:8443" || h == "127.0.0.1:8443");
    let needs_secret = req.method() != Method::GET && req.uri().path().starts_with("/api");
    let secret_ok = !needs_secret
        || req
            .headers()
            .get("x-brisk-secret")
            .is_some_and(|v| ct_eq(v.as_bytes(), s.cfg.secret.as_bytes()));
    if host_ok && secret_ok {
        next.run(req).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

async fn static_file(State(s): State<Arc<AppState>>, req: Request) -> Response {
    let rel = req.uri().path().trim_start_matches('/');
    let safe = Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    if !safe || (req.method() != Method::GET && req.method() != Method::HEAD) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let dist: &PathBuf = &s.cfg.dist;
    let mut path = dist.join(rel);
    if path.is_dir() {
        path.push("index.html");
    }
    // Unknown page: the root index, so client-side routes still load.
    if !path.is_file() {
        path = dist.join("index.html");
    }
    match tokio::fs::read(&path).await {
        Ok(b) => ([(header::CONTENT_TYPE, mime(&path))], b).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn mime(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}
