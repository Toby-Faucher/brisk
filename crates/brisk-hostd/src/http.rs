use crate::auth::{Hosts, tailnet_ok};
use crate::clipboard::{self, Clip, Clipboard, Kind, WlBackend};
use crate::config::Config;
use crate::controls::{Controls, CtlError, Kind as PadKind};
use crate::files::Files;
use crate::game::{Game, GameId};
use crate::host::{self, Clock, RealRunner, RestartError, Restarter, Runner, SystemClock};
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
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

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
pub struct ScanGate {
    running: bool,
    last: Option<std::time::Instant>,
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
    pub fn parse_id(&self, raw: &str) -> Option<GameId> {
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

/// The game a path names: 400 for text that is no id, 404 for a well-formed id (or a bare number)
/// this machine does not have.
#[allow(clippy::result_large_err)]
async fn path_game(s: &AppState, raw: &str) -> Result<Game, Response> {
    let id = match s.parse_id(raw) {
        Some(id) => id,
        None if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) => {
            return Err(err(StatusCode::NOT_FOUND, "unknown game"));
        }
        None => return Err(err(StatusCode::BAD_REQUEST, "bad game id")),
    };
    s.game(&id)
        .await
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown game"))
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
            .filter_map(|x| x.id.steam_appid())
            .collect(),
        g.games.iter().filter_map(|x| x.id.steam_appid()).collect(),
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
    UrlPath(raw): UrlPath<String>,
    req: Request,
) -> Response {
    let g = match path_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let Some(id) = g.id.steam_appid() else {
        return err(StatusCode::NOT_FOUND, "no Steam details for this game");
    };
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
    UrlPath(raw): UrlPath<String>,
    req: Request,
) -> Response {
    let g = match path_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let refresh = req
        .uri()
        .query()
        .is_some_and(|q| q.split('&').any(|p| p == "refresh=1"));
    let Some(id) = g.id.steam_appid() else {
        return emu_achievements(&s, &g, refresh).await;
    };
    axum::Json(steam_info::achievements(&s.cfg, id, refresh).await).into_response()
}

/// RetroAchievements progress for an emulator game, always a 200 whose `state` says what the
/// panel should show (see `ra_game`). `refresh` skips the 5 minute cache, at most once per 30 s a game.
async fn emu_achievements(s: &AppState, g: &Game, refresh: bool) -> Response {
    let recs = s.records();
    let Some(r) = crate::emu::find(&recs, &g.id) else {
        return err(StatusCode::NOT_FOUND, "unknown game");
    };
    let fresh = refresh && {
        let mut m = s.ra_refresh.lock().unwrap();
        let now = std::time::Instant::now();
        let ok = m
            .get(g.id.as_str())
            .is_none_or(|t| now.duration_since(*t) >= std::time::Duration::from_secs(30));
        if ok {
            m.insert(g.id.to_string(), now);
        }
        ok
    };
    let body = crate::ra_game::state_json(&s.cfg, &r.file, fresh).await;
    ([(header::CACHE_CONTROL, "no-store")], axum::Json(body)).into_response()
}

/// Debug: the RetroAchievements hash brisk computes for an emulator game, as
/// `{console, hash, container, computedAt}`, so it can be looked up on retroachievements.org by
/// hand. The Wii routine is verified only against a synthetic image, not a real disc.
/// 422 `{error, reason}` for a container it cannot hash.
async fn game_ra_hash(State(s): State<Arc<AppState>>, UrlPath(raw): UrlPath<String>) -> Response {
    let g = match path_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let recs = s.records();
    let Some(r) = crate::emu::find(&recs, &g.id) else {
        return err(StatusCode::NOT_FOUND, "no disc hash for this game");
    };
    let res = match crate::ra_game::hash_blocking(&s.cfg, &r.file).await {
        Ok(h) => axum::Json(h).into_response(),
        Err(crate::ra_game::HashErr::Unsupported(why)) => {
            err(StatusCode::UNPROCESSABLE_ENTITY, &why)
        }
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "game file unreadable"),
    };
    ([(header::CACHE_CONTROL, "no-store")], res).into_response()
}

/// Latest headlines for the detail panel; 404 for an unknown game before any upstream call. An
/// upstream failure is a 200 with an empty list.
async fn game_news(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
    req: Request,
) -> Response {
    let g = match path_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let Some(id) = g.id.steam_appid() else {
        return err(StatusCode::NOT_FOUND, "no Steam details for this game");
    };
    let refresh = req
        .uri()
        .query()
        .is_some_and(|q| q.split('&').any(|p| p == "refresh=1"));
    axum::Json(steam_info::news(&s.cfg, id, refresh).await).into_response()
}

/// Screenshot names and a cloud-save summary from the host's Steam folder; 404 for an unknown game.
/// No folder is a 200 with empty fields.
async fn game_local(State(s): State<Arc<AppState>>, UrlPath(raw): UrlPath<String>) -> Response {
    let id = match path_game(&s, &raw).await {
        Ok(g) => g.appid,
        Err(r) => return r,
    };
    let s2 = s.clone();
    match tokio::task::spawn_blocking(move || steam_info::local(&s2.cfg, &s2.shots, id)).await {
        Ok(l) => axum::Json(l).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "listing failed"),
    }
}

/// Saves and save states of an emulator game, read-only. 404 for Steam games and games without the
/// `saves` cap; a missing user dir is a 200 with `available: false`.
async fn game_saves(State(s): State<Arc<AppState>>, UrlPath(raw): UrlPath<String>) -> Response {
    let g = match path_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    if !g.caps.saves {
        return err(StatusCode::NOT_FOUND, "no saves for this game");
    }
    let recs = s.records();
    let (disc, system) = (
        crate::emu::find(&recs, &g.id).and_then(crate::emu::disc_id_of),
        g.system.clone().unwrap_or_default(),
    );
    let dir = s.cfg.dolphin_user_dir.clone();
    let found = tokio::task::spawn_blocking(move || {
        disc.map_or(Some(vec![]), |d| crate::saves::list(&dir, &d, &system))
    })
    .await;
    let Ok(found) = found else {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "listing failed");
    };
    let body = serde_json::json!({
        "available": found.is_some(),
        "userDir": "Dolphin user folder",
        "items": found.unwrap_or_default(),
    });
    ([(header::CACHE_CONTROL, "no-store")], axum::Json(body)).into_response()
}

/// One screenshot file, only if `name` is in the game's listing (see `steam_info::screenshot`).
async fn game_screenshot(
    State(s): State<Arc<AppState>>,
    UrlPath((raw, name)): UrlPath<(String, String)>,
) -> Response {
    let id = match path_game(&s, &raw).await {
        Ok(g) => g.appid,
        Err(r) => return r,
    };
    let s2 = s.clone();
    match tokio::task::spawn_blocking(move || steam_info::screenshot(&s2.cfg, &s2.shots, id, &name))
        .await
    {
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

pub(crate) fn err(code: StatusCode, msg: &str) -> Response {
    (code, axum::Json(serde_json::json!({ "error": msg }))).into_response()
}

/// JSON body or 400 (axum's `Json` would answer 422). `appid` is a u32 here or the request fails.
#[allow(clippy::result_large_err)]
fn parse<T: serde::de::DeserializeOwned>(b: &[u8]) -> Result<T, Response> {
    serde_json::from_slice(b)
        .map_err(|e| err(StatusCode::BAD_REQUEST, &format!("bad request: {e}")))
}

/// The id text of a request: `id` (text or number) wins over the old numeric `appid`.
fn raw_id(id: Option<RawId>, appid: Option<u32>) -> Option<String> {
    id.map(|i| i.text()).or(appid.map(|n| n.to_string()))
}

/// The game a request body names: 400 for an id that is bad or unknown here. `None` in, `None` out.
#[allow(clippy::result_large_err)]
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

#[allow(clippy::result_large_err)]
async fn need_game(s: &AppState, raw: Option<String>) -> Result<Game, Response> {
    check_game(s, raw)
        .await?
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad request: id required"))
}

async fn stream_get(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(s.stream.info()).into_response()
}

async fn stream_start(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    let run = async {
        let req: StreamReq = parse(&b)?;
        req.flags().map_err(|e| err(StatusCode::BAD_REQUEST, &e))?;
        let game = check_game(&s, raw_id(req.id.clone(), req.appid)).await?;
        // Own task: a client that hangs up must not abort a start halfway through a stop.
        let s2 = s.clone();
        tokio::spawn(async move { s2.stream.start(req, game).await })
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
struct GameReq {
    appid: Option<u32>,
    id: Option<RawId>,
    #[serde(default)]
    force: bool,
}

/// Starts or installs a known game through its source, detached: 204 once it runs, 400, or 500
/// if it could not start. Install is 400 for a game whose source has nothing to install.
async fn game_cmd(s: Arc<AppState>, b: &[u8], install: bool) -> Response {
    let g = match parse::<GameReq>(b) {
        Ok(r) => match need_game(&s, raw_id(r.id, r.appid)).await {
            Ok(g) => g,
            Err(r) => return r,
        },
        Err(r) => return r,
    };
    if install && !g.caps.install {
        return err(StatusCode::BAD_REQUEST, "nothing to install for this game");
    }
    let s2 = s.clone();
    let id = g.id;
    let ok = tokio::task::spawn_blocking(move || {
        if install {
            s2.stream.install(&id) == Some(true)
        } else {
            s2.stream.launch(&id)
        }
    })
    .await;
    match (ok, g.source) {
        (Ok(true), _) => StatusCode::NO_CONTENT.into_response(),
        (_, crate::game::Source::Emu) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the emulator didn't start (is the game file there?)",
        ),
        _ => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "steam didn't take the command",
        ),
    }
}

async fn launch(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    game_cmd(s, &b, false).await
}

async fn install(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    game_cmd(s, &b, true).await
}

/// `{state: "closed" | "still-running"}`; 400 for an unknown game, 409 if it is not running or
/// already being quit. Answers within ~25 s; a longer quit carries on and reports still-running.
async fn quit(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    let (g, force) = match parse::<GameReq>(&b) {
        Ok(q) => match need_game(&s, raw_id(q.id, q.appid)).await {
            Ok(g) => (g, q.force),
            Err(r) => return r,
        },
        Err(r) => return r,
    };
    // Own task: a client that hangs up must not stop a quit between its steps.
    let s2 = s.clone();
    let job = tokio::spawn(async move { s2.quit.quit(&g.id, force).await });
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
    axum::Json(serde_json::json!({
        "prefs": s.prefs.all(),
        "homeOrder": s.prefs.home_order(),
        "machinesOrder": s.prefs.machines_order(),
        "machineColors": s.prefs.machine_colors(),
    }))
    .into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LayoutBody {
    home_order: Option<Vec<String>>,
    machines_order: Option<Vec<String>>,
    machine_colors: Option<std::collections::BTreeMap<String, Option<String>>>,
}

/// The Home card and Machines page orders plus the machine colours: at least one key, orders
/// arrays of strings (else 400); bad, duplicate and surplus ids are dropped (prefs::clean_ids),
/// a key not sent keeps its value. `machineColors` is merged (null removes an id) and a bad id,
/// bad colour or more than 64 entries is a 400 that stores nothing. All three are returned.
async fn layout_put(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    match parse::<LayoutBody>(&b) {
        Ok(b)
            if b.home_order.is_none()
                && b.machines_order.is_none()
                && b.machine_colors.is_none() =>
        {
            err(
                StatusCode::BAD_REQUEST,
                "homeOrder, machinesOrder or machineColors required",
            )
        }
        Ok(b) => match s
            .prefs
            .set_layout(b.home_order, b.machines_order, b.machine_colors)
        {
            Ok((home, machines, colors)) => axum::Json(serde_json::json!({
                "homeOrder": home, "machinesOrder": machines, "machineColors": colors
            }))
            .into_response(),
            Err(e) => err(StatusCode::BAD_REQUEST, e),
        },
        Err(r) => r,
    }
}

/// Reset layout: forget the saved orders and colours (Home and Machines fall back to their defaults).
async fn layout_delete(State(s): State<Arc<AppState>>) -> Response {
    let all = s
        .prefs
        .machine_colors()
        .into_keys()
        .map(|k| (k, None))
        .collect();
    let _ = s.prefs.set_layout(Some(vec![]), Some(vec![]), Some(all));
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
    UrlPath(raw): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    let Some(id) = s.parse_id(&raw) else {
        return err(StatusCode::BAD_REQUEST, "bad game id");
    };
    if id
        .steam_appid()
        .is_some_and(|n| n == 0 || n == s.cfg.app_id)
    {
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
        id: None,
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
    axum::Json(s.prefs.put(id.as_str(), e)).into_response()
}

async fn prefs_delete(State(s): State<Arc<AppState>>, UrlPath(raw): UrlPath<String>) -> Response {
    match s.parse_id(&raw) {
        Some(id) => {
            s.prefs.delete(id.as_str());
            StatusCode::NO_CONTENT.into_response()
        }
        None => err(StatusCode::BAD_REQUEST, "bad game id"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemapBody {
    map: std::collections::BTreeMap<String, String>,
}

/// `{map: {"<old number>": "<new id>"}}`: rekeys notes and prefs entries of emulator games that
/// startup could not map (the hub has no library of its own; the web client knows `legacyId` and
/// `id` from each machine's games). Keys must be old emulator numbers, values new emulator ids;
/// at most 2000 pairs, else 400. Answers `{notes, prefs}` counts. Safe to repeat.
async fn ids_remap(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    let m = match parse::<RemapBody>(&b) {
        Ok(m) => m.map,
        Err(r) => return r,
    };
    let ok = |(k, v): (&String, &String)| {
        k.parse::<u32>()
            .is_ok_and(|n| n >= crate::game::LEGACY_EMU_BASE)
            && crate::game::GameId::parse(v).is_some_and(|g| g.emu_parts().is_some())
    };
    if m.len() > 2000 || !m.iter().all(ok) {
        return err(
            StatusCode::BAD_REQUEST,
            "map wants old emulator numbers and new emu ids",
        );
    }
    // A machine with a library moves only to games it has, and never from a number two of its
    // games share. The hub has no library and takes the web client's pairs as sent.
    let recs = s.records();
    let m: std::collections::BTreeMap<_, _> = if recs.is_empty() {
        m
    } else {
        m.into_iter()
            .filter(|(k, v)| {
                let n = k.parse::<u32>().ok();
                crate::game::GameId::parse(v).is_some_and(|g| recs.iter().any(|r| r.is(&g)))
                    && recs.iter().filter(|r| Some(r.legacy()) == n).count() <= 1
            })
            .collect()
    };
    let notes = match s.notes.rekey(&m) {
        Ok(n) => n,
        Err(e) => return notes_err(e),
    };
    axum::Json(serde_json::json!({ "notes": notes, "prefs": s.prefs.rekey(&m) })).into_response()
}

fn notes_err(e: NotesError) -> Response {
    match e {
        NotesError::Bad(m) => err(StatusCode::BAD_REQUEST, m),
        NotesError::Full => err(StatusCode::CONFLICT, "too many notes or tags"),
        NotesError::Io => err(StatusCode::INTERNAL_SERVER_ERROR, "could not save notes"),
    }
}

async fn notes_get(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(s.notes.all()).into_response()
}

/// Creates or replaces one tag; answers with what was stored (name trimmed).
async fn tag_put(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    match parse::<Tag>(&b).map(|t| s.notes.put_tag(&id, t)) {
        Ok(Ok(t)) => axum::Json(t).into_response(),
        Ok(Err(e)) => notes_err(e),
        Err(r) => r,
    }
}

/// Removes the tag and takes it off every game; an unknown id is fine (204 either way).
async fn tag_delete(State(s): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    match s.notes.delete_tag(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => notes_err(e),
    }
}

/// One game's note and tags; answers with what was stored (unknown tag ids dropped).
async fn game_note_put(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    let Some(id) = s.parse_id(&raw) else {
        return err(StatusCode::BAD_REQUEST, "bad game id");
    };
    match parse::<GameNote>(&b).map(|g| s.notes.put_game(id.as_str(), g)) {
        Ok(Ok(g)) => axum::Json(g).into_response(),
        Ok(Err(e)) => notes_err(e),
        Err(r) => r,
    }
}

/// The editor needs a library game whose emulator has this editor (`registry` controller types);
/// and whether any such emulator runs (the config files are shared, so any running one owns them).
#[allow(clippy::result_large_err)]
fn controls_gate(s: &AppState, kind: &str) -> Result<(PadKind, bool), Response> {
    let Some(kind) = PadKind::parse(kind) else {
        return Err(err(StatusCode::NOT_FOUND, "no such controller type"));
    };
    let recs = s.records();
    let mut editable = recs
        .iter()
        .filter(|r| crate::emu::caps(&r.emulator).controls)
        .peekable();
    if editable.peek().is_none() {
        return Err(err(StatusCode::NOT_FOUND, "no controls on this machine"));
    }
    let ps = crate::running::procs(&s.cfg.proc_dir);
    let running = editable.any(|r| !crate::emu::pids(&ps, r).is_empty());
    Ok((kind, running))
}

fn controls_reply(r: Result<crate::controls::View, CtlError>) -> Response {
    match r {
        Ok(v) => axum::Json(v).into_response(),
        Err(CtlError::Bad(m)) => err(StatusCode::BAD_REQUEST, m),
        Err(CtlError::Changed(cur)) => (
            StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "error": "changed elsewhere", "current": cur })),
        )
            .into_response(),
        Err(CtlError::Io) => err(StatusCode::INTERNAL_SERVER_ERROR, "could not save controls"),
    }
}

async fn controls_get(State(s): State<Arc<AppState>>, UrlPath(kind): UrlPath<String>) -> Response {
    match controls_gate(&s, &kind) {
        Ok((k, run)) => controls_reply(s.controls.get(k, run)),
        Err(r) => r,
    }
}

async fn controls_put(
    State(s): State<Arc<AppState>>,
    UrlPath(kind): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    match controls_gate(&s, &kind) {
        Ok((k, run)) => controls_reply(s.controls.put(k, &b, run)),
        Err(r) => r,
    }
}

async fn controls_reset(
    State(s): State<Arc<AppState>>,
    UrlPath(kind): UrlPath<String>,
    _b: axum::body::Bytes,
) -> Response {
    match controls_gate(&s, &kind) {
        Ok((k, run)) => controls_reply(s.controls.reset(k, run)),
        Err(r) => r,
    }
}

/// Frees the scan slot however the request ends.
struct ScanSlot<'a>(&'a Mutex<ScanGate>);
impl Drop for ScanSlot<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap().running = false;
    }
}

/// The cards for `ids`, as `/api/games` would show them.
fn cards(s: &AppState, ids: &[GameId]) -> Vec<Game> {
    let recs = s.records();
    crate::emu::games(&recs, &s.cfg.proc_dir, &s.cfg.data_dir)
        .into_iter()
        .filter(|g| ids.contains(&g.id))
        .collect()
}

/// POST /api/library/scan: registers the games found in the games folder.
async fn library_scan(
    State(s): State<Arc<AppState>>,
    axum::extract::RawQuery(q): axum::extract::RawQuery,
) -> Response {
    let include_removed = q.is_some_and(|q| q.split('&').any(|p| p == "include_removed=1"));
    {
        let mut g = s.scan_gate.lock().unwrap();
        if g.running {
            return err(StatusCode::CONFLICT, "a scan is already running");
        }
        if let Some(t) = g.last.filter(|t| t.elapsed() < s.scan_gap) {
            let wait = (s.scan_gap - t.elapsed()).as_secs() + 1;
            let mut r = err(StatusCode::TOO_MANY_REQUESTS, "scanned a moment ago");
            r.headers_mut()
                .insert(header::RETRY_AFTER, header::HeaderValue::from(wait));
            return r;
        }
        g.running = true;
        g.last = Some(std::time::Instant::now());
    }
    let _slot = ScanSlot(&s.scan_gate);
    let cfg = s.cfg.clone();
    match tokio::task::spawn_blocking(move || {
        crate::ingest::scan_library_with(&cfg, include_removed)
    })
    .await
    {
        Ok(Ok(r)) => {
            s.art.trigger_new(&s.cfg, &r.merged.added);
            axum::Json(serde_json::json!({
                "scanned": r.scan.scanned,
                "added": cards(&s, &r.merged.added),
                "aliased": r.merged.aliased,
                "duplicates": r.merged.duplicates,
                "unplayable": r.merged.unplayable,
                "removed": r.merged.removed,
                "unknown": r.scan.unknown,
                "broken": r.scan.broken,
                "truncated": r.scan.truncated,
            }))
            .into_response()
        }
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "scan task failed"),
    }
}

/// DELETE /api/games/{id}: forgets an emulator game's record; the file stays.
async fn game_remove(State(s): State<Arc<AppState>>, UrlPath(raw): UrlPath<String>) -> Response {
    let Some(id) = s.parse_id(&raw) else {
        // a bare old number this library no longer knows was forgotten already
        return if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
            StatusCode::NO_CONTENT.into_response()
        } else {
            err(StatusCode::BAD_REQUEST, "bad game id")
        };
    };
    if id.source() != crate::game::Source::Emu {
        return err(
            StatusCode::BAD_REQUEST,
            "not supported: only emulator games can be removed",
        );
    }
    // forget waits for the library lock, which a long scan holds
    let dir = s.cfg.data_dir.clone();
    match tokio::task::spawn_blocking(move || crate::ingest::forget(&dir, &id)).await {
        Ok(Ok(_)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "remove task failed"),
    }
}

fn up_reply(r: Result<serde_json::Value, crate::uploads::Fail>, ok: StatusCode) -> Response {
    match r {
        Ok(v) => (ok, axum::Json(v)).into_response(),
        Err((c, v)) => (c, axum::Json(v)).into_response(),
    }
}

async fn upload_create(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    #[derive(Deserialize)]
    struct B {
        name: String,
        size: u64,
    }
    match parse::<B>(&b) {
        Ok(b) => up_reply(s.uploads.create(&b.name, b.size), StatusCode::CREATED),
        Err(r) => r,
    }
}

async fn upload_status(State(s): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    up_reply(s.uploads.status(&id), StatusCode::OK)
}

async fn upload_put(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
    req: Request,
) -> Response {
    let h = req.headers();
    let len = h
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok()?.parse().ok());
    let off = h
        .get("x-offset")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    up_reply(
        s.uploads
            .put(&id, off.as_deref(), len, req.into_body())
            .await,
        StatusCode::OK,
    )
}

async fn upload_abort(State(s): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    s.uploads.abort(&id);
    StatusCode::NO_CONTENT.into_response()
}

async fn upload_finish(State(s): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    match s.uploads.finish(&s.cfg.data_dir, &id).await {
        Ok(f) => {
            if let Some(g) = &f.game {
                s.art.trigger_new(&s.cfg, std::slice::from_ref(g));
            }
            let game = f
                .game
                .as_ref()
                .and_then(|g| cards(&s, std::slice::from_ref(g)).pop());
            axum::Json(serde_json::json!({
                "system": f.system,
                "registered": f.game.is_some(),
                "duplicate": f.duplicate,
                "game": game,
            }))
            .into_response()
        }
        Err((c, v)) => (c, axum::Json(v)).into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DefaultsPut {
    rev: Option<String>,
    allow_custom_commands: bool,
    options: LaunchOptions,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverridePut {
    rev: Option<String>,
    #[serde(rename = "override")]
    over: LaunchOverride,
}

fn defaults_view(d: &launchstore::Data) -> serde_json::Value {
    serde_json::json!({
        "rev": launchstore::defaults_rev(d),
        "allowCustomCommands": d.allow_custom_commands,
        "options": d.defaults,
        "envAllowed": crate::launch::ENV_ALLOWED,
    })
}

/// The paste string for a Steam game's launch option; the program is where this hostd runs from.
fn steam_line(id: &GameId) -> Option<String> {
    id.steam_appid()?;
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| "brisk-hostd".into());
    let exe = if exe
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/._-+".contains(&b))
    {
        exe
    } else {
        format!("'{}'", exe.replace('\'', "'\\''"))
    };
    Some(format!("{exe} wrap {id} -- %command%"))
}

fn game_launch_view(d: &launchstore::Data, id: &GameId) -> serde_json::Value {
    let eff = d.effective(id.as_str());
    let problems: Vec<String> = match crate::launch::validate(&eff, d.allow_custom_commands) {
        Ok(_) => vec![],
        Err(p) => p.iter().map(launchstore::describe).collect(),
    };
    serde_json::json!({
        "id": id,
        "rev": launchstore::game_rev(d, id.as_str()),
        "override": d.games.get(id.as_str()),
        "effective": eff,
        "problems": problems,
        "allowCustomCommands": d.allow_custom_commands,
        "steamLine": steam_line(id),
    })
}

fn launch_reply(
    r: Result<launchstore::Data, StoreError>,
    view: impl Fn(&launchstore::Data) -> serde_json::Value,
) -> Response {
    match r {
        Ok(d) => axum::Json(view(&d)).into_response(),
        Err(StoreError::Bad(p)) => (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": "these launch options are not allowed", "problems": p })),
        )
            .into_response(),
        Err(StoreError::Changed) => (
            StatusCode::CONFLICT,
            axum::Json(serde_json::json!({ "error": "changed elsewhere" })),
        )
            .into_response(),
        Err(StoreError::Full) => err(StatusCode::CONFLICT, "too many games have launch overrides"),
        Err(StoreError::Io) => err(StatusCode::INTERNAL_SERVER_ERROR, "could not save launch options"),
    }
}

/// A conflict also carries the current state, so the page can show what changed.
fn with_current(r: Response, cur: serde_json::Value) -> Response {
    if r.status() != StatusCode::CONFLICT || cur.is_null() {
        return r;
    }
    (
        StatusCode::CONFLICT,
        axum::Json(serde_json::json!({ "error": "changed elsewhere", "current": cur })),
    )
        .into_response()
}

async fn launch_defaults_get(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(defaults_view(&s.launch.all())).into_response()
}

async fn launch_defaults_put(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    let p = match parse::<DefaultsPut>(&b) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let r = launch_reply(
        s.launch
            .put_defaults(p.rev.as_deref(), p.allow_custom_commands, p.options),
        defaults_view,
    );
    with_current(r, defaults_view(&s.launch.all()))
}

#[allow(clippy::result_large_err)]
fn launch_id(s: &AppState, raw: &str) -> Result<GameId, Response> {
    s.parse_id(raw)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad game id"))
}

async fn game_launch_get(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
    match launch_id(&s, &raw) {
        Ok(id) => axum::Json(game_launch_view(&s.launch.all(), &id)).into_response(),
        Err(r) => r,
    }
}

async fn game_launch_put(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    let id = match launch_id(&s, &raw) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let p = match parse::<OverridePut>(&b) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let r = launch_reply(
        s.launch.put_game(id.as_str(), p.rev.as_deref(), p.over),
        |d| game_launch_view(d, &id),
    );
    with_current(r, game_launch_view(&s.launch.all(), &id))
}

async fn game_launch_delete(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
    match launch_id(&s, &raw) {
        Ok(id) => launch_reply(s.launch.delete_game(id.as_str()), |d| {
            game_launch_view(d, &id)
        }),
        Err(r) => r,
    }
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
        .route("/games/{id}/screenshots/{name}", get(game_screenshot))
        .route("/games/{id}/art/{kind}", get(crate::art::serve))
        .route("/art/pending", get(crate::art::pending))
        .route(
            "/art/{id}/match",
            post(crate::art::answer).layer(DefaultBodyLimit::max(4 << 10)),
        )
        .route("/art/{id}/refresh", post(crate::art::refresh))
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
