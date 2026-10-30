//! Per-game stream quality and the layout prefs.

use super::{AppState, blocking, err, parse};
use crate::prefs::Entry;
use crate::stream::{Codec, Preset, StreamReq};
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use std::sync::Arc;

pub(super) async fn prefs_get(State(s): State<Arc<AppState>>) -> Response {
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
pub(super) struct LayoutBody {
    home_order: Option<Vec<String>>,
    machines_order: Option<Vec<String>>,
    machine_colors: Option<std::collections::BTreeMap<String, Option<String>>>,
}

/// The Home card and Machines page orders plus the machine colours: at least one key, orders
/// arrays of strings (else 400); bad, duplicate and surplus ids are dropped (prefs::clean_ids),
/// a key not sent keeps its value. `machineColors` is merged (null removes an id) and a bad id,
/// bad colour or more than 64 entries is a 400 that stores nothing. All three are returned.
pub(super) async fn layout_put(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
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
        Ok(b) => match blocking(&s, move |s| {
            s.prefs
                .set_layout(b.home_order, b.machines_order, b.machine_colors)
        })
        .await
        {
            Ok(Ok((home, machines, colors))) => axum::Json(serde_json::json!({
                "homeOrder": home, "machinesOrder": machines, "machineColors": colors
            }))
            .into_response(),
            Ok(Err(e)) => err(StatusCode::BAD_REQUEST, e),
            Err(r) => r,
        },
        Err(r) => r,
    }
}

/// Reset layout: forget the saved orders and colours (Home and Machines fall back to their defaults).
pub(super) async fn layout_delete(State(s): State<Arc<AppState>>) -> Response {
    let done = blocking(&s, |s| {
        let all = s
            .prefs
            .machine_colors()
            .into_keys()
            .map(|k| (k, None))
            .collect();
        let _ = s.prefs.set_layout(Some(vec![]), Some(vec![]), Some(all));
    })
    .await;
    match done {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PrefsBody {
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
pub(super) async fn prefs_put(
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
    match blocking(&s, move |s| s.prefs.put(id.as_str(), e)).await {
        Ok(e) => axum::Json(e).into_response(),
        Err(r) => r,
    }
}

pub(super) async fn prefs_delete(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
) -> Response {
    match s.parse_id(&raw) {
        Some(id) => match blocking(&s, move |s| s.prefs.delete(id.as_str())).await {
            Ok(_) => StatusCode::NO_CONTENT.into_response(),
            Err(r) => r,
        },
        None => err(StatusCode::BAD_REQUEST, "bad game id"),
    }
}
