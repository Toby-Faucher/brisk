//! Notes, tags and the old-id remap.

use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RemapBody {
    map: std::collections::BTreeMap<String, String>,
}

/// `{map: {"<old number>": "<new id>"}}`: rekeys notes and prefs entries of emulator games that
/// startup could not map (the hub has no library of its own; the web client knows `legacyId` and
/// `id` from each machine's games). Keys must be old emulator numbers, values new emulator ids;
/// at most 2000 pairs, else 400. Answers `{notes, prefs}` counts. Safe to repeat.
pub(super) async fn ids_remap(State(s): State<Arc<AppState>>, b: axum::body::Bytes) -> Response {
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

pub(super) fn notes_err(e: NotesError) -> Response {
    match e {
        NotesError::Bad(m) => err(StatusCode::BAD_REQUEST, m),
        NotesError::Full => err(StatusCode::CONFLICT, "too many notes or tags"),
        NotesError::Io => err(StatusCode::INTERNAL_SERVER_ERROR, "could not save notes"),
    }
}

pub(super) async fn notes_get(State(s): State<Arc<AppState>>) -> Response {
    axum::Json(s.notes.all()).into_response()
}

/// Creates or replaces one tag; answers with what was stored (name trimmed).
pub(super) async fn tag_put(
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
pub(super) async fn tag_delete(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
) -> Response {
    match s.notes.delete_tag(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => notes_err(e),
    }
}

/// One game's note and tags; answers with what was stored (unknown tag ids dropped).
pub(super) async fn game_note_put(
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
