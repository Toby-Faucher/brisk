//! SteamGridDB art by game name: search, rank the hits, list art, download once into the cache.
//!
//! Everything here is blocking (callers wrap it in `spawn_blocking`, like `steam.rs`). The API key
//! is read from its file, lives in a redacted newtype and goes out only as the Authorization
//! header. Facts marked UNVERIFIED were not confirmable from the official docs page (it renders
//! client-side and could not be fetched); see docs/research/sgdb-notes.md.

use crate::steam::{base_ok, get_limited};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEFAULT_BASE: &str = "https://www.steamgriddb.com/api/v2";
/// shortcut: UNVERIFIED hosts (the docs show only example.com); cdn2 is what wrappers' sample
/// output uses. Confirm with one real response, then fix this list.
const DEFAULT_HOSTS: &[&str] = &[
    "cdn2.steamgriddb.com",
    "cdn.steamgriddb.com",
    "www.steamgriddb.com",
    "steamgriddb.com",
];
const MAX_JSON: u64 = 2 << 20;
const MAX_IMAGE: u64 = 10 << 20;
/// shortcut: UNVERIFIED (no published rate limit); 2 calls a second is polite. Raise if needed.
const MIN_INTERVAL: Duration = Duration::from_millis(500);

/// The key, with a Debug that says nothing.
#[derive(Clone)]
struct Key(String);
impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(<redacted>)")
    }
}

#[derive(Clone, Debug)]
pub struct SgdbConfig {
    pub base: String,
    key: Key,
    pub data_dir: PathBuf,
    /// Exact host names an art URL may point at (no ports).
    pub allowed_hosts: Vec<String>,
    pub min_interval: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A name, key or path that is not safe to use.
    BadInput(&'static str),
    BadBase,
    /// 401/403: the key was refused.
    Unauthorized,
    RateLimited,
    NotFound,
    Http(u16),
    Transport(String),
    TooLarge,
    BadResponse,
    BadImage,
    HostNotAllowed,
    Io(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BadInput(w) => write!(f, "bad input: {w}"),
            Error::BadBase => f.write_str("SteamGridDB base must be https (or http to loopback)"),
            Error::Unauthorized => f.write_str("SteamGridDB refused the API key"),
            Error::RateLimited => f.write_str("SteamGridDB rate limit (HTTP 429)"),
            Error::NotFound => f.write_str("not found"),
            Error::Http(c) => write!(f, "HTTP {c}"),
            Error::Transport(t) => write!(f, "request failed: {t}"),
            Error::TooLarge => f.write_str("response too large"),
            Error::BadResponse => f.write_str("unexpected response shape"),
            Error::BadImage => f.write_str("not a PNG, JPEG or WEBP image"),
            Error::HostNotAllowed => f.write_str("image host is not on the allowlist"),
            Error::Io(k) => write!(f, "i/o error ({k})"),
        }
    }
}
impl std::error::Error for Error {}

impl SgdbConfig {
    /// None = feature off (no key file, or not a plain key). `BRISK_SGDB_BASE` points tests at a
    /// mock; a loopback base also puts its host on the image allowlist.
    pub fn from_env(data_dir: &Path) -> Option<SgdbConfig> {
        let key_file = std::env::var_os("BRISK_SGDB_KEY_FILE")
            .map(PathBuf::from)
            .or_else(|| {
                let h = std::env::var_os("HOME")?;
                Some(PathBuf::from(h).join(".config/brisk/steamgriddb-api-key"))
            })?;
        let base = std::env::var("BRISK_SGDB_BASE").unwrap_or_else(|_| DEFAULT_BASE.into());
        Self::with_key_file(&key_file, base, data_dir)
    }

    pub fn with_key_file(key_file: &Path, base: String, data_dir: &Path) -> Option<SgdbConfig> {
        let raw = std::fs::read_to_string(key_file).ok()?;
        let key = raw.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return None;
        }
        let base = base.trim_end_matches('/').to_string();
        let mut allowed_hosts: Vec<String> = DEFAULT_HOSTS.iter().map(|s| s.to_string()).collect();
        if let Some((_, h)) = split_url(&base)
            && is_loopback(&h)
        {
            allowed_hosts.push(h);
        }
        Some(SgdbConfig {
            base,
            key: Key(key.to_string()),
            data_dir: data_dir.to_path_buf(),
            allowed_hosts,
            min_interval: MIN_INTERVAL,
        })
    }
}

fn is_loopback(h: &str) -> bool {
    h == "127.0.0.1" || h == "localhost"
}

/// (scheme, lowercase host) of an http(s) URL; None for userinfo, IPv6 literals or junk.
fn split_url(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let auth = rest.split(['/', '?', '#']).next()?;
    if auth.contains(['@', '\\', '[']) || auth.is_empty() {
        return None;
    }
    let host = auth.split(':').next()?.to_ascii_lowercase();
    (!host.is_empty() && matches!(scheme, "http" | "https")).then(|| (scheme.to_string(), host))
}

fn host_allowed(cfg: &SgdbConfig, url: &str) -> bool {
    let Some((scheme, host)) = split_url(url) else {
        return false;
    };
    (scheme == "https" || is_loopback(&host)) && cfg.allowed_hosts.contains(&host)
}

static LAST_CALL: Mutex<Option<Instant>> = Mutex::new(None);

/// Sleeps until `min_interval` has passed since the previous call (any thread).
fn pace(cfg: &SgdbConfig) {
    let mut last = LAST_CALL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = *last {
        let wait = cfg.min_interval.saturating_sub(t.elapsed());
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }
    *last = Some(Instant::now());
}

fn map_err(e: String) -> Error {
    match e.as_str() {
        "HTTP 401" | "HTTP 403" => Error::Unauthorized,
        "HTTP 429" => Error::RateLimited,
        "HTTP 404" => Error::NotFound,
        "response too large" => Error::TooLarge,
        s => match s.strip_prefix("HTTP ").and_then(|c| c.parse().ok()) {
            Some(c) => Error::Http(c),
            None => Error::Transport(e),
        },
    }
}

/// One API GET, returning the `data` member of a `success: true` envelope.
fn api_get(cfg: &SgdbConfig, path_query: &str) -> Result<Value, Error> {
    if !base_ok(&cfg.base) {
        return Err(Error::BadBase);
    }
    pace(cfg);
    let url = format!("{}/{path_query}", cfg.base);
    let body = get_limited(&url, Some(&cfg.key.0), MAX_JSON).map_err(map_err)?;
    let mut v: Value = serde_json::from_slice(&body).map_err(|_| Error::BadResponse)?;
    if v.get("success") != Some(&Value::Bool(true)) {
        return Err(Error::BadResponse);
    }
    Ok(v["data"].take())
}

// ---- search and ranking ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: u64,
    pub name: String,
    pub verified: bool,
    pub year: Option<i32>,
}

/// Year of a unix timestamp (Hinnant's civil-from-days).
fn year_of(secs: i64) -> i32 {
    let z = secs.div_euclid(86400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2)) as i32
}

/// Looks the name up (region tags and platform suffix dropped from the search term).
pub fn search(cfg: &SgdbConfig, name: &str) -> Result<Vec<Candidate>, Error> {
    let term = normalize(name);
    if term.is_empty() {
        return Err(Error::BadInput("empty game name"));
    }
    let enc = percent_encoding::utf8_percent_encode(&term, percent_encoding::NON_ALPHANUMERIC);
    let data = match api_get(cfg, &format!("search/autocomplete/{enc}")) {
        Ok(d) => d,
        // shortcut: UNVERIFIED whether an empty search is 200 [] or 404; both mean "no hits".
        Err(Error::NotFound) => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let list = data.as_array().ok_or(Error::BadResponse)?;
    Ok(list
        .iter()
        .filter_map(|g| {
            Some(Candidate {
                id: g.get("id")?.as_u64()?,
                name: g.get("name")?.as_str()?.to_string(),
                // release_date: unix seconds per the node wrapper's types; absent is fine.
                verified: g.get("verified").and_then(Value::as_bool).unwrap_or(false),
                year: g.get("release_date").and_then(Value::as_i64).map(year_of),
            })
        })
        .collect())
}

/// Lowercase words: bracketed tags removed, "&" as "and", punctuation gone, a leading "the" and a
/// trailing platform word dropped, roman numerals II..XX (after the first word) as digits.
/// shortcut: a lone "I" stays as is, and "Final Fantasy X" equals "Final Fantasy 10" (intended).
pub fn normalize(name: &str) -> String {
    let mut s = String::new();
    let mut depth = 0u32;
    for c in name.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            '&' => s.push_str(" and "),
            c if c.is_alphanumeric() => s.extend(c.to_lowercase()),
            _ => s.push(' '),
        }
    }
    let mut w: Vec<String> = s.split_whitespace().map(String::from).collect();
    if w.first().is_some_and(|t| t == "the") {
        w.remove(0);
    }
    if w.len() > 1
        && w.last()
            .is_some_and(|t| matches!(t.as_str(), "wii" | "gamecube" | "gc"))
    {
        w.pop();
    }
    const ROMAN: [&str; 19] = [
        "ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x", "xi", "xii", "xiii", "xiv", "xv",
        "xvi", "xvii", "xviii", "xix", "xx",
    ];
    for t in w.iter_mut().skip(1) {
        if let Some(i) = ROMAN.iter().position(|r| r == t) {
            *t = (i + 2).to_string();
        }
    }
    w.join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranked {
    pub best: Option<Candidate>,
    /// True only for one clear exact match; anything else should be put to Toby.
    pub confident: bool,
    /// Up to 3 candidates, best first, for the "which one?" card. Empty when confident.
    pub alternatives: Vec<Candidate>,
}

/// Word-overlap score 0..=1000; 2000 for an exact normalised match.
fn score(want: &str, cand: &str) -> u32 {
    if want == cand {
        return 2000;
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (want.split(' ').collect(), cand.split(' ').collect());
    let common = a.iter().filter(|t| b.contains(t)).count() as u32;
    let union = (a.len() + b.len()) as u32 - common;
    (common * 1000).checked_div(union).unwrap_or(0)
}

pub fn rank(name: &str, cands: &[Candidate]) -> Ranked {
    let want = normalize(name);
    let mut seen = std::collections::HashSet::new();
    let mut scored: Vec<(u32, &Candidate)> = cands
        .iter()
        .filter(|c| seen.insert(c.id))
        .map(|c| (score(&want, &normalize(&c.name)), c))
        .filter(|(s, _)| *s > 0 && !want.is_empty())
        .collect();
    // Stable: ties keep the API's order; verified first within equal scores.
    scored.sort_by_key(|(s, c)| (std::cmp::Reverse(*s), std::cmp::Reverse(c.verified)));
    let exact: Vec<&Candidate> = scored
        .iter()
        .filter(|(s, _)| *s == 2000)
        .map(|(_, c)| *c)
        .collect();
    let verified: Vec<&&Candidate> = exact.iter().filter(|c| c.verified).collect();
    let pick = match (exact.len(), verified.len()) {
        (1, _) => Some(exact[0]),
        (n, 1) if n > 1 => Some(*verified[0]),
        _ => None,
    };
    if let Some(c) = pick {
        return Ranked {
            best: Some(c.clone()),
            confident: true,
            alternatives: vec![],
        };
    }
    let alternatives: Vec<Candidate> = scored.iter().take(3).map(|(_, c)| (*c).clone()).collect();
    Ranked {
        best: alternatives.first().cloned(),
        confident: false,
        alternatives,
    }
}

// ---- art lists ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtKind {
    Grid,
    Hero,
    Logo,
    Icon,
}
impl ArtKind {
    fn path(self) -> &'static str {
        match self {
            ArtKind::Grid => "grids",
            ArtKind::Hero => "heroes",
            ArtKind::Logo => "logos",
            ArtKind::Icon => "icons",
        }
    }
    /// The size to ask for first; logos and icons have no useful fixed size.
    fn wanted(self) -> Option<(u32, u32)> {
        match self {
            ArtKind::Grid => Some((600, 900)),
            ArtKind::Hero => Some((1920, 620)),
            _ => None,
        }
    }
    /// shortcut: UNVERIFIED mime lists per kind come from the wrappers (logos: png/webp; icons:
    /// png/ico). Icons ask for png only, so the magic check never meets an .ico.
    fn mimes(self) -> &'static str {
        match self {
            ArtKind::Grid | ArtKind::Hero => "image/png,image/jpeg,image/webp",
            ArtKind::Logo => "image/png,image/webp",
            ArtKind::Icon => "image/png",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArtChoice {
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub style: String,
    pub mime: Option<String>,
    pub score: i64,
}

/// Static art for a game, best first: the wanted size, then the format for the kind (PNG for
/// logos and icons, WEBP/JPEG before PNG for big art), then the community score. Content filters
/// are all `false` (no NSFW, humor or flashing art). First page only; when the sized query is
/// empty, one unsized retry. shortcut: no paging, 50 results are plenty for picking one.
pub fn art_for(cfg: &SgdbConfig, game_id: u64, kind: ArtKind) -> Result<Vec<ArtChoice>, Error> {
    let base = format!(
        "{}/game/{game_id}?types=static&nsfw=false&humor=false&epilepsy=false&mimes={}",
        kind.path(),
        kind.mimes()
    );
    let mut list = Vec::new();
    if let Some((w, h)) = kind.wanted() {
        list = parse_art(&api_get(cfg, &format!("{base}&dimensions={w}x{h}"))?)?;
    }
    if list.is_empty() {
        list = parse_art(&api_get(cfg, &base)?)?;
    }
    let fmt = |c: &ArtChoice| match (kind, c.mime.as_deref()) {
        (ArtKind::Logo | ArtKind::Icon, Some("image/png")) => 0,
        (ArtKind::Grid | ArtKind::Hero, Some("image/webp")) => 0,
        (ArtKind::Grid | ArtKind::Hero, Some("image/jpeg")) => 1,
        _ => 2,
    };
    list.sort_by_key(|c| {
        let sized = kind.wanted().is_none_or(|wh| wh == (c.width, c.height));
        (!sized, fmt(c), std::cmp::Reverse(c.score))
    });
    Ok(list)
}

fn parse_art(data: &Value) -> Result<Vec<ArtChoice>, Error> {
    let list = data.as_array().ok_or(Error::BadResponse)?;
    Ok(list
        .iter()
        .filter_map(|a| {
            let n = |k: &str| a.get(k).and_then(Value::as_u64).map(|x| x as u32);
            // `style` is a string in the docs' sample; the node wrapper types it as an array.
            let style = match a.get("style")? {
                Value::String(s) => s.clone(),
                Value::Array(v) => v.first()?.as_str()?.to_string(),
                _ => String::new(),
            };
            Some(ArtChoice {
                url: a.get("url")?.as_str()?.to_string(),
                width: n("width")?,
                height: n("height")?,
                style,
                mime: a.get("mime").and_then(Value::as_str).map(String::from),
                score: a.get("score").and_then(Value::as_i64).unwrap_or(0),
            })
        })
        .collect())
}

// ---- cache ----

fn io(e: std::io::Error) -> Error {
    Error::Io(format!("{:?}", e.kind()))
}

/// data_dir/art/<key>/, with the key limited to plain file-name characters.
pub fn art_dir(cfg: &SgdbConfig, key: &str) -> Result<PathBuf, Error> {
    let ok = !key.is_empty()
        && key.len() <= 64
        && !key.starts_with('.')
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok {
        return Err(Error::BadInput("art key"));
    }
    Ok(cfg.data_dir.join("art").join(key))
}

/// Atomic private write: temp file (0600, created fresh) then rename.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let dir = path.parent().ok_or(Error::BadInput("path"))?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(io)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("x")
    ));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(io)?;
    f.write_all(bytes).and_then(|()| f.sync_all()).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

/// The confirmed match for a game, so it is never asked twice.
pub fn load_match(cfg: &SgdbConfig, key: &str) -> Option<Candidate> {
    let p = art_dir(cfg, key).ok()?.join("match.json");
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

pub fn save_match(cfg: &SgdbConfig, key: &str, c: &Candidate) -> Result<(), Error> {
    let body = serde_json::to_vec(c).map_err(|_| Error::BadResponse)?;
    write_private(&art_dir(cfg, key)?.join("match.json"), &body)
}

fn image_ext(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

pub(crate) const EXTS: [&str; 3] = ["png", "jpg", "webp"];

/// Downloads `url` once to `dest` + the extension its bytes call for (`dest` is a path under
/// data_dir/art without extension, e.g. `art_dir(..)/grid`). If any such file exists it is
/// returned without a request. The URL host must be on the allowlist, redirects are refused
/// (by the shared helper), the body is capped at 10 MiB and checked for PNG/JPEG/WEBP magic.
pub fn fetch_art(cfg: &SgdbConfig, url: &str, dest: &Path) -> Result<PathBuf, Error> {
    let art_root = cfg.data_dir.join("art");
    let rel = dest
        .strip_prefix(&art_root)
        .map_err(|_| Error::BadInput("dest outside art dir"))?;
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) || rel.as_os_str().is_empty() {
        return Err(Error::BadInput("dest outside art dir"));
    }
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(Error::BadInput("dest"))?;
    for e in EXTS {
        let p = dest.with_file_name(format!("{name}.{e}"));
        if p.is_file() {
            return Ok(p);
        }
    }
    if !host_allowed(cfg, url) {
        return Err(Error::HostNotAllowed);
    }
    pace(cfg);
    // No Authorization header here: the key never goes to the image host.
    let body = get_limited(url, None, MAX_IMAGE).map_err(map_err)?;
    let ext = image_ext(&body).ok_or(Error::BadImage)?;
    let out = dest.with_file_name(format!("{name}.{ext}"));
    write_private(&out, &body)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::Arc;

    const KEY: &str = "SENTINELKEY-0123abcd";

    fn c(id: u64, name: &str, verified: bool) -> Candidate {
        Candidate {
            id,
            name: name.into(),
            verified,
            year: None,
        }
    }

    #[test]
    fn rank_table() {
        let mk = |n: &str| vec![c(1, n, false), c(2, "Something Else Entirely", false)];
        for (want, have) in [
            ("Mario Kart Wii", "Mario Kart Wii"),
            ("mario kart wii", "MARIO KART WII"),
            ("Mario Kart: Wii!", "Mario Kart Wii"),
            ("Super Mario Galaxy (USA)", "Super Mario Galaxy"),
            (
                "Super Mario Galaxy [!] (Europe) (En,Fr)",
                "Super Mario Galaxy",
            ),
            ("Metroid Prime Wii", "Metroid Prime"),
            ("Metroid Prime", "Metroid Prime Wii"),
            ("Final Fantasy VII", "Final Fantasy 7"),
            ("Final Fantasy 7", "Final Fantasy VII"),
            ("Resident Evil 4 (USA)", "Resident Evil IV"),
            ("The Legend of Zelda", "Legend of Zelda"),
            ("Sonic & Knuckles", "Sonic and Knuckles"),
        ] {
            let r = rank(want, &mk(have));
            assert!(r.confident, "{want} vs {have}");
            assert_eq!(r.best.unwrap().id, 1);
            assert!(r.alternatives.is_empty());
        }
    }

    #[test]
    fn rank_weak_and_none() {
        let r = rank(
            "Mario Kart Wii",
            &[c(1, "Mario Kart 8", false), c(2, "Mario Party", false)],
        );
        assert!(!r.confident);
        assert_eq!(r.best.as_ref().unwrap().id, 1);
        assert_eq!(r.alternatives.len(), 2);

        let r = rank("Zzz", &[c(1, "Mario Kart 8", false)]);
        assert_eq!(
            (r.best, r.confident, r.alternatives.len()),
            (None, false, 0)
        );
        assert_eq!(rank("Zzz", &[]).best, None);
        assert_eq!(rank("(USA)", &[c(1, "x", false)]).best, None);

        // At most 3 alternatives.
        let many: Vec<_> = (0..6)
            .map(|i| c(i, &format!("Mario Kart {i}"), false))
            .collect();
        assert_eq!(rank("Mario Kart", &many).alternatives.len(), 3);
    }

    #[test]
    fn rank_ties() {
        // Two exact hits: not confident, unless exactly one is verified.
        let r = rank("Doom", &[c(1, "Doom", false), c(2, "DOOM", false)]);
        assert!(!r.confident);
        assert_eq!(r.best.unwrap().id, 1, "API order wins a tie");
        let r = rank("Doom", &[c(1, "Doom", false), c(2, "DOOM", true)]);
        assert!(r.confident);
        assert_eq!(r.best.unwrap().id, 2);
        let r = rank("Doom", &[c(1, "Doom", true), c(2, "DOOM", true)]);
        assert!(!r.confident);
        // Same id twice is one candidate.
        assert!(rank("Doom", &[c(1, "Doom", false), c(1, "Doom", false)]).confident);
    }

    #[test]
    fn years() {
        assert_eq!(year_of(0), 1970);
        assert_eq!(year_of(1_162_339_200), 2006); // 2006-11-01
        assert_eq!(year_of(1_136_073_599), 2005); // 2005-12-31 23:59:59
        assert_eq!(year_of(1_136_073_600), 2006);
    }

    /// Loopback server: each route is (substring of the request line, head, body). Records the
    /// raw request heads. Unmatched requests get 404.
    type Route = (&'static str, String, Vec<u8>);
    fn mock(routes: Vec<Route>) -> (String, Arc<Mutex<Vec<String>>>) {
        use std::io::Write;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { return };
                let mut buf = [0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = req.lines().next().unwrap_or("").to_string();
                seen.lock().unwrap().push(req);
                let (head, body) = routes
                    .iter()
                    .find(|(m, ..)| line.contains(m))
                    .map(|(_, h, b)| (h.clone(), b.clone()))
                    .unwrap_or((
                        "HTTP/1.1 404 NF\r\ncontent-length: 0\r\n\r\n".into(),
                        vec![],
                    ));
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        (base, log)
    }
    fn ok(body: &[u8]) -> (String, Vec<u8>) {
        (
            format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            ),
            body.to_vec(),
        )
    }
    fn json(m: &'static str, body: &str) -> Route {
        let (h, b) = ok(body.as_bytes());
        (m, h, b)
    }
    fn status(m: &'static str, code: u16) -> Route {
        (
            m,
            format!("HTTP/1.1 {code} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"),
            vec![],
        )
    }

    fn cfg(base: &str, d: &Path) -> SgdbConfig {
        let kf = d.join("keyfile");
        std::fs::write(&kf, format!("{KEY}\n")).unwrap();
        let mut c = SgdbConfig::with_key_file(&kf, base.into(), &d.join("data")).unwrap();
        c.min_interval = Duration::ZERO;
        c
    }
    fn nokey(e: &Error) {
        assert!(!format!("{e} {e:?}").contains("SENTINEL"), "{e:?}");
    }

    #[test]
    fn search_hit_sends_bearer_only_in_the_header() {
        let (base, log) = mock(vec![json(
            "search/autocomplete/mario",
            r#"{"success":true,"data":[{"id":5,"name":"Mario Kart Wii","verified":true,"release_date":1209600000,"types":["steam"]},{"name":"no id"}]}"#,
        )]);
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg(&base, d.path());
        let got = search(&cfg, "Mario (USA)").unwrap();
        assert_eq!(
            got,
            vec![Candidate {
                id: 5,
                name: "Mario Kart Wii".into(),
                verified: true,
                year: Some(2008)
            }]
        );
        let reqs = log.lock().unwrap();
        assert!(
            reqs[0].starts_with("GET /search/autocomplete/mario "),
            "{}",
            reqs[0]
        );
        assert!(
            reqs[0]
                .to_lowercase()
                .contains(&format!("authorization: bearer {KEY}").to_lowercase())
        );
        assert!(!reqs[0].lines().next().unwrap().contains(KEY));
    }

    #[test]
    fn search_miss_and_errors() {
        let (base, _) = mock(vec![
            json("autocomplete/empty", r#"{"success":true,"data":[]}"#),
            status("autocomplete/gone", 404),
            status("autocomplete/bad", 401),
            status("autocomplete/slow", 429),
            status("autocomplete/boom", 500),
            json("autocomplete/junk", "{not json"),
            json("autocomplete/fail", r#"{"success":false,"errors":["x"]}"#),
            json("autocomplete/shape", r#"{"success":true,"data":{"a":1}}"#),
        ]);
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg(&base, d.path());
        assert_eq!(search(&cfg, "empty").unwrap(), vec![]);
        assert_eq!(search(&cfg, "gone").unwrap(), vec![]);
        for (n, want) in [
            ("bad", Error::Unauthorized),
            ("slow", Error::RateLimited),
            ("boom", Error::Http(500)),
            ("junk", Error::BadResponse),
            ("fail", Error::BadResponse),
            ("shape", Error::BadResponse),
        ] {
            let e = search(&cfg, n).unwrap_err();
            assert_eq!(e, want, "{n}");
            nokey(&e);
        }
        assert_eq!(
            search(&cfg, "(USA)").unwrap_err(),
            Error::BadInput("empty game name")
        );
    }

    #[test]
    fn oversize_response_is_cut() {
        let (base, _) = mock(vec![(
            "autocomplete",
            "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n".into(),
            vec![b'x'; (2 << 20) + 10],
        )]);
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            search(&cfg(&base, d.path()), "Big").unwrap_err(),
            Error::TooLarge
        );
    }

    #[test]
    fn plain_http_to_a_remote_base_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg("http://example.com/api", d.path());
        assert_eq!(search(&cfg, "x").unwrap_err(), Error::BadBase);
    }

    #[test]
    fn art_filters_and_order() {
        let body = r#"{"success":true,"data":[
          {"id":1,"score":9,"style":"alternate","width":920,"height":430,"mime":"image/png","url":"http://h/a.png"},
          {"id":2,"score":1,"style":"material","width":600,"height":900,"mime":"image/png","url":"http://h/b.png"},
          {"id":3,"score":2,"style":["blurred"],"width":600,"height":900,"mime":"image/webp","url":"http://h/c.webp"},
          {"id":4,"score":5,"style":"alternate","width":600,"height":900,"mime":"image/jpeg","url":"http://h/d.jpg"}]}"#;
        let (base, log) = mock(vec![json("grids/game/7", body)]);
        let d = tempfile::tempdir().unwrap();
        let got = art_for(&cfg(&base, d.path()), 7, ArtKind::Grid).unwrap();
        let urls: Vec<&str> = got.iter().map(|a| a.url.as_str()).collect();
        // 600x900 first (webp, jpeg, png), then the other size.
        assert_eq!(
            urls,
            [
                "http://h/c.webp",
                "http://h/d.jpg",
                "http://h/b.png",
                "http://h/a.png"
            ]
        );
        assert_eq!(got[0].style, "blurred");
        let q = log.lock().unwrap()[0].lines().next().unwrap().to_string();
        for p in [
            "dimensions=600x900",
            "types=static",
            "nsfw=false",
            "humor=false",
            "epilepsy=false",
            "mimes=image/png,image/jpeg,image/webp",
        ] {
            assert!(q.contains(p), "{q}");
        }
    }

    #[test]
    fn art_kinds_and_unsized_retry() {
        let (base, log) = mock(vec![
            json("heroes/game/7?types", r#"{"success":true,"data":[]}"#),
            json(
                "logos/game/7",
                r#"{"success":true,"data":[{"id":1,"style":"official","width":1,"height":1,"mime":"image/webp","url":"u1"},{"id":2,"style":"white","width":1,"height":1,"mime":"image/png","url":"u2"}]}"#,
            ),
            json("icons/game/7", r#"{"success":true,"data":[]}"#),
        ]);
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg(&base, d.path());
        assert_eq!(art_for(&cfg, 7, ArtKind::Hero).unwrap(), vec![]);
        // Hero: sized query then one unsized retry, no more.
        assert_eq!(log.lock().unwrap().len(), 2);
        assert!(log.lock().unwrap()[0].contains("dimensions=1920x620"));
        assert!(!log.lock().unwrap()[1].contains("dimensions"));
        let logos = art_for(&cfg, 7, ArtKind::Logo).unwrap();
        assert_eq!(logos[0].url, "u2", "png first for logos");
        assert!(art_for(&cfg, 7, ArtKind::Icon).unwrap().is_empty());
    }

    fn png() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend([0u8; 40]);
        v
    }

    #[test]
    fn fetch_happy_path_cache_and_modes() {
        use std::os::unix::fs::PermissionsExt;
        let img = png();
        let (hd, bd) = ok(&img);
        let (base, log) = mock(vec![("/g.bin", hd, bd)]);
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg(&base, d.path());
        let dest = art_dir(&cfg, "wii-RMCE01").unwrap().join("grid");
        let p = fetch_art(&cfg, &format!("{base}/g.bin"), &dest).unwrap();
        assert_eq!(p, dest.with_file_name("grid.png"));
        assert_eq!(std::fs::read(&p).unwrap(), img);
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let req = log.lock().unwrap()[0].to_lowercase();
        assert!(
            !req.contains("authorization") && !req.contains("sentinel"),
            "key sent to the image host"
        );
        // Second call: cache hit, no request (even for a URL that would be refused).
        assert_eq!(fetch_art(&cfg, "https://evil.example/x", &dest).unwrap(), p);
        assert_eq!(log.lock().unwrap().len(), 1);
        assert!(
            !std::fs::read_dir(p.parent().unwrap()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
        );
    }

    #[test]
    fn fetch_refusals() {
        let (hd, bd) = ok(b"<html>not an image, long enough to pass the length check</html>");
        let big = vec![0x89; (10 << 20) + 5];
        let (base, _) = mock(vec![
            ("/text", hd, bd),
            ("/big", "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n".into(), big),
            ("/redir", "HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:1/z\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(), vec![]),
        ]);
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg(&base, d.path());
        let dest = art_dir(&cfg, "k").unwrap().join("hero");
        assert_eq!(
            fetch_art(&cfg, &format!("{base}/text"), &dest).unwrap_err(),
            Error::BadImage
        );
        assert_eq!(
            fetch_art(&cfg, &format!("{base}/big"), &dest).unwrap_err(),
            Error::TooLarge
        );
        assert_eq!(
            fetch_art(&cfg, &format!("{base}/redir"), &dest).unwrap_err(),
            Error::Http(302)
        );
        for url in [
            "https://evil.example/a.png",
            "http://cdn2.steamgriddb.com/a.png", // plain http only to loopback
            "https://cdn2.steamgriddb.com@evil.example/a.png",
            "https://cdn2.steamgriddb.com.evil.example/a.png",
            "ftp://cdn2.steamgriddb.com/a.png",
            "http://localhost:1/a.png", // loopback but not on this config's list
        ] {
            assert_eq!(
                fetch_art(&cfg, url, &dest).unwrap_err(),
                Error::HostNotAllowed,
                "{url}"
            );
        }
        assert!(
            !art_dir(&cfg, "k").unwrap().exists(),
            "nothing written on failure"
        );
        // Destinations outside data_dir/art and odd keys are refused.
        for bad in [
            d.path().join("x"),
            cfg.data_dir.join("art/../x"),
            cfg.data_dir.join("art"),
        ] {
            assert!(
                matches!(
                    fetch_art(&cfg, &format!("{base}/text"), &bad),
                    Err(Error::BadInput(_))
                ),
                "{bad:?}"
            );
        }
        for k in ["", "..", ".hidden", "a/b", "a b"] {
            assert!(art_dir(&cfg, k).is_err(), "{k}");
        }
    }

    #[test]
    fn magic_bytes() {
        assert_eq!(image_ext(&png()), Some("png"));
        assert_eq!(image_ext(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
        assert_eq!(image_ext(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some("webp"));
        assert_eq!(image_ext(b"RIFF\x00\x00\x00\x00WAVEfmt "), None);
        assert_eq!(image_ext(b"GIF89a"), None);
    }

    #[test]
    fn match_cache_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg("http://127.0.0.1:1", d.path());
        assert_eq!(load_match(&cfg, "k"), None);
        let m = c(9, "Doom", true);
        save_match(&cfg, "k", &m).unwrap();
        assert_eq!(load_match(&cfg, "k"), Some(m));
        assert_eq!(load_match(&cfg, "../k"), None);
    }

    #[test]
    fn the_key_stays_out_of_debug_errors_and_files() {
        let (base, _) = mock(vec![status("autocomplete", 500)]);
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg(&base, d.path());
        assert!(!format!("{cfg:?}").contains("SENTINEL"));
        assert!(format!("{cfg:?}").contains("redacted"));
        nokey(&search(&cfg, "x").unwrap_err());
        save_match(&cfg, "k", &c(1, "Doom", false)).unwrap();
        let (hd, bd) = ok(&png());
        let (b2, _) = mock(vec![("/i", hd, bd)]);
        fetch_art(
            &cfg,
            &format!("{b2}/i"),
            &art_dir(&cfg, "k").unwrap().join("logo"),
        )
        .unwrap();
        // data dir files
        fn walk(p: &Path, hits: &mut u32) {
            for e in std::fs::read_dir(p).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, hits);
                } else if std::fs::read(&p)
                    .unwrap()
                    .windows(8)
                    .any(|w| w == b"SENTINEL")
                {
                    *hits += 1;
                }
            }
        }
        let mut hits = 0;
        walk(&cfg.data_dir, &mut hits);
        assert_eq!(hits, 0);
    }

    #[test]
    fn key_file_rules() {
        let d = tempfile::tempdir().unwrap();
        let f = |s: &str| {
            let p = d.path().join("k");
            std::fs::write(&p, s).unwrap();
            SgdbConfig::with_key_file(&p, DEFAULT_BASE.into(), d.path()).is_some()
        };
        assert!(f("abc123\n"));
        assert!(!f(""));
        assert!(!f("a b"));
        assert!(!f("key\nsecond"));
        assert!(
            SgdbConfig::with_key_file(&d.path().join("none"), DEFAULT_BASE.into(), d.path())
                .is_none()
        );
    }
}
