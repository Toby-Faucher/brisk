//! The Dolphin controller editor.

use super::*;

/// The editor needs a library game whose emulator has this editor (`registry` controller types);
/// and whether any such emulator runs (the config files are shared, so any running one owns them).
pub(super) fn controls_gate(s: &AppState, kind: &str) -> Result<(PadKind, bool), Response> {
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

pub(super) fn controls_reply(r: Result<crate::controls::View, CtlError>) -> Response {
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

pub(super) async fn controls_get(
    State(s): State<Arc<AppState>>,
    UrlPath(kind): UrlPath<String>,
) -> Response {
    match controls_gate(&s, &kind) {
        Ok((k, run)) => controls_reply(s.controls.get(k, run)),
        Err(r) => r,
    }
}

pub(super) async fn controls_put(
    State(s): State<Arc<AppState>>,
    UrlPath(kind): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    match controls_gate(&s, &kind) {
        Ok((k, run)) => controls_reply(s.controls.put(k, &b, run)),
        Err(r) => r,
    }
}

pub(super) async fn controls_reset(
    State(s): State<Arc<AppState>>,
    UrlPath(kind): UrlPath<String>,
    _b: axum::body::Bytes,
) -> Response {
    match controls_gate(&s, &kind) {
        Ok((k, run)) => controls_reply(s.controls.reset(k, run)),
        Err(r) => r,
    }
}
