use crate::auth::{Hosts, tailnet_ok};
use crate::clipboard::{self, Clip, Clipboard, Kind, WlBackend};
use crate::config::Config;
use crate::files::Files;
use crate::host::{self, Clock, RealRunner, RestartError, Restarter, Runner, SystemClock};
use crate::prefs::{Entry, Prefs};
use crate::running::{Quit, QuitError, Quitter};
use crate::sessions::{MAX_POINTS, Point, SeriesError, valid_point};
use crate::site::Site;
use crate::steam::{self, GamesResult};
use crate::steam_info;
use crate::stream::{Codec, Preset, StreamReq, Supervisor};
use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub cfg: Config,
    /// Last `/api/games` result, for `is_known_appid`.
    pub games: Mutex<Option<GamesResult>>,
    pub restarter: Restarter,
    pub stream: Supervisor,
    pub clip: Arc<Clipboard>,
    pub files: Files,
    pub quit: Quitter,
    pub prefs: Prefs,
    pub hosts: Hosts,
    /// Disk (`cfg.dist`) unless main picks the embedded build (feature `embed`, BRISK_STATIC).
    pub site: Site,
    /// A background cache warm is running (see `steam_info::warm`).
    pub warming: AtomicBool,
    /// Friends online: in memory only (other people's names never reach disk), see `steam_info::friends_cached`.
    pub friends: Mutex<Option<(std::time::Instant, steam_info::Friends)>>,
}

impl AppState {
    pub fn new(cfg: Config) -> Self {
        Self::with_host(cfg, Arc::new(RealRunner), Arc::new(SystemClock::default()))
    }

    /// Tests inject a fake runner and clock so nothing real is restarted.
    pub fn with_host(cfg: Config, runner: Arc<dyn Runner>, clock: Arc<dyn Clock>) -> Self {
        AppState {
            stream: Supervisor::new(&cfg, runner.clone()),
            games: Mutex::new(None),
            clip: Arc::new(Clipboard::new(Arc::new(WlBackend::default()))),
            files: Files::new(cfg.downloads_dir.clone()),
            prefs: Prefs::load(cfg.data_dir.join("prefs.json")),
            quit: Quitter::new(runner.clone(), cfg.proc_dir.clone()),
            hosts: Hosts::new(&cfg.allowed_hosts, runner.clone()),
            site: Site::Disk(cfg.dist.clone()),
            warming: AtomicBool::new(false),
            friends: Mutex::new(None),
            cfg,
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
    // Games with play time are the ones worth warming; the task never delays this answer.
    steam_info::warm(
        s.clone(),
        g.games
            .iter()
            .filter(|x| x.installed && x.playtime_min > 0)
            .map(|x| x.appid)
            .collect(),
    );
    match serde_json::to_vec(&g) {
        Ok(b) => ([(header::CONTENT_TYPE, "application/json")], b).into_response(),
        Err(e) => {
            eprintln!("games: serializing the cache failed: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "games unavailable")
        }
    }
}

/// Store details for the detail panel; 404 for a game that is neither owned nor installed, so
/// no upstream call is made for it. A Store failure is a 200 with `available: false`.
async fn game_info(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<u32>,
    req: Request,
) -> Response {
    if !s.is_known_appid(id).await {
        return err(StatusCode::NOT_FOUND, "unknown game");
    }
    let refresh = req
        .uri()
        .query()
        .is_some_and(|q| q.split('&').any(|p| p == "refresh=1"));
    axum::Json(steam_info::info(&s.cfg, id, refresh).await).into_response()
}

/// Achievements for the detail panel; 404 for an unknown game before any upstream call. A game
/// without stats or a private profile is a 200 with `available: false`.
async fn game_achievements(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<u32>,
    req: Request,
) -> Response {
    if !s.is_known_appid(id).await {
        return err(StatusCode::NOT_FOUND, "unknown game");
    }
    let refresh = req
        .uri()
        .query()
        .is_some_and(|q| q.split('&').any(|p| p == "refresh=1"));
    axum::Json(steam_info::achievements(&s.cfg, id, refresh).await).into_response()
}

/// Latest headlines for the detail panel; 404 for an unknown game before any upstream call. An
/// upstream failure is a 200 with an empty list.
async fn game_news(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<u32>,
    req: Request,
) -> Response {
    if !s.is_known_appid(id).await {
        return err(StatusCode::NOT_FOUND, "unknown game");
    }
    let refresh = req
        .uri()
        .query()
        .is_some_and(|q| q.split('&').any(|p| p == "refresh=1"));
    axum::Json(steam_info::news(&s.cfg, id, refresh).await).into_response()
}

/// Screenshot names and a cloud-save summary from the host's Steam folder; 404 for an unknown game.
/// No folder is a 200 with empty fields.
async fn game_local(State(s): State<Arc<AppState>>, UrlPath(id): UrlPath<u32>) -> Response {
    if !s.is_known_appid(id).await {
        return err(StatusCode::NOT_FOUND, "unknown game");
    }
    let s2 = s.clone();
    match tokio::task::spawn_blocking(move || steam_info::local(&s2.cfg, id)).await {
        Ok(l) => axum::Json(l).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "listing failed"),
    }
}

/// One screenshot file, only if `name` is in the game's listing (see `steam_info::screenshot`).
async fn game_screenshot(
    State(s): State<Arc<AppState>>,
    UrlPath((id, name)): UrlPath<(u32, String)>,
) -> Response {
    if !s.is_known_appid(id).await {
        return err(StatusCode::NOT_FOUND, "unknown game");
    }
    let s2 = s.clone();
    match tokio::task::spawn_blocking(move || steam_info::screenshot(&s2.cfg, id, &name)).await {
        Ok(Some((ct, body))) => (
            [
                (header::CONTENT_TYPE, ct),
                (header::CACHE_CONTROL, "private, max-age=3600"),
            ],
            body,
        )
            .into_response(),
        _ => err(StatusCode::NOT_FOUND, "no such screenshot"),
    }
}

/// Friends in a game or online. Any failure is a 200 with `available: false`.
async fn friends(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(steam_info::friends_cached(&s).await).into_response()
}

async fn host_status(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(host::host_info(&s.cfg).await).into_response()
}

async fn sunshine_restart(State(s): State<Arc<AppState>>) -> Response {
    let (code, msg) = match s.restarter.restart().await {
        Ok(()) => return StatusCode::NO_CONTENT.into_response(),
        Err(RestartError::TooSoon(secs)) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, secs.to_string())],
                axum::Json(serde_json::json!({ "error": "Sunshine was restarted a moment ago" })),
            )
                .into_response();
        }
        Err(RestartError::Failed) => (StatusCode::INTERNAL_SERVER_ERROR, "restart failed"),
    };
    (code, axum::Json(serde_json::json!({ "error": msg }))).into_response()
}

fn err(code: StatusCode, msg: &str) -> Response {
    (code, axum::Json(serde_json::json!({ "error": msg }))).into_response()
}

/// JSON body or 400 (axum's `Json` would answer 422). `appid` is a u32 here or the request fails.
#[allow(clippy::result_large_err)]
fn parse<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T, Response> {
    serde_json::from_slice(b)
        .map_err(|e| err(StatusCode::BAD_REQUEST, &format!("bad request: {e}")))
}

#[allow(clippy::result_large_err)]
async fn check_appid(s: &AppState, id: Option<u32>) -> Result<(), Response> {
    match id {
        Some(id) if !s.is_known_appid(id).await => {
            Err(err(StatusCode::BAD_REQUEST, "unknown game"))
        }
        _ => Ok(()),
    }
}

async fn stream_get(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(s.stream.info()).into_response()
}

async fn stream_start(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    let run = async {
        let req: StreamReq = parse(&b)?;
        req.flags().map_err(|e| err(StatusCode::BAD_REQUEST, &e))?;
        check_appid(&s, req.appid).await?;
        let name = req.appid.and_then(|id| {
            let g = s.games.lock().unwrap();
            g.as_ref()?
                .games
                .iter()
                .find(|g| g.appid == id)
                .map(|g| g.name.clone())
        });
        // Own task: a client that hangs up must not abort a start halfway through a stop.
        let s2 = s.clone();
        tokio::spawn(async move { s2.stream.start(req, name).await })
            .await
            .unwrap_or_else(|e| Err(e.to_string()))
            .map_err(|e| {
                let down = s.stream.shutting_down();
                err(
                    if down {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::INTERNAL_SERVER_ERROR
                    },
                    &e,
                )
            })
    };
    match run.await {
        Ok(i) => axum::Json(i).into_response(),
        Err(r) => r,
    }
}

async fn stream_stop(State(s): State<Arc<AppState>>) -> Response {
    match tokio::spawn(async move { s.stream.stop().await }).await {
        Ok(i) => axum::Json(i).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "stop failed"),
    }
}

#[derive(Deserialize)]
struct AppId {
    appid: u32,
}

/// `steam steam://<verb>/<appid>` for a known game, detached: 204 once it runs, 400, or 500 if it
/// could not start.
async fn steam_cmd(s: Arc<AppState>, b: &[u8], verb: &'static str) -> Response {
    let id = match parse::<AppId>(b) {
        Ok(a) => a.appid,
        Err(r) => return r,
    };
    if let Err(r) = check_appid(&s, Some(id)).await {
        return r;
    }
    let s2 = s.clone();
    match tokio::task::spawn_blocking(move || s2.stream.steam(verb, id)).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        _ => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "steam didn't take the command",
        ),
    }
}

async fn launch(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    steam_cmd(s, &b, "rungameid").await
}

async fn install(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    steam_cmd(s, &b, "install").await
}

#[derive(Deserialize)]
struct QuitReq {
    appid: u32,
    #[serde(default)]
    force: bool,
}

/// `{state: "closed" | "still-running"}`; 400 for an unknown game, 409 if it is not running or
/// already being quit. Answers within ~25 s; a longer quit carries on and reports still-running.
async fn quit(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    let q = match parse::<QuitReq>(&b) {
        Ok(q) => q,
        Err(r) => return r,
    };
    if let Err(r) = check_appid(&s, Some(q.appid)).await {
        return r;
    }
    // Own task: a client that hangs up must not stop a quit between its steps.
    let s2 = s.clone();
    let job = tokio::spawn(async move { s2.quit.quit(q.appid, q.force).await });
    let state = match tokio::time::timeout(std::time::Duration::from_secs(25), job).await {
        Ok(Ok(Ok(st))) => st,
        Ok(Ok(Err(QuitError::NotRunning))) => return err(StatusCode::CONFLICT, "not running"),
        Ok(Ok(Err(QuitError::Busy))) => return err(StatusCode::CONFLICT, "already quitting"),
        Ok(Ok(Err(QuitError::Root))) => {
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "hostd runs as root; quit is off",
            );
        }
        Ok(Err(_)) => return err(StatusCode::INTERNAL_SERVER_ERROR, "quit failed"),
        Err(_) => Quit::StillRunning,
    };
    axum::Json(serde_json::json!({ "state": state })).into_response()
}

async fn sessions(State(s): State<Arc<AppState>>) -> Response {
    // Each row carries a 24-point delay sparkline (from its series file) so the list needs no per-card GET.
    let rows: Vec<serde_json::Value> = s
        .stream
        .sessions(100)
        .into_iter()
        .filter_map(|r| {
            let spark = s.stream.sessions.spark(r.started, 24);
            let id = r.started;
            let mut v = serde_json::to_value(r)
                .inspect_err(|e| eprintln!("sessions: skipping session {id}: {e}"))
                .ok()?;
            v["spark"] = spark.into();
            Some(v)
        })
        .collect();
    axum::Json(serde_json::json!({ "sessions": rows })).into_response()
}

/// `{session, series}`; 404 for a session hostd has no record of.
async fn session_get(State(s): State<Arc<AppState>>, UrlPath(id): UrlPath<u64>) -> Response {
    match s.stream.sessions.get(id) {
        Some(r) => {
            axum::Json(serde_json::json!({ "session": r, "series": s.stream.sessions.series(id) }))
                .into_response()
        }
        None => err(StatusCode::NOT_FOUND, "no such session"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeriesBody {
    points: Vec<Point>,
}

/// `{points: [[t, delayMs, fps, mbps], ...]}` merged by t; 400 for a bad or implausible point
/// (JSON has no NaN), 413 past MAX_POINTS (in the request or once merged) or 1 MiB, 404 for an
/// unknown session.
async fn series_put(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<u64>,
    b: axum::body::Bytes,
) -> Response {
    let b = match parse::<SeriesBody>(&b) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.points.len() > MAX_POINTS {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "too many points");
    }
    if !b.points.iter().all(valid_point) {
        return err(StatusCode::BAD_REQUEST, "bad point");
    }
    match s.stream.sessions.put_series(id, &b.points) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SeriesError::NoSession) => err(StatusCode::NOT_FOUND, "no such session"),
        Err(SeriesError::TooMany) => err(StatusCode::PAYLOAD_TOO_LARGE, "too many points"),
    }
}

/// No `since`: 204 with only the seq (the page starts from it without writing anything). Newer
/// text is JSON, a newer image the PNG itself; `x-brisk-seq` is on every answer.
async fn clip_get(State(s): State<Arc<AppState>>, req: Request) -> Response {
    let since = req.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|p| p.strip_prefix("since=")?.parse().ok())
    });
    let (seq, c) = s.clip.since(since);
    let h = [("x-brisk-seq", seq.to_string())];
    match c {
        None => (StatusCode::NO_CONTENT, h).into_response(),
        Some(c) if c.kind == Kind::Text => {
            let text = String::from_utf8_lossy(&c.data);
            (
                h,
                axum::Json(serde_json::json!({ "seq": seq, "kind": "text", "text": text })),
            )
                .into_response()
        }
        Some(c) => (h, [(header::CONTENT_TYPE, "image/png")], c.data).into_response(),
    }
}

/// `text/plain` (UTF-8) or `image/png` body becomes bawn's clipboard.
async fn clip_post(State(s): State<Arc<AppState>>, req: Request) -> Response {
    let ct = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let kind = match ct.split(';').next().unwrap_or("").trim() {
        "text/plain" => Kind::Text,
        "image/png" => Kind::Image,
        _ => {
            return err(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "only text/plain and image/png",
            );
        }
    };
    let data = match axum::body::to_bytes(req.into_body(), clipboard::MAX).await {
        Ok(b) => b,
        Err(_) => return err(StatusCode::PAYLOAD_TOO_LARGE, "clipboard is over 16 MiB"),
    };
    let valid = match kind {
        Kind::Text => std::str::from_utf8(&data).is_ok(),
        Kind::Image => data.starts_with(b"\x89PNG\r\n\x1a\n"),
    };
    if !valid {
        return err(StatusCode::BAD_REQUEST, "not UTF-8 text or a PNG");
    }
    let s2 = s.clone();
    match tokio::task::spawn_blocking(move || s2.clip.set(Clip { kind, data })).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        _ => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "couldn't set bawn's clipboard",
        ),
    }
}

/// Reply `{path, size}`; see files.rs.
async fn file_put(
    State(s): State<Arc<AppState>>,
    UrlPath(name): UrlPath<String>,
    req: Request,
) -> Response {
    let len = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse().ok());
    match s.files.upload(&name, len, req.into_body()).await {
        Ok((p, size)) => axum::Json(serde_json::json!({ "path": p, "size": size })).into_response(),
        Err((code, msg)) => err(code, &msg),
    }
}

async fn prefs_get(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(serde_json::json!({ "prefs": s.prefs.all(), "homeOrder": s.prefs.home_order() }))
        .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LayoutBody {
    home_order: Vec<String>,
}

/// The Home card order: not an array of strings is a 400; bad, duplicate and surplus ids are
/// dropped (prefs::clean_home_order) and the stored order is returned.
async fn layout_put(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    match parse::<LayoutBody>(&b) {
        Ok(b) => {
            axum::Json(serde_json::json!({ "homeOrder": s.prefs.set_home_order(b.home_order) }))
                .into_response()
        }
        Err(r) => r,
    }
}

/// Reset layout: forget the saved order (Home falls back to its default).
async fn layout_delete(State(s): State<Arc<AppState>>) -> Response {
    s.prefs.set_home_order(vec![]);
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrefsBody {
    preset: Preset,
    #[serde(default)]
    codec: Codec,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<u32>,
    bitrate_kbps: Option<u32>,
}

/// Same ranges as `StreamReq`; custom fields are kept only for the custom preset. 400 for a bad
/// body or for the Desktop app, which has no per-game quality.
async fn prefs_put(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<u32>,
    b: axum::body::Bytes,
) -> Response {
    if id == 0 || id == s.cfg.app_id {
        return err(
            StatusCode::BAD_REQUEST,
            "the Desktop has no per-game quality",
        );
    }
    let b = match parse::<PrefsBody>(&b) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let req = StreamReq {
        preset: b.preset,
        width: b.width,
        height: b.height,
        fps: b.fps,
        bitrate_kbps: b.bitrate_kbps,
        codec: b.codec,
        appid: None,
    };
    if let Err(e) = req.flags() {
        return err(StatusCode::BAD_REQUEST, &e);
    }
    let custom = b.preset == Preset::Custom;
    let keep = |v: Option<u32>| v.filter(|_| custom);
    let e = Entry {
        preset: b.preset,
        codec: b.codec,
        width: keep(b.width),
        height: keep(b.height),
        fps: keep(b.fps),
        bitrate_kbps: keep(b.bitrate_kbps),
        updated: 0,
    };
    axum::Json(s.prefs.put(id, e)).into_response()
}

async fn prefs_delete(State(s): State<Arc<AppState>>, UrlPath(id): UrlPath<u32>) -> Response {
    s.prefs.delete(id);
    StatusCode::NO_CONTENT.into_response()
}

/// Mount API routes on `api` below; everything else falls through to the static site (site.rs).
/// The guard wraps all.
pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/games", get(games))
        .route("/games/{appid}/info", get(game_info))
        .route("/games/{appid}/achievements", get(game_achievements))
        .route("/games/{appid}/news", get(game_news))
        .route("/games/{appid}/local", get(game_local))
        .route("/games/{appid}/screenshots/{name}", get(game_screenshot))
        .route("/friends", get(friends))
        .route("/host", get(host_status))
        .route("/sunshine/restart", post(sunshine_restart))
        .route(
            "/stream",
            get(stream_get).post(stream_start).delete(stream_stop),
        )
        .route("/launch", post(launch))
        .route("/install", post(install))
        .route("/quit", post(quit))
        .route("/sessions", get(sessions))
        .route("/sessions/{started}", get(session_get))
        .route(
            "/sessions/{started}/series",
            put(series_put).layer(DefaultBodyLimit::max(1 << 20)),
        )
        .route("/prefs", get(prefs_get))
        // static beats the {appid} capture in axum's router
        .route("/prefs/layout", put(layout_put).delete(layout_delete))
        .route("/prefs/{appid}", put(prefs_put).delete(prefs_delete))
        .route("/clipboard", get(clip_get).post(clip_post))
        // Uploads stream to disk with their own 8 GiB cap; only this route lifts the body limit.
        .route(
            "/files/{name}",
            put(file_put).layer(DefaultBodyLimit::disable()),
        );
    Router::new()
        .nest("/api", api.fallback(|| async { StatusCode::NOT_FOUND }))
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Host check on everything (DNS rebinding); every /api request (GET too: /api/stream carries
/// the session token) needs the secret or an allowed tailnet login (see auth.rs).
async fn guard(State(s): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let hd = req.headers();
    let host = hd.get(header::HOST).and_then(|h| h.to_str().ok());
    let host_ok = match host {
        Some(h) => s.hosts.allowed(h).await,
        None => false,
    };
    let needs_auth = req.uri().path().starts_with("/api");
    let auth_ok = !needs_auth
        || hd
            .get("x-brisk-secret")
            .is_some_and(|v| ct_eq(v.as_bytes(), s.cfg.secret.as_bytes()))
        || tailnet_ok(
            &s.cfg.allowed_users,
            req.extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .is_some_and(|c| c.0.ip().is_loopback()),
            hd.get("tailscale-user-login").and_then(|v| v.to_str().ok()),
            hd.contains_key("x-brisk-csrf"),
        );
    let mut r = if host_ok && auth_ok {
        next.run(req).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    };
    r.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    r
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

async fn static_file(State(s): State<Arc<AppState>>, req: Request) -> Response {
    s.site.serve(req).await
}
