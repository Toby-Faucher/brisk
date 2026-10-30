//! Host routes: status, Sunshine restart, clipboard and file drop.

use super::{AppState, err};
use crate::clipboard::{self, Clip, Kind};
use crate::host::RestartError;
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

pub(super) async fn host_status() -> Response {
    axum::Json(crate::host::host_info().await).into_response()
}

pub(super) async fn sunshine_restart(State(s): State<Arc<AppState>>) -> Response {
    let (code, msg) = match s.restarter.restart().await {
        Ok(()) => return StatusCode::NO_CONTENT.into_response(),
        Err(RestartError::TooSoon(secs)) => {
            return (
                [(header::RETRY_AFTER, secs.to_string())],
                err(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Sunshine was restarted a moment ago",
                ),
            )
                .into_response();
        }
        Err(RestartError::Failed) => (StatusCode::INTERNAL_SERVER_ERROR, "restart failed"),
    };
    err(code, msg)
}

/// No `since`: 204 with only the seq (the page starts from it without writing anything). Newer
/// text is JSON, a newer image the PNG itself; `x-brisk-seq` is on every answer.
pub(super) async fn clip_get(State(s): State<Arc<AppState>>, req: Request) -> Response {
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

/// `text/plain` (UTF-8) or `image/png` body becomes this machine's clipboard.
pub(super) async fn clip_post(State(s): State<Arc<AppState>>, req: Request) -> Response {
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
            "couldn't set this machine's clipboard",
        ),
    }
}

/// Reply `{path, size}`; see files.rs.
pub(super) async fn file_put(
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
