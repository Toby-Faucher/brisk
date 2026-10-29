//! Launch option defaults and per-game overrides.

use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct DefaultsPut {
    rev: Option<String>,
    allow_custom_commands: bool,
    options: LaunchOptions,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OverridePut {
    rev: Option<String>,
    #[serde(rename = "override")]
    over: LaunchOverride,
}

pub(super) fn defaults_view(d: &launchstore::Data) -> serde_json::Value {
    serde_json::json!({
        "rev": launchstore::defaults_rev(d),
        "allowCustomCommands": d.allow_custom_commands,
        "options": d.defaults,
        "envAllowed": crate::launch::ENV_ALLOWED,
    })
}

/// The paste string for a Steam game's launch option; the program is where this hostd runs from.
pub(super) fn steam_line(id: &GameId) -> Option<String> {
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

pub(super) fn game_launch_view(d: &launchstore::Data, id: &GameId) -> serde_json::Value {
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

pub(super) fn launch_reply(
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
        Err(StoreError::Changed) => err(StatusCode::CONFLICT, "changed elsewhere"),
        Err(StoreError::Full) => err(StatusCode::CONFLICT, "too many games have launch overrides"),
        Err(StoreError::Io) => err(StatusCode::INTERNAL_SERVER_ERROR, "could not save launch options"),
    }
}

/// A conflict also carries the current state, so the page can show what changed.
pub(super) fn with_current(r: Response, cur: serde_json::Value) -> Response {
    if r.status() != StatusCode::CONFLICT || cur.is_null() {
        return r;
    }
    (
        StatusCode::CONFLICT,
        axum::Json(serde_json::json!({ "error": "changed elsewhere", "current": cur })),
    )
        .into_response()
}

pub(super) async fn launch_defaults_get(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(defaults_view(&s.launch.all())).into_response()
}

pub(super) async fn launch_defaults_put(
    State(s): State<Arc<AppState>>,
    b: axum::body::Bytes,
) -> Response {
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

pub(super) fn launch_id(s: &AppState, raw: &str) -> Result<GameId, Response> {
    s.parse_id(raw)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad game id"))
}

pub(super) async fn game_launch_get(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
    match launch_id(&s, &raw) {
        Ok(id) => axum::Json(game_launch_view(&s.launch.all(), &id)).into_response(),
        Err(r) => r,
    }
}

pub(super) async fn game_launch_put(
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

pub(super) async fn game_launch_delete(
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
