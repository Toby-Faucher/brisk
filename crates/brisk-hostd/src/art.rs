//! Art and details for emulator games, looked up online once and kept in `data_dir/art/`.
//!
//! Per game, `art/<key>/` holds `match.json` (the confirmed SteamGridDB game), `nomatch` (a
//! "none of these" answer), `cover|hero|logo|icon.<ext>`, `meta.json` (details and credits) and
//! `done.json` (the fetch finished; nothing is asked again until a refresh clears the directory).
//! `art/pending.json` is the confirm queue: games whose title matched several things.
//!
//! The pipeline is blocking and runs off the request path (`Art::spawn`: one game at a time, one
//! task per game). Nothing here runs on a timer or on `/api/games`; that route only reads the cache
//! (`decorate`). The title comes from the library record; GameTDB only adds details and a better
//! title when it is configured. Notes: docs/research/sgdb-notes.md.

use crate::config::Config;
use crate::game::{Details, Game, GameId};
use crate::gametdb;
use crate::http::{AppState, err};
use crate::steamgriddb::{self as sg, ArtKind, Candidate, SgdbConfig, write_private};
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const KINDS: [(&str, ArtKind); 4] = [
    ("cover", ArtKind::Grid),
    ("hero", ArtKind::Hero),
    ("logo", ArtKind::Logo),
    ("icon", ArtKind::Icon),
];
const PENDING: &str = "pending.json";
const MAX_PENDING: usize = 50;
const MAX_CANDS: usize = 3;
/// Images tried per kind before giving up on that kind.
const TRIES: usize = 3;
pub const CREDIT_SGDB: &str = "SteamGridDB";
pub const CREDIT_TDB: &str = "GameTDB";

// ---- names and keys ----

/// The art directory name for a game: `emu:dolphin:RSBE01` is `emu_dolphin_RSBE01`. The emulator
/// part has no `_`, so two ids never share a key. None when it would be over 64 characters.
pub fn key_of(id: &GameId) -> Option<String> {
    let k = id.as_str().replace(':', "_");
    (k.len() <= 64).then_some(k)
}

/// The title to search for: bracketed tags and a "Disc N" suffix gone, underscores as spaces,
/// a trailing system word dropped, blanks collapsed.
pub fn clean_name(raw: &str) -> String {
    let mut s = String::new();
    let mut depth = 0u32;
    for c in raw.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '_' => s.push(' '),
            c => s.push(c),
        }
    }
    let mut w: Vec<&str> = s.split_whitespace().collect();
    loop {
        let n = w.len();
        let last = w.last().map(|t| t.to_ascii_lowercase());
        let prev = n.checked_sub(2).map(|i| w[i].to_ascii_lowercase());
        if n > 2
            && prev.as_deref() == Some("disc")
            && last
                .as_deref()
                .is_some_and(|t| t.chars().all(|c| c.is_ascii_digit()))
        {
            w.truncate(n - 2);
        } else if n > 1 && matches!(last.as_deref(), Some("wii" | "gamecube" | "gc" | "-" | ":")) {
            w.pop();
        } else if n > 1 && matches!(last.as_deref(), Some("for")) {
            w.pop();
        } else {
            break;
        }
    }
    w.join(" ").trim_matches(['-', ':', ' ']).to_string()
}

// ---- files ----

fn dir_of(data_dir: &Path, key: &str) -> PathBuf {
    data_dir.join("art").join(key)
}

fn read_json<T: for<'a> Deserialize<'a>>(p: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

fn write_json<T: Serialize>(p: &Path, v: &T) -> Result<(), sg::Error> {
    write_private(
        p,
        &serde_json::to_vec(v).map_err(|_| sg::Error::BadResponse)?,
    )
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub year: Option<u16>,
    #[serde(default)]
    pub publisher: String,
    #[serde(default)]
    pub developer: String,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PendingItem {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Pending {
    #[serde(default)]
    pub items: BTreeMap<String, PendingItem>,
}

pub fn load_pending(data_dir: &Path) -> Pending {
    let mut p: Pending = read_json(&data_dir.join("art").join(PENDING)).unwrap_or_default();
    while p.items.len() > MAX_PENDING {
        p.items.pop_last();
    }
    for i in p.items.values_mut() {
        i.candidates.truncate(MAX_CANDS);
    }
    p
}

fn find_img(dir: &Path, name: &str) -> Option<PathBuf> {
    sg::EXTS
        .iter()
        .map(|e| dir.join(format!("{name}.{e}")))
        .find(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file()))
}

/// Fills the art URLs, details, credits and `needsMatch` of an emulator card from the cache. Reads
/// files only.
pub fn decorate(g: &mut Game, data_dir: &Path, pending: &Pending) {
    if !g.caps.art {
        return;
    }
    g.needs_match = pending.items.contains_key(g.id.as_str());
    let Some(key) = key_of(&g.id) else { return };
    let dir = dir_of(data_dir, &key);
    let seg = g.id.as_str().replace(':', "%3A");
    for (kind, _) in KINDS {
        let Some(p) = find_img(&dir, kind) else {
            continue;
        };
        let v = std::fs::metadata(&p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let url = format!("/api/games/{seg}/art/{kind}?v={v}");
        match kind {
            "cover" => g.cover = url,
            "hero" => g.hero = url,
            "logo" => g.logo = url,
            _ => {}
        }
    }
    if let Some(m) = read_json::<Meta>(&dir.join("meta.json")) {
        if m.year.is_some()
            || !m.publisher.is_empty()
            || !m.developer.is_empty()
            || !m.languages.is_empty()
        {
            g.details = Some(Details {
                year: m.year,
                publisher: m.publisher,
                developer: m.developer,
                languages: m.languages,
            });
        }
        g.credits = m.sources;
    }
}

// ---- serving ----

pub fn image_type(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// One cached image: (content type, etag, bytes). None for an unknown kind or a missing file.
pub fn read_art(
    data_dir: &Path,
    id: &GameId,
    kind: &str,
) -> Option<(&'static str, String, Vec<u8>)> {
    let (name, _) = KINDS.iter().find(|(k, _)| *k == kind)?;
    let p = find_img(&dir_of(data_dir, &key_of(id)?), name)?;
    let body = std::fs::read(&p).ok()?;
    let ct = image_type(&body)?;
    let mt = std::fs::metadata(&p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    Some((ct, format!("\"{}-{mt}\"", body.len()), body))
}

// ---- the pipeline ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Art fetched now (or details saved).
    Fetched,
    /// Everything was cached before; no request was made.
    Cached,
    /// Several possible games: the choice waits in the queue.
    Pending,
    /// No hits, or a "none of these" answer.
    NoMatch,
    /// No SteamGridDB key on this machine.
    Off,
    /// A network or key problem; nothing is stored, the next request tries again.
    Failed(String),
}

type KeyLoader = Box<dyn Fn(&Path) -> Option<SgdbConfig> + Send + Sync>;

pub struct Art {
    sgdb: KeyLoader,
    tdb: fn(&Config, &str) -> Option<gametdb::Entry>,
    inflight: Mutex<HashSet<String>>,
    /// One pipeline at a time, so a scan of many games makes a slow trickle of calls.
    gate: tokio::sync::Semaphore,
    pending_lock: Mutex<()>,
    refreshed: Mutex<HashMap<String, Instant>>,
    pub refresh_gap: Duration,
}

fn real_tdb(cfg: &Config, id: &str) -> Option<gametdb::Entry> {
    // Not configured means no call at all.
    std::env::var_os(gametdb::ENV_XML_URL)?;
    gametdb::lookup(cfg, id).ok()
}

impl Default for Art {
    fn default() -> Self {
        Art::new(Box::new(SgdbConfig::from_env), real_tdb)
    }
}

fn transient(e: &sg::Error) -> bool {
    use sg::Error::*;
    matches!(
        e,
        Unauthorized | RateLimited | Http(_) | Transport(_) | Io(_) | BadBase
    )
}

impl Art {
    pub fn new(sgdb: KeyLoader, tdb: fn(&Config, &str) -> Option<gametdb::Entry>) -> Art {
        Art {
            sgdb,
            tdb,
            inflight: Mutex::default(),
            gate: tokio::sync::Semaphore::new(1),
            pending_lock: Mutex::default(),
            refreshed: Mutex::default(),
            refresh_gap: Duration::from_secs(30),
        }
    }

    pub fn has_key(&self, data_dir: &Path) -> bool {
        (self.sgdb)(data_dir).is_some()
    }

    fn edit_pending(&self, data_dir: &Path, f: impl FnOnce(&mut Pending)) {
        let _g = self.pending_lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut p = load_pending(data_dir);
        f(&mut p);
        let path = data_dir.join("art").join(PENDING);
        if let Err(e) = write_json(&path, &p) {
            eprintln!("art: could not save the queue ({e})");
        }
    }

    /// The whole pipeline for one game. Blocking.
    pub fn run(&self, cfg: &Config, id: &GameId, title: &str, system: Option<&str>) -> Outcome {
        let Some(key) = key_of(id) else {
            return Outcome::Failed("game id too long for the art cache".into());
        };
        let dir = dir_of(&cfg.data_dir, &key);
        if dir.join("done.json").is_file() {
            return Outcome::Cached;
        }
        let mut meta: Meta = read_json(&dir.join("meta.json")).unwrap_or_default();
        // GameTDB (only when configured): a better title and the description fields, once.
        if !meta.sources.iter().any(|s| s == CREDIT_TDB)
            && matches!(system, Some("wii" | "gamecube"))
            && let Some((_, gid)) = id.emu_parts()
            && let Some(e) = (self.tdb)(cfg, gid.split('-').next().unwrap_or(gid))
        {
            meta.title = e.title("EN").to_string();
            meta.year = e.year;
            meta.publisher = e.publisher.clone();
            meta.developer = e.developer.clone();
            meta.languages = e.languages.clone();
            meta.sources.push(CREDIT_TDB.into());
            let _ = write_json(&dir.join("meta.json"), &meta);
        }
        let Some(sgc) = (self.sgdb)(&cfg.data_dir) else {
            return Outcome::Off;
        };
        let cand = match sg::load_match(&sgc, &key) {
            Some(c) => c,
            None => {
                if dir.join("nomatch").is_file() {
                    return Outcome::NoMatch;
                }
                let q = clean_name(if meta.title.is_empty() {
                    title
                } else {
                    &meta.title
                });
                let hits = match sg::search(&sgc, &q) {
                    Ok(h) => h,
                    Err(e) => return Outcome::Failed(e.to_string()),
                };
                let r = sg::rank(&q, &hits);
                match r.best {
                    Some(c) if r.confident => {
                        if let Err(e) = sg::save_match(&sgc, &key, &c) {
                            return Outcome::Failed(e.to_string());
                        }
                        self.edit_pending(&cfg.data_dir, |p| {
                            p.items.remove(id.as_str());
                        });
                        c
                    }
                    _ if r.alternatives.is_empty() => {
                        let _ = write_private(&dir.join("nomatch"), b"");
                        return Outcome::NoMatch;
                    }
                    _ => {
                        let item = PendingItem {
                            title: title.trim().to_string(),
                            system: system.map(String::from),
                            candidates: r.alternatives.into_iter().take(MAX_CANDS).collect(),
                        };
                        self.edit_pending(&cfg.data_dir, |p| {
                            if p.items.len() < MAX_PENDING || p.items.contains_key(id.as_str()) {
                                p.items.insert(id.as_str().into(), item);
                            }
                        });
                        return Outcome::Pending;
                    }
                }
            }
        };
        if let Err(e) = fetch_all(&sgc, &key, &dir, &cand) {
            return Outcome::Failed(e.to_string());
        }
        if !meta.sources.iter().any(|s| s == CREDIT_SGDB) {
            meta.sources.push(CREDIT_SGDB.into());
        }
        if meta.year.is_none() {
            meta.year = cand.year.and_then(|y| u16::try_from(y).ok());
        }
        let _ = write_json(&dir.join("meta.json"), &meta);
        let _ = write_json(&dir.join("done.json"), &serde_json::json!({}));
        Outcome::Fetched
    }

    /// Queues the pipeline for a game: one task per game, one running at a time. False when this
    /// game is already queued or running.
    pub fn spawn(
        self: &Arc<Self>,
        cfg: Config,
        id: GameId,
        title: String,
        system: Option<String>,
    ) -> bool {
        let Some(key) = key_of(&id) else { return false };
        if !self
            .inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.clone())
        {
            return false;
        }
        let me = self.clone();
        tokio::spawn(async move {
            let _turn = me.gate.acquire().await;
            let (m2, id2) = (me.clone(), id.clone());
            let r =
                tokio::task::spawn_blocking(move || m2.run(&cfg, &id2, &title, system.as_deref()))
                    .await;
            match r {
                Ok(Outcome::Failed(why)) => eprintln!("art: {id}: {why}"),
                Ok(o) => eprintln!("art: {id}: {o:?}"),
                Err(_) => eprintln!("art: {id}: task failed"),
            }
            me.inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
        });
        true
    }

    pub fn busy(&self, id: &GameId) -> bool {
        key_of(id).is_some_and(|k| {
            self.inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&k)
        })
    }

    /// New games (a scan or an upload): look each up once.
    pub fn trigger_new(self: &Arc<Self>, cfg: &Config, ids: &[GameId]) {
        let recs = crate::emu::load(&cfg.data_dir);
        for r in recs.iter().filter(|r| ids.contains(&r.id)) {
            self.spawn(cfg.clone(), r.id.clone(), r.name.clone(), r.system.clone());
        }
    }

    /// Records the answer to a queued question. `Some(id)` confirms that candidate (the caller then
    /// runs the fetch); `None` caches "none of these". Err: nothing queued, or not a candidate.
    pub fn answer(
        &self,
        data_dir: &Path,
        id: &GameId,
        pick: Option<u64>,
    ) -> Result<(), &'static str> {
        let key = key_of(id).ok_or("bad game id")?;
        let item = load_pending(data_dir)
            .items
            .remove(id.as_str())
            .ok_or("nothing to choose for this game")?;
        let sgc_dir = dir_of(data_dir, &key);
        match pick {
            Some(n) => {
                let c = item
                    .candidates
                    .into_iter()
                    .find(|c| c.id == n)
                    .ok_or("not one of the candidates")?;
                write_json(&sgc_dir.join("match.json"), &c).map_err(|_| "could not save")?;
            }
            None => write_private(&sgc_dir.join("nomatch"), b"").map_err(|_| "could not save")?,
        }
        self.edit_pending(data_dir, |p| {
            p.items.remove(id.as_str());
        });
        Ok(())
    }

    /// Clears the match and the cache of a game, so the next run searches again.
    pub fn clear(&self, data_dir: &Path, id: &GameId) {
        if let Some(k) = key_of(id) {
            let _ = std::fs::remove_dir_all(dir_of(data_dir, &k));
        }
        self.edit_pending(data_dir, |p| {
            p.items.remove(id.as_str());
        });
    }
}

fn fetch_all(sgc: &SgdbConfig, key: &str, dir: &Path, cand: &Candidate) -> Result<(), sg::Error> {
    for (name, kind) in KINDS {
        if find_img(dir, name).is_some() {
            continue;
        }
        let list = match sg::art_for(sgc, cand.id, kind) {
            Ok(l) => l,
            Err(e) if transient(&e) => return Err(e),
            Err(_) => continue,
        };
        let dest = sg::art_dir(sgc, key)?.join(name);
        for c in list.iter().take(TRIES) {
            match sg::fetch_art(sgc, &c.url, &dest) {
                Ok(_) => break,
                Err(e) if transient(&e) => return Err(e),
                Err(_) => {}
            }
        }
    }
    Ok(())
}

// ---- routes ----

fn emu_id(s: &AppState, raw: &str) -> Result<GameId, Response> {
    let id = s
        .parse_id(raw)
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "bad game id"))?;
    if id.source() != crate::game::Source::Emu {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "not supported: art is for emulator games",
        ));
    }
    let recs = crate::emu::load(&s.cfg.data_dir);
    if crate::emu::find(&recs, &id).is_none() {
        return Err(err(StatusCode::NOT_FOUND, "unknown game"));
    }
    Ok(id)
}

/// GET /api/art/pending: the confirm queue, only for games still in the library.
pub async fn pending(State(s): State<Arc<AppState>>) -> Response {
    let (cfg, st) = (s.cfg.clone(), s.clone());
    let r = tokio::task::spawn_blocking(move || {
        let recs = crate::emu::load(&cfg.data_dir);
        let items: Vec<_> = load_pending(&cfg.data_dir)
            .items
            .into_iter()
            .filter(|(id, _)| recs.iter().any(|r| r.id.as_str() == id))
            .map(|(id, i)| serde_json::json!({ "id": id, "title": i.title, "system": i.system, "candidates": i.candidates }))
            .collect();
        serde_json::json!({ "hasKey": st.art.has_key(&cfg.data_dir), "items": items, "credits": [CREDIT_SGDB] })
    })
    .await;
    match r {
        Ok(v) => axum::Json(v).into_response(),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "queue unavailable"),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MatchBody {
    candidate_id: Option<u64>,
    none: Option<bool>,
}

/// POST /api/art/{id}/match: `{"candidateId": n}` confirms and fetches, `{"none": true}` is cached.
pub async fn answer(
    State(s): State<Arc<AppState>>,
    UrlPath(raw): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    let id = match emu_id(&s, &raw) {
        Ok(i) => i,
        Err(r) => return r,
    };
    let pick = match serde_json::from_slice::<MatchBody>(&b) {
        Ok(MatchBody {
            candidate_id: Some(n),
            none: None | Some(false),
        }) => Some(n),
        Ok(MatchBody {
            candidate_id: None,
            none: Some(true),
        }) => None,
        _ => {
            return err(
                StatusCode::BAD_REQUEST,
                "bad request: send candidateId or none",
            );
        }
    };
    let st = s.clone();
    let dd = s.cfg.data_dir.clone();
    let id2 = id.clone();
    let r = tokio::task::spawn_blocking(move || st.art.answer(&dd, &id2, pick)).await;
    match r {
        Ok(Ok(())) if pick.is_none() => {
            axum::Json(serde_json::json!({ "state": "none" })).into_response()
        }
        Ok(Ok(())) => {
            start(&s, &id);
            (
                StatusCode::ACCEPTED,
                axum::Json(serde_json::json!({ "state": "fetching" })),
            )
                .into_response()
        }
        Ok(Err(e)) => err(StatusCode::NOT_FOUND, e),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "task failed"),
    }
}

fn start(s: &Arc<AppState>, id: &GameId) {
    let recs = crate::emu::load(&s.cfg.data_dir);
    if let Some(r) = crate::emu::find(&recs, id) {
        s.art.spawn(
            s.cfg.clone(),
            r.id.clone(),
            r.name.clone(),
            r.system.clone(),
        );
    }
}

/// POST /api/art/{id}/refresh: forgets the match and the cached art and searches again. One per
/// game every `refresh_gap`; needs a key, so a machine without one loses nothing.
pub async fn refresh(State(s): State<Arc<AppState>>, UrlPath(raw): UrlPath<String>) -> Response {
    let id = match emu_id(&s, &raw) {
        Ok(i) => i,
        Err(r) => return r,
    };
    if !s.art.has_key(&s.cfg.data_dir) {
        return err(
            StatusCode::CONFLICT,
            "Art needs a SteamGridDB key on the machine",
        );
    }
    if s.art.busy(&id) {
        return err(StatusCode::CONFLICT, "already looking for art");
    }
    {
        let mut m = s.art.refreshed.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = m
            .get(id.as_str())
            .filter(|t| t.elapsed() < s.art.refresh_gap)
        {
            let wait = (s.art.refresh_gap - t.elapsed()).as_secs() + 1;
            let mut r = err(StatusCode::TOO_MANY_REQUESTS, "searched a moment ago");
            r.headers_mut()
                .insert(header::RETRY_AFTER, header::HeaderValue::from(wait));
            return r;
        }
        if m.len() > 1000 {
            m.clear();
        }
        m.insert(id.as_str().into(), Instant::now());
    }
    let (st, dd, id2) = (s.clone(), s.cfg.data_dir.clone(), id.clone());
    let _ = tokio::task::spawn_blocking(move || st.art.clear(&dd, &id2)).await;
    start(&s, &id);
    (
        StatusCode::ACCEPTED,
        axum::Json(serde_json::json!({ "state": "searching" })),
    )
        .into_response()
}

/// GET /api/games/{id}/art/{kind}: a cached image. The id must be a well-formed emulator id and
/// the kind one of four words; nothing else reaches the file system.
pub async fn serve(
    State(s): State<Arc<AppState>>,
    UrlPath((raw, kind)): UrlPath<(String, String)>,
    h: HeaderMap,
) -> Response {
    let Some(id) = GameId::parse(&raw).filter(|i| i.source() == crate::game::Source::Emu) else {
        return err(StatusCode::BAD_REQUEST, "bad game id");
    };
    if !KINDS.iter().any(|(k, _)| *k == kind) {
        return err(StatusCode::NOT_FOUND, "no such art");
    }
    let dd = s.cfg.data_dir.clone();
    let got = tokio::task::spawn_blocking(move || read_art(&dd, &id, &kind)).await;
    let Ok(Some((ct, etag, body))) = got else {
        return err(StatusCode::NOT_FOUND, "no such art");
    };
    let cache = (
        header::CACHE_CONTROL,
        "private, max-age=31536000, immutable",
    );
    if h.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return (
            StatusCode::NOT_MODIFIED,
            [cache, (header::ETAG, etag.as_str())],
        )
            .into_response();
    }
    (
        [
            (header::CONTENT_TYPE, ct),
            cache,
            (header::ETAG, etag.as_str()),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_cleaning_table() {
        for (raw, want) in [
            ("SUPER MARIO GALAXY", "SUPER MARIO GALAXY"),
            ("Mario Kart Wii (USA)", "Mario Kart"),
            ("Metroid Prime [!] (Europe) (En,Fr)", "Metroid Prime"),
            ("Super_Smash_Bros_Brawl", "Super Smash Bros Brawl"),
            ("Resident Evil 4 Disc 1", "Resident Evil 4"),
            (
                "Final Fantasy Crystal Chronicles - Wii",
                "Final Fantasy Crystal Chronicles",
            ),
            ("Wii", "Wii"),
            (
                "  Zelda   Twilight Princess  GC ",
                "Zelda Twilight Princess",
            ),
            ("Disc 2", "Disc 2"),
            ("(USA)", ""),
        ] {
            assert_eq!(clean_name(raw), want, "{raw}");
        }
    }

    #[test]
    fn keys_are_flat_and_bounded() {
        let id = GameId::parse("emu:dolphin:RSBE01").unwrap();
        assert_eq!(key_of(&id).unwrap(), "emu_dolphin_RSBE01");
        let long = GameId::emu("dolphin", &"a".repeat(60)).unwrap();
        assert_eq!(key_of(&long), None);
    }

    #[test]
    fn image_types_come_from_the_bytes() {
        assert_eq!(image_type(b"\x89PNG\r\n\x1a\nxxxx"), Some("image/png"));
        assert_eq!(image_type(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_type(b"<svg></svg>"), None);
    }
}
