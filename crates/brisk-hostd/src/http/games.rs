//! Game routes: the library, details, screenshots, launch, quit, scan and uploads.

use super::*;

/// The game a path names: 400 for text that is no id, 404 for a well-formed id (or a bare number)
/// this machine does not have.
pub(super) async fn path_game(s: &AppState, raw: &str) -> Result<Game, Response> {
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

pub(super) async fn games(State(s): State<Arc<AppState>>, req: Request) -> Response {
    let refresh = query_flag(req.uri().query(), "refresh=1");
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
            tracing::error!("games: serializing the cache failed: {e}");
            err(StatusCode::INTERNAL_SERVER_ERROR, "games unavailable")
        }
    }
}

/// Store details for the detail panel; 404 for a game that is neither owned nor installed, so
/// no upstream call is made for it. A Store failure is a 200 with `available: false`.
pub(super) async fn game_info(
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
    let refresh = query_flag(req.uri().query(), "refresh=1");
    axum::Json(steam_info::info(&s.cfg, id, refresh).await).into_response()
}

/// Achievements for the detail panel; 404 for an unknown game before any upstream call. A game
/// without stats or a private profile is a 200 with `available: false`.
pub(super) async fn game_achievements(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
    req: Request,
) -> Response {
    let g = match path_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let refresh = query_flag(req.uri().query(), "refresh=1");
    let Some(id) = g.id.steam_appid() else {
        return emu_achievements(&s, &g, refresh).await;
    };
    axum::Json(steam_info::achievements(&s.cfg, id, refresh).await).into_response()
}

/// RetroAchievements progress for an emulator game, always a 200 whose `state` says what the
/// panel should show (see `ra_game`). `refresh` skips the 5 minute cache, at most once per 30 s a game.
pub(super) async fn emu_achievements(s: &AppState, g: &Game, refresh: bool) -> Response {
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
    let body =
        crate::ra_game::state_json(&s.cfg, &r.file, std::slice::from_ref(&r.name), fresh).await;
    ([(header::CACHE_CONTROL, "no-store")], axum::Json(body)).into_response()
}

/// Debug: the RetroAchievements hash brisk computes for an emulator game, as
/// `{console, hash, container, computedAt}`, so it can be looked up on retroachievements.org by
/// hand. The Wii routine is verified only against a synthetic image, not a real disc.
/// 422 `{error, reason}` for a container it cannot hash.
pub(super) async fn game_ra_hash(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
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
pub(super) async fn game_news(
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
    let refresh = query_flag(req.uri().query(), "refresh=1");
    axum::Json(steam_info::news(&s.cfg, id, refresh).await).into_response()
}

/// Screenshot names and a cloud-save summary from the host's Steam folder; 404 for an unknown game.
/// No folder is a 200 with empty fields.
pub(super) async fn game_local(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
    let g = match path_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    if g.source == crate::game::Source::Emu {
        let (d, id) = (s.cfg.data_dir.clone(), g.id.as_str().to_string());
        let names = tokio::task::spawn_blocking(move || crate::shots::list_names(&d, &id)).await;
        let names = names.unwrap_or_default();
        return axum::Json(serde_json::json!({ "screenshots": names, "saves": null }))
            .into_response();
    }
    let id = g.appid;
    let s2 = s.clone();
    match tokio::task::spawn_blocking(move || steam_info::local(&s2.cfg, &s2.shots, id)).await {
        Ok(l) => axum::Json(l).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "listing failed"),
    }
}

/// Saves and save states of an emulator game, read-only. 404 for Steam games and games without the
/// `saves` cap; a missing user dir is a 200 with `available: false`.
pub(super) async fn game_saves(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
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
pub(super) async fn game_screenshot(
    State(s): State<Arc<AppState>>,
    UrlPath((raw, name)): UrlPath<(String, String)>,
) -> Response {
    let g = match path_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    if g.source == crate::game::Source::Emu {
        return shot_get(&s, &g, name).await;
    }
    let id = g.appid;
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

pub(super) const NO_SHOTS: &str = "no stored screenshots for this game";

/// An emulator game that takes screenshots, else the error to send.
pub(super) async fn shot_game(s: &AppState, raw: &str) -> Result<Game, Response> {
    let g = path_game(s, raw).await?;
    if g.source == crate::game::Source::Emu && g.caps.screenshots {
        Ok(g)
    } else {
        Err(err(StatusCode::NOT_FOUND, NO_SHOTS))
    }
}

pub(super) async fn shot_get(s: &AppState, g: &Game, name: String) -> Response {
    let (d, id) = (s.cfg.data_dir.clone(), g.id.as_str().to_string());
    match tokio::task::spawn_blocking(move || crate::shots::read(&d, &id, &name)).await {
        Ok(Some(body)) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::CACHE_CONTROL, "private, max-age=3600"),
            ],
            body,
        )
            .into_response(),
        _ => err(StatusCode::NOT_FOUND, "no such screenshot"),
    }
}

/// The raw PNG as the body (8 MiB cap, counted while it streams in). 201 with the stored entry.
pub(super) async fn shot_post(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
    body: axum::body::Bytes,
) -> Response {
    let g = match shot_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let (d, id) = (s.cfg.data_dir.clone(), g.id.as_str().to_string());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_millis() as u64);
    let r = tokio::task::spawn_blocking(move || {
        let free = crate::files::disk_free(&d);
        crate::shots::save(&d, &id, &body, now, free)
    })
    .await;
    match r {
        Ok(Ok(shot)) => (StatusCode::CREATED, axum::Json(shot)).into_response(),
        Ok(Err(crate::shots::Fail::Invalid(m))) => err(StatusCode::UNPROCESSABLE_ENTITY, m),
        Ok(Err(crate::shots::Fail::NoSpace)) => {
            err(StatusCode::INSUFFICIENT_STORAGE, "the disk is full")
        }
        _ => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not store the screenshot",
        ),
    }
}

pub(super) async fn shot_list(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
    let g = match shot_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let (d, id) = (s.cfg.data_dir.clone(), g.id.as_str().to_string());
    let l = tokio::task::spawn_blocking(move || crate::shots::list(&d, &id)).await;
    (
        [(header::CACHE_CONTROL, "no-store")],
        axum::Json(l.unwrap_or_default()),
    )
        .into_response()
}

pub(super) async fn shot_delete(
    State(s): State<Arc<AppState>>,
    UrlPath((raw, name)): UrlPath<(String, String)>,
) -> Response {
    let g = match shot_game(&s, &raw).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let (d, id) = (s.cfg.data_dir.clone(), g.id.as_str().to_string());
    match tokio::task::spawn_blocking(move || crate::shots::delete(&d, &id, &name)).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        _ => err(StatusCode::NOT_FOUND, "no such screenshot"),
    }
}

/// Friends in a game or online. Any failure is a 200 with `available: false`.
pub(super) async fn friends(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(steam_info::friends_cached(&s).await).into_response()
}

#[derive(Deserialize)]
pub(super) struct GameReq {
    appid: Option<u32>,
    id: Option<RawId>,
    #[serde(default)]
    force: bool,
}

/// Starts or installs a known game through its source, detached: 204 once it runs, 400, or 500
/// if it could not start. Install is 400 for a game whose source has nothing to install.
pub(super) async fn game_cmd(s: Arc<AppState>, b: &[u8], install: bool) -> Response {
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

pub(super) async fn launch(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    game_cmd(s, &b, false).await
}

pub(super) async fn install(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
    game_cmd(s, &b, true).await
}

/// `{state: "closed" | "still-running"}`; 400 for an unknown game, 409 if it is not running or
/// already being quit. Answers within ~25 s; a longer quit carries on and reports still-running.
pub(super) async fn quit(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
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

/// Frees the scan slot however the request ends.
pub(super) struct ScanSlot<'a>(&'a Mutex<ScanGate>);
impl Drop for ScanSlot<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap().running = false;
    }
}

/// The cards for `ids`, as `/api/games` would show them.
pub(super) fn cards(s: &AppState, ids: &[GameId]) -> Vec<Game> {
    let recs = s.records();
    crate::emu::games(&recs, &s.cfg.proc_dir, &s.cfg.data_dir)
        .into_iter()
        .filter(|g| ids.contains(&g.id))
        .collect()
}

/// POST /api/library/scan: registers the games found in the games folder.
pub(super) async fn library_scan(
    State(s): State<Arc<AppState>>,
    axum::extract::RawQuery(q): axum::extract::RawQuery,
) -> Response {
    let include_removed = query_flag(q.as_deref(), "include_removed=1");
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
pub(super) async fn game_remove(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
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

pub(super) fn up_reply(
    r: Result<serde_json::Value, crate::uploads::Fail>,
    ok: StatusCode,
) -> Response {
    match r {
        Ok(v) => (ok, axum::Json(v)).into_response(),
        Err((c, v)) => (c, axum::Json(v)).into_response(),
    }
}

pub(super) async fn upload_create(
    State(s): State<Arc<AppState>>,
    b: axum::body::Bytes,
) -> Response {
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

pub(super) async fn upload_status(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    up_reply(s.uploads.status(&id), StatusCode::OK)
}

pub(super) async fn upload_put(
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

pub(super) async fn upload_abort(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    s.uploads.abort(&id);
    StatusCode::NO_CONTENT.into_response()
}

pub(super) async fn upload_finish(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
) -> Response {
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
