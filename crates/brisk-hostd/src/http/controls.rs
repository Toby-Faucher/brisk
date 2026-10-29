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

/// The gate and then `act`, together on the blocking pool (/proc scan, canonicalize, fsynced writes).
async fn controls_run(
    s: &Arc<AppState>,
    kind: String,
    act: impl FnOnce(&Controls, PadKind, bool) -> Result<crate::controls::View, CtlError>
    + Send
    + 'static,
) -> Response {
    let r = blocking(s, move |s| {
        controls_gate(s, &kind).map(|(k, run)| controls_reply(act(&s.controls, k, run)))
    })
    .await;
    match r {
        Ok(Ok(r)) | Ok(Err(r)) | Err(r) => r,
    }
}

pub(super) async fn controls_get(
    State(s): State<Arc<AppState>>,
    UrlPath(kind): UrlPath<String>,
) -> Response {
    controls_run(&s, kind, |c, k, run| c.get(k, run)).await
}

pub(super) async fn controls_put(
    State(s): State<Arc<AppState>>,
    UrlPath(kind): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    controls_run(&s, kind, move |c, k, run| c.put(k, &b, run)).await
}

pub(super) async fn controls_reset(
    State(s): State<Arc<AppState>>,
    UrlPath(kind): UrlPath<String>,
    _b: axum::body::Bytes,
) -> Response {
    controls_run(&s, kind, |c, k, run| c.reset(k, run)).await
}
