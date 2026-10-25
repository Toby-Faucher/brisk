use crate::config::Config;
use crate::host::{self, Clock, RealRunner, RestartError, Restarter, Runner, SystemClock};
use crate::steam::{self, GamesResult};
use axum::extract::{Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Later tasks add fields (supervisor) next to `cfg`.
pub struct AppState {
    pub cfg: Config,
    /// Last `/api/games` result, for `is_known_appid`.
    pub games: Mutex<Option<GamesResult>>,
    pub restarter: Restarter,
}

impl AppState {
    pub fn new(cfg: Config) -> Self {
        Self::with_host(cfg, Arc::new(RealRunner), Arc::new(SystemClock))
    }

    /// Tests inject a fake runner and clock so nothing real is restarted.
    pub fn with_host(cfg: Config, runner: Arc<dyn Runner>, clock: Arc<dyn Clock>) -> Self {
        AppState {
            cfg,
            games: Mutex::new(None),
            restarter: Restarter::new(runner, clock),
        }
    }

    /// Whether `id` is an owned or installed game; loads the library if nothing is cached yet.
    pub async fn is_known_appid(&self, id: u32) -> bool {
        let cached = self
            .games
            .lock()
            .unwrap()
            .as_ref()
            .map(|g| g.is_known_appid(id));
        match cached {
            Some(k) => k,
            None => {
                let g = steam::load_games(&self.cfg).await;
                let k = g.is_known_appid(id);
                *self.games.lock().unwrap() = Some(g);
                k
            }
        }
    }
}

async fn games(State(s): State<Arc<AppState>>, req: Request) -> Response {
    let refresh = req
        .uri()
        .query()
        .is_some_and(|q| q.split('&').any(|p| p == "refresh=1"));
    let g = steam::load_games_with(&s.cfg, refresh).await;
    *s.games.lock().unwrap() = Some(g.clone());
    (
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_vec(&g).unwrap_or_default(),
    )
        .into_response()
}

async fn host_status(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(host::host_info(&s.cfg).await).into_response()
}

async fn sunshine_restart(State(s): State<Arc<AppState>>) -> Response {
    let (code, msg) = match s.restarter.restart().await {
        Ok(()) => return StatusCode::NO_CONTENT.into_response(),
        Err(RestartError::TooSoon) => (
            StatusCode::TOO_MANY_REQUESTS,
            "Sunshine was restarted a moment ago",
        ),
        Err(RestartError::Failed) => (StatusCode::INTERNAL_SERVER_ERROR, "restart failed"),
    };
    (code, axum::Json(serde_json::json!({ "error": msg }))).into_response()
}

/// Mount API routes on `api` below; everything else falls through to `dist/`. The guard wraps all.
pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/games", get(games))
        .route("/host", get(host_status))
        .route("/sunshine/restart", post(sunshine_restart));
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
