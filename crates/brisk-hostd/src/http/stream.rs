//! Stream and session routes.

use super::*;

pub(super) async fn stream_get(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(s.stream.info()).into_response()
}

pub(super) async fn stream_start(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
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

pub(super) async fn stream_stop(State(s): State<Arc<AppState>>) -> Response {
    match tokio::spawn(async move { s.stream.stop().await }).await {
        Ok(i) => axum::Json(i).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "stop failed"),
    }
}

pub(super) async fn sessions(State(s): State<Arc<AppState>>) -> Response {
    // Each row carries a 24-point delay sparkline (from its series file) so the list needs no per-card GET.
    let rows = blocking(&s, |s| {
        s.stream
            .sessions(100)
            .into_iter()
            .filter_map(|r| {
                let spark = s.stream.sessions.spark(r.started, 24);
                let id = r.started;
                let mut v = serde_json::to_value(r)
                    .inspect_err(|e| tracing::warn!("sessions: skipping session {id}: {e}"))
                    .ok()?;
                v["spark"] = spark.into();
                Some(v)
            })
            .collect::<Vec<serde_json::Value>>()
    })
    .await;
    match rows {
        Ok(rows) => axum::Json(serde_json::json!({ "sessions": rows })).into_response(),
        Err(r) => r,
    }
}

/// `{session, series}`; 404 for a session hostd has no record of.
pub(super) async fn session_get(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<u64>,
) -> Response {
    let got = blocking(&s, move |s| {
        let sessions = &s.stream.sessions;
        sessions.get(id).map(|r| (r, sessions.series(id)))
    })
    .await;
    match got {
        Ok(Some((r, series))) => {
            axum::Json(serde_json::json!({ "session": r, "series": series })).into_response()
        }
        Ok(None) => err(StatusCode::NOT_FOUND, "no such session"),
        Err(r) => r,
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SeriesBody {
    points: Vec<Point>,
}

/// `{points: [[t, delayMs, fps, mbps], ...]}` merged by t; 400 for a bad or implausible point
/// (JSON has no NaN), 413 past MAX_POINTS (in the request or once merged) or 1 MiB, 404 for an
/// unknown session.
pub(super) async fn series_put(
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
