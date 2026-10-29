//! The HTTP API: shared state, the router, the guard and the helpers every route uses.
#![allow(
    clippy::result_large_err,
    reason = "handlers return an axum Response as their error, as axum does"
)]

mod controls;
mod games;
mod host;
mod launch;
mod notes;
mod prefs;
mod stream;

use crate::auth::{Hosts, tailnet_ok};
use crate::clipboard::{self, Clip, Clipboard, Kind, WlBackend};
use crate::config::Config;
use crate::controls::{Controls, CtlError, Kind as PadKind};
use crate::files::Files;
use crate::game::{Game, GameId};
use crate::host::{Clock, RealRunner, RestartError, Restarter, Runner, SystemClock};
use crate::launch::{LaunchOptions, LaunchOverride};
use crate::launchstore::{self, LaunchStore, StoreError};
use crate::machines::{self, Hub};
use crate::notes::{GameNote, NotesError, NotesStore, Tag};
use crate::prefs::{Entry, Prefs};
use crate::running::{Quit, QuitError, Quitter};
use crate::sessions::{MAX_POINTS, Point, SeriesError, valid_point};
use crate::site::Site;
use crate::steam::{self, GamesResult};
use crate::steam_info;
use crate::stream::{Codec, Preset, RawId, StreamReq, Supervisor};
use axum::Router;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use controls::*;
use games::*;
use host::*;
use launch::*;
use notes::*;
use prefs::*;
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use stream::*;

/// State shared by every route.
pub struct AppState {
    pub cfg: Config,
    /// Last `/api/games` result, for `game`.
    pub games: Mutex<Option<GamesResult>>,
    pub restarter: Restarter,
    pub stream: Supervisor,
    pub clip: Arc<Clipboard>,
    pub files: Files,
    /// Resumable uploads into the games folder (`uploads.rs`).
    pub uploads: crate::uploads::Uploads,
    /// One games-folder scan at a time, at least `scan_gap` apart.
    pub scan_gate: Mutex<ScanGate>,
    pub scan_gap: std::time::Duration,
    pub quit: Quitter,
    pub prefs: Prefs,
    /// Your notes and tags (`data_dir/notes.json`); private text, never logged.
    pub notes: NotesStore,
    /// Launch option defaults and per-game overrides (`data_dir/launch.json`).
    pub launch: LaunchStore,
    /// Dolphin's config directory (controller editor).
    pub controls: Controls,
    pub hosts: Hosts,
    /// Disk (`cfg.dist`) unless main picks the embedded build (feature `embed`, BRISK_STATIC).
    pub site: Site,
    /// A background cache warm is running (see `steam_info::warm`).
    pub warming: AtomicBool,
    /// Friends online: in memory only (other people's names never reach disk), see `steam_info::friends_cached`.
    pub friends: Mutex<Option<steam_info::FriendsEntry>>,
    /// Screenshot listings per game for a few seconds (see `steam_info::ShotCache`).
    pub shots: steam_info::ShotCache,
    /// Set when machines.json exists (main.rs); None = single host, no hub routes answer.
    pub hub: Option<Arc<Hub>>,
    /// When `?refresh=1` last went through for each emulator game (RetroAchievements), 30 s apart.
    pub ra_refresh: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    /// Art and details lookups for emulator games (`art.rs`).
    pub art: Arc<crate::art::Art>,
    /// library.json as last read, keyed by its size and modified time (`records`).
    lib: Mutex<LibCache>,
}

type LibCache = Option<((u64, std::time::SystemTime), Arc<Vec<crate::emu::Record>>)>;

#[derive(Default)]
/// When the last games-folder scan ran and whether one is running.
pub struct ScanGate {
    running: bool,
    last: Option<std::time::Instant>,
}

impl AppState {
    /// State with the real runner and clock.
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
            uploads: crate::uploads::Uploads::new(cfg.games_dir.clone()),
            scan_gate: Mutex::default(),
            scan_gap: std::time::Duration::from_secs(5),
            prefs: Prefs::load(cfg.data_dir.join("prefs.json")),
            notes: NotesStore::load(cfg.data_dir.join("notes.json")),
            launch: LaunchStore::load(cfg.data_dir.join("launch.json")),
            controls: Controls::new(cfg.dolphin_dir.clone()),
            quit: Quitter::new(runner.clone(), cfg.proc_dir.clone(), cfg.data_dir.clone()),
            hosts: Hosts::new(&cfg.allowed_hosts, runner.clone()),
            site: Site::Disk(cfg.dist.clone()),
            warming: AtomicBool::new(false),
            friends: Mutex::new(None),
            shots: steam_info::ShotCache::default(),
            hub: None,
            ra_refresh: Mutex::default(),
            art: Arc::new(crate::art::Art::default()),
            lib: Mutex::default(),
            cfg,
            restarter: Restarter::new(runner, clock),
        }
    }

    /// The library, re-read only when library.json changed (a stat per call, no parse).
    pub fn records(&self) -> Arc<Vec<crate::emu::Record>> {
        let key = std::fs::metadata(self.cfg.data_dir.join(crate::emu::LIBRARY_FILE))
            .and_then(|m| Ok((m.len(), m.modified()?)))
            .ok();
        let mut c = self.lib.lock().unwrap();
        if let (Some(k), Some((ck, r))) = (key, c.as_ref())
            && k == *ck
        {
            return r.clone();
        }
        let r = Arc::new(crate::emu::load(&self.cfg.data_dir));
        *c = key.map(|k| (k, r.clone()));
        r
    }

    /// An id as a request writes it (`steam:620`, or a bare number) as a GameId: a bare number
    /// in the old emulator range needs this machine's library to say which game it was.
    pub(crate) fn parse_id(&self, raw: &str) -> Option<GameId> {
        let recs = self.records();
        crate::emu::resolve(&recs, raw)
    }

    /// The owned or installed game `id`; loads the library if nothing is cached yet.
    pub async fn game(&self, id: &GameId) -> Option<Game> {
        let cached = self
            .games
            .lock()
            .unwrap()
            .as_ref()
            .map(|g| g.find(id).cloned());
        match cached {
            Some(k) => k,
            None => {
                let g = steam::load_games(&self.cfg).await;
                let k = g.find(id).cloned();
                *self.games.lock().unwrap() = Some(g);
                k
            }
        }
    }
}

pub(crate) fn err(code: StatusCode, msg: &str) -> Response {
    (code, axum::Json(serde_json::json!({ "error": msg }))).into_response()
}

/// JSON body or 400 (axum's `Json` would answer 422). `appid` is a u32 here or the request fails.
/// True when the query string has exactly `kv` (like `refresh=1`) as one of its parts.
pub(crate) fn query_flag(q: Option<&str>, kv: &str) -> bool {
    q.is_some_and(|q| q.split('&').any(|p| p == kv))
}

fn parse<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T, Response> {
    serde_json::from_slice(b)
        .map_err(|e| err(StatusCode::BAD_REQUEST, &format!("bad request: {e}")))
}

/// The id text of a request: `id` (text or number) wins over the old numeric `appid`.
fn raw_id(id: Option<RawId>, appid: Option<u32>) -> Option<String> {
    id.map(|i| i.text()).or(appid.map(|n| n.to_string()))
}

/// The game a request body names: 400 for an id that is bad or unknown here. `None` in, `None` out.
async fn check_game(s: &AppState, raw: Option<String>) -> Result<Option<Game>, Response> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let known = match s.parse_id(&raw) {
        Some(id) => s.game(&id).await,
        None => None,
    };
    known
        .map(Some)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "unknown game"))
}

async fn need_game(s: &AppState, raw: Option<String>) -> Result<Game, Response> {
    check_game(s, raw)
        .await?
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad request: id required"))
}

/// Mount API routes on `api` below; everything else falls through to the static site (site.rs).
/// The guard wraps all.
pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/games", get(games))
        .route("/games/{id}", axum::routing::delete(game_remove))
        .route("/games/{id}/info", get(game_info))
        .route(
            "/games/{id}/launch",
            get(game_launch_get)
                .put(game_launch_put)
                .delete(game_launch_delete)
                .layer(DefaultBodyLimit::max(4 << 10)),
        )
        .route(
            "/launch/defaults",
            get(launch_defaults_get)
                .put(launch_defaults_put)
                .layer(DefaultBodyLimit::max(4 << 10)),
        )
        .route("/games/{id}/achievements", get(game_achievements))
        .route("/games/{id}/ra-hash", get(game_ra_hash))
        .route("/games/{id}/news", get(game_news))
        .route("/games/{id}/local", get(game_local))
        .route("/games/{id}/saves", get(game_saves))
        .route(
            "/games/{id}/screenshots",
            get(shot_list)
                .post(shot_post)
                .layer(DefaultBodyLimit::max(crate::shots::MAX_BYTES)),
        )
        .route(
            "/games/{id}/screenshots/{name}",
            get(game_screenshot).delete(shot_delete),
        )
        .route("/games/{id}/art/{kind}", get(crate::art::serve))
        .route("/art/pending", get(crate::art::pending))
        .route(
            "/art/{id}/match",
            post(crate::art::answer).layer(DefaultBodyLimit::max(4 << 10)),
        )
        .route("/art/{id}/refresh", post(crate::art::refresh))
        .route("/art/{id}/candidates", get(crate::art::candidates))
        .route("/art/{id}/thumb/{kind}/{n}", get(crate::art::thumb))
        .route(
            "/art/{id}/pick",
            post(crate::art::pick).layer(DefaultBodyLimit::max(4 << 10)),
        )
        .route(
            "/emulators/dolphin/controls/{kind}",
            get(controls_get)
                .put(controls_put)
                .layer(DefaultBodyLimit::max(4 << 10)),
        )
        .route(
            "/emulators/dolphin/controls/{kind}/reset",
            post(controls_reset).layer(DefaultBodyLimit::max(4 << 10)),
        )
        .route("/library/scan", post(library_scan))
        .route(
            "/uploads",
            post(upload_create).layer(DefaultBodyLimit::max(4 << 10)),
        )
        .route(
            "/uploads/{id}",
            get(upload_status)
                .delete(upload_abort)
                // the raw body is not limited by axum; Uploads::put counts the bytes (32 MiB)
                .put(upload_put),
        )
        .route("/uploads/{id}/finish", post(upload_finish))
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
        // static beats the {id} capture in axum's router
        .route("/prefs/layout", put(layout_put).delete(layout_delete))
        .route("/prefs/{id}", put(prefs_put).delete(prefs_delete))
        .route(
            "/ids/remap",
            post(ids_remap).layer(DefaultBodyLimit::max(256 << 10)),
        )
        .route("/notes", get(notes_get))
        .route(
            "/notes/tags/{id}",
            put(tag_put)
                .delete(tag_delete)
                .layer(DefaultBodyLimit::max(16 << 10)),
        )
        .route(
            "/notes/games/{id}",
            put(game_note_put).layer(DefaultBodyLimit::max(64 << 10)),
        )
        .route("/machines", get(machines::list))
        .route("/machines/{id}/power", post(machines::power))
        .route("/m/{id}/{*path}", any(machines::proxy))
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
        if s.hub.is_some() && hub_has_no(req.uri().path()) {
            err(StatusCode::CONFLICT, "pick a machine")
        } else {
            next.run(req).await
        }
    } else {
        StatusCode::FORBIDDEN.into_response()
    };
    r.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        header::HeaderValue::from_static("nosniff"),
    );
    r
}

/// A hub has no Steam, library or emulators of its own; those live on the machines behind `/api/m/`.
/// `/api/ids/remap`, prefs, notes and the rest stay the hub's.
fn hub_has_no(path: &str) -> bool {
    let seg = path.strip_prefix("/api/").unwrap_or("");
    let seg = seg.split('/').next().unwrap_or("");
    matches!(
        seg,
        "games" | "friends" | "launch" | "library" | "uploads" | "art" | "emulators"
    )
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

async fn static_file(State(s): State<Arc<AppState>>, req: Request) -> Response {
    s.site.serve(req).await
}
