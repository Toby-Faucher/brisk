//! GameTDB lookups for Wii and GameCube games by 6-character disc ID: title, region, languages
//! and credits. The database is fetched once and cached on disk. Facts about GameTDB are in
//! `docs/research/gametdb-notes.md` (many are UNVERIFIED: the site answered 403 to our fetch tool). GameTDB asks that its data is not edited and that users
//! link back to it: show "Data and art from GameTDB (gametdb.com)" wherever this is shown.

use crate::config::Config;
use crate::fsutil::write_atomic;
use crate::steam::{base_ok, get_limited_for, now};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Env: URL of a plain `wiitdb.xml` (no default, see the notes: the zip needs a decision).
pub(crate) const ENV_XML_URL: &str = "BRISK_GAMETDB_XML_URL";
/// Database download cap. shortcut: real size of wiitdb.xml is UNVERIFIED (believed ~20 MB), raise if refused.
const MAX_DB: u64 = 64 << 20;
/// One `<game>` element longer than this is skipped (hostile or broken input).
const MAX_GAME: usize = 64 << 10;
const MAX_GAMES: usize = 200_000;
const TTL_S: u64 = 7 * 86_400;
/// After a failed database download nobody tries again for this long.
const FAIL_COOLDOWN_S: u64 = 3600;
/// The database is 20 MB or more; the shared helper's 15 s total limit cannot fetch it.
const DB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The id is not exactly 6 ASCII alphanumerics.
    BadId,
    /// No database URL configured, or the URL is not https (or loopback http).
    NotConfigured,
    /// The configured source is a zip; we cannot inflate it (no pure-Rust inflate in the lockfile).
    ZipUnsupported,
    NotFound,
    /// Download failed: only the kind of failure ("HTTP 500", "timed out"), never the URL.
    Network(String),
    TooLarge,
    /// Not the file type we expected (bad magic bytes, unparsable cache).
    BadContent,
    Io(std::io::ErrorKind),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BadId => f.write_str("not a 6-character disc id"),
            Error::NotConfigured => f.write_str("GameTDB source not configured"),
            Error::ZipUnsupported => {
                f.write_str("GameTDB zip source is not supported, use wiitdb.xml")
            }
            Error::NotFound => f.write_str("not in the GameTDB database"),
            Error::Network(s) => write!(f, "GameTDB download failed: {s}"),
            Error::TooLarge => f.write_str("GameTDB download too large"),
            Error::BadContent => f.write_str("GameTDB data not in the expected format"),
            Error::Io(k) => write!(f, "cache i/o error ({k:?})"),
        }
    }
}
impl std::error::Error for Error {}

fn net(msg: String) -> Error {
    if msg == "response too large" {
        Error::TooLarge
    } else {
        Error::Network(msg)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// The `name` attribute of the game element.
    pub name: String,
    /// As in the database, e.g. "NTSC-U", "PAL".
    pub region: String,
    pub languages: Vec<String>,
    /// Language code (as in the database, upper case) to title.
    pub titles: BTreeMap<String, String>,
    pub developer: String,
    pub publisher: String,
    pub year: Option<u16>,
}

impl Entry {
    /// Title in `lang` (e.g. "EN"), else English, else any, else the game name.
    pub fn title(&self, lang: &str) -> &str {
        let l = lang.to_ascii_uppercase();
        self.titles
            .get(&l)
            .or_else(|| self.titles.get("EN"))
            .or_else(|| self.titles.values().next())
            .map_or(self.name.as_str(), |s| s.as_str())
    }
}

// ---- id ----

/// True for a six character GameTDB id.
pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == 6 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

// ---- wiitdb.xml scanner ----

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let rep = end.and_then(|e| match &rest[1..e] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            n => n
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| n.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32)
                .filter(|c| !c.is_control()),
        });
        match (rep, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text between `<tag ...>` and `</tag>`, first match. Tolerant: no nesting support.
fn element<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = s[from..].find(&open) {
        let at = from + i + open.len();
        let next = s.as_bytes().get(at).copied();
        if matches!(next, Some(b'>' | b' ' | b'/' | b'\t' | b'\n' | b'\r')) {
            let gt = s[at..].find('>')? + at;
            if s[..gt].ends_with('/') {
                return Some("");
            }
            let close = s[gt..].find(&format!("</{tag}>"))? + gt;
            return Some(&s[gt + 1..close]);
        }
        from = at;
    }
    None
}

/// Value of `attr="..."` (or single quotes) inside the opening tag text `tag`.
fn attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = tag[from..].find(name) {
        let at = from + i;
        let before = tag[..at].chars().last();
        let after = &tag[at + name.len()..];
        from = at + name.len();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let after = after.trim_start().strip_prefix('=')?.trim_start();
        let q = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let body = &after[1..];
        return body.find(q).map(|e| unescape(&body[..e]));
    }
    None
}

fn clean(s: &str) -> String {
    unescape(s.trim())
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

fn parse_one(g: &str) -> Option<Entry> {
    let open_end = g.find('>')?;
    let id = clean(element(g, "id")?);
    if !valid_id(&id) {
        return None;
    }
    let mut e = Entry {
        id,
        name: attr(&g[..open_end], "name").unwrap_or_default(),
        region: element(g, "region").map(clean).unwrap_or_default(),
        languages: element(g, "languages")
            .map(|l| {
                l.split(',')
                    .map(|x| clean(x).to_ascii_uppercase())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        developer: element(g, "developer").map(clean).unwrap_or_default(),
        publisher: element(g, "publisher").map(clean).unwrap_or_default(),
        ..Entry::default()
    };
    let mut rest = g;
    while let Some(i) = rest.find("<locale") {
        rest = &rest[i + 7..];
        let Some(gt) = rest.find('>') else { break };
        let Some(close) = rest.find("</locale>") else {
            break;
        };
        if close > gt
            && let (Some(lang), Some(t)) = (
                attr(&rest[..gt], "lang"),
                element(&rest[gt..close], "title"),
            )
        {
            e.titles.insert(clean(&lang).to_ascii_uppercase(), clean(t));
        }
        rest = &rest[close.max(gt)..];
    }
    // <date year="2008" month="" day=""/>
    if let Some(i) = g.find("<date") {
        let tag = &g[i..];
        let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
        e.year = attr(tag, "year").and_then(|y| y.parse().ok());
    }
    Some(e)
}

/// Iterate `<game ...>...</game>` slices without copying; oversized ones are skipped.
fn games(xml: &str) -> impl Iterator<Item = &str> {
    let mut rest = xml;
    std::iter::from_fn(move || {
        loop {
            let i = rest.find("<game")?;
            let after = rest.as_bytes().get(i + 5).copied();
            if !matches!(after, Some(b' ' | b'>' | b'\t' | b'\n' | b'\r')) {
                rest = &rest[i + 5..];
                continue;
            }
            let Some(len) = rest[i..].find("</game>") else {
                rest = ""; // unterminated: nothing more can be trusted
                return None;
            };
            let g = &rest[i..i + len + 7];
            rest = &rest[i + len + 7..];
            if g.len() <= MAX_GAME {
                return Some(g);
            }
        }
    })
}

/// Every game in the text (capped at 200k games).
fn build_index(xml: &str) -> Vec<Entry> {
    games(xml).filter_map(parse_one).take(MAX_GAMES).collect()
}

// ---- cache and fetch ----

#[derive(Serialize, Deserialize)]
struct Cache {
    fetched: u64,
    games: Vec<Entry>,
}

#[cfg(test)]
thread_local! { static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

type Mem = Vec<(PathBuf, (u64, std::time::SystemTime), Arc<Cache>)>;
static MEM: Mutex<Mem> = Mutex::new(Vec::new());

/// The parsed index; re-read only when the file's size or modified time changed.
fn read_cache(path: &Path) -> Option<Arc<Cache>> {
    let m = std::fs::metadata(path).ok()?;
    if m.len() > MAX_DB {
        return None;
    }
    let key = (m.len(), m.modified().ok()?);
    let mut mem = MEM.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, c)) = mem.iter().find(|(p, k, _)| p == path && *k == key) {
        return Some(c.clone());
    }
    #[cfg(test)]
    PARSES.with(|n| n.set(n.get() + 1));
    let c: Arc<Cache> = Arc::new(serde_json::from_slice(&std::fs::read(path).ok()?).ok()?);
    // one path in real use; the few slots only keep tests apart
    mem.retain(|(p, ..)| p != path);
    mem.truncate(7);
    mem.push((path.to_path_buf(), key, c.clone()));
    Some(c)
}

fn failed_recently(dir: &Path, at: u64) -> bool {
    std::fs::read_to_string(dir.join("fetch-failed"))
        .ok()
        .and_then(|t| t.trim().parse::<u64>().ok())
        .is_some_and(|t| at.saturating_sub(t) < FAIL_COOLDOWN_S)
}

fn lookup_at(data_dir: &Path, xml_url: Option<&str>, id: &str, at: u64) -> Result<Entry, Error> {
    if !valid_id(id) {
        return Err(Error::BadId);
    }
    let path = data_dir.join("gametdb/index.json");
    let cached = read_cache(&path);
    let cache = if let Some(c) = cached
        .clone()
        .filter(|c| at.saturating_sub(c.fetched) < TTL_S)
    {
        c
    } else if failed_recently(&data_dir.join("gametdb"), at) {
        // The last download failed lately: stale (or nothing) until the cool-down ends.
        cached.ok_or_else(|| Error::Network("database download failed lately".into()))?
    } else {
        match fetch_index(xml_url, at) {
            Ok(c) => {
                let _ = std::fs::remove_file(data_dir.join("gametdb/fetch-failed"));
                // Failing to cache is not fatal: the answer is still good.
                if let Ok(b) = serde_json::to_vec(&c) {
                    let _ = write_atomic(&path, &b);
                }
                Arc::new(c)
            }
            Err(e) => {
                if matches!(e, Error::Network(_) | Error::TooLarge | Error::BadContent) {
                    let _ = write_atomic(
                        &data_dir.join("gametdb/fetch-failed"),
                        at.to_string().as_bytes(),
                    );
                }
                // Stale beats nothing when the site is down.
                cached.ok_or(e)?
            }
        }
    };
    cache
        .games
        .iter()
        .find(|g| g.id == id)
        .cloned()
        .ok_or(Error::NotFound)
}

fn fetch_index(xml_url: Option<&str>, at: u64) -> Result<Cache, Error> {
    let url = xml_url.filter(|u| base_ok(u)).ok_or(Error::NotConfigured)?;
    if url
        .split(['?', '#'])
        .next()
        .is_some_and(|p| p.ends_with(".zip"))
    {
        return Err(Error::ZipUnsupported);
    }
    let body = get_limited_for(url, None, MAX_DB, DB_TIMEOUT).map_err(net)?;
    let text = String::from_utf8_lossy(&body);
    let games = build_index(&text);
    if games.is_empty() {
        return Err(Error::BadContent);
    }
    Ok(Cache { fetched: at, games })
}

/// Title, region, languages and credits for a disc id. The first call fetches the database from
/// `BRISK_GAMETDB_XML_URL`; later calls read `data_dir/gametdb/index.json` until it is 7 days old.
pub fn lookup(cfg: &Config, id: &str) -> Result<Entry, Error> {
    lookup_at(
        &cfg.data_dir,
        std::env::var(ENV_XML_URL).ok().as_deref(),
        id,
        now(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// One game from the database text, by exact id.
    fn parse_game(xml: &str, id: &str) -> Option<Entry> {
        if !valid_id(id) {
            return None;
        }
        let needle = format!(">{id}</id>");
        games(xml)
            .filter(|g| g.contains(&needle))
            .find_map(parse_one)
            .filter(|e| e.id == id)
    }
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<WiiTDB version="20261028" games="3">
  <game name="Mario Kart Wii (USA)"><id>RMCE01</id><type/><region>NTSC-U</region><languages>EN,FR,ES</languages>
    <locale lang="EN"><title>Mario Kart Wii</title><synopsis>Race &amp; win.</synopsis></locale>
    <developer>Nintendo EAD</developer><publisher>Nintendo</publisher><date year="2008" month="4" day="10"/></game>
  <game name="Super Smash Bros. Brawl (Europe)"><id>RSBP01</id><region>PAL</region><languages>EN,FR,DE,ES,IT,NL</languages>
    <locale lang="EN"><title>Super Smash Bros. Brawl</title></locale>
    <locale lang="DE"><title>Super Smash Bros. Brawl &#xDC;</title></locale>
    <publisher>Nintendo</publisher><date year="2008" month="6" day="27"/></game>
  <game name="Metroid Prime"><id>GM8E01</id><region>NTSC-U</region><languages>EN</languages>
    <locale lang="EN"><title>Metroid Prime</title></locale><developer>Retro Studios</developer></game>
</WiiTDB>"#;

    #[test]
    fn parses_hits_and_misses() {
        let m = parse_game(XML, "RMCE01").unwrap();
        assert_eq!((m.region.as_str(), m.year), ("NTSC-U", Some(2008)));
        assert_eq!(m.title("EN"), "Mario Kart Wii");
        assert_eq!(m.languages, ["EN", "FR", "ES"]);
        assert_eq!(
            (m.developer.as_str(), m.publisher.as_str()),
            ("Nintendo EAD", "Nintendo")
        );
        let p = parse_game(XML, "RSBP01").unwrap();
        assert_eq!(p.titles.len(), 2);
        assert_eq!(p.title("de"), "Super Smash Bros. Brawl \u{dc}");
        assert_eq!(p.title("ja"), "Super Smash Bros. Brawl");
        let gc = parse_game(XML, "GM8E01").unwrap();
        assert_eq!((gc.name.as_str(), gc.year), ("Metroid Prime", None));
        assert!(parse_game(XML, "ZZZZ99").is_none());
        assert!(parse_game(XML, "RMCE0").is_none());
        assert_eq!(build_index(XML).len(), 3);
    }

    #[test]
    fn hostile_xml_does_not_panic_or_blow_up() {
        let huge = format!(
            "<game name=\"{}\"><id>AAAAAA</id></game>",
            "x".repeat(MAX_GAME + 10)
        );
        assert!(build_index(&huge).is_empty());
        for bad in [
            "<game name=\"a\"><id>AAAAAA</id>",       // unterminated element
            "<game name=\"a\"><id>AAAAAA</id",        // unterminated tag
            "<game <game <game </game>",              // nested garbage
            "<game name=\"a><id>AAAAAA</id></game>",  // unterminated attribute
            "<games><id>AAAAAA</id></games>",         // not a game element
            "<game><id>AAAA&#99999999;A</id></game>", // bad numeric entity
            "<game><id>&amp;&amp;&amp;</id></game>",
            "<game><id>AAAAAA</id><locale lang=></locale><locale></game>",
            "<game><id>AAAAAA</id><date year=\"99999999999\"/></game>",
        ] {
            let _ = build_index(bad);
            let _ = parse_game(bad, "AAAAAA");
        }
        // Entity oddities: unknown entity stays literal, a lone ampersand survives.
        let e = parse_game(
            "<game name=\"A &bogus; & B\"><id>AAAAAA</id></game>",
            "AAAAAA",
        )
        .unwrap();
        assert_eq!(e.name, "A &bogus; & B");
        // An id with markup never matches.
        assert!(parse_game(XML, "RMCE01</id><id>").is_none());
    }

    #[test]
    fn ids_are_six_plain_characters() {
        assert!(valid_id("RMCE01"));
        for bad in [
            "", "RMCE0", "RMCE011", "RM/E01", "..RMCE", "RMCE0\n", "RMC E1", "RMCÉ01",
        ] {
            assert!(!valid_id(bad), "{bad:?}");
        }
    }

    type Routes = HashMap<&'static str, (u16, Vec<u8>)>;

    /// Tiny HTTP/1.1 mock on loopback; returns its base URL and a hit counter.
    fn serve(routes: Routes) -> (String, Arc<AtomicUsize>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("");
                h.fetch_add(1, Ordering::SeqCst);
                let (code, body) = routes.get(path).cloned().unwrap_or((404, vec![]));
                let _ = write!(
                    s,
                    "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(&body);
            }
        });
        (base, hits)
    }

    #[test]
    fn lookup_fetches_once_caches_and_expires() {
        let (base, hits) = serve(HashMap::from([(
            "/wiitdb.xml",
            (200, XML.as_bytes().to_vec()),
        )]));
        let url = format!("{base}/wiitdb.xml");
        let d = tempfile::tempdir().unwrap();
        let e = lookup_at(d.path(), Some(&url), "RSBP01", 1000).unwrap();
        assert_eq!(e.region, "PAL");
        assert_eq!(
            lookup_at(d.path(), Some(&url), "GM8E01", 2000)
                .unwrap()
                .name,
            "Metroid Prime"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        // unknown id, served from the cache
        assert_eq!(
            lookup_at(d.path(), Some(&url), "AAAAAA", 2000),
            Err(Error::NotFound)
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        // expiry refetches
        lookup_at(d.path(), Some(&url), "RMCE01", 1000 + TTL_S + 1).unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(d.path().join("gametdb/index.json")).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
        // stale cache is used when the refresh fails
        let dead = "http://127.0.0.1:1/wiitdb.xml";
        assert!(lookup_at(d.path(), Some(dead), "RMCE01", 1000 + 3 * TTL_S).is_ok());
    }

    #[test]
    fn a_failed_download_is_not_retried_per_game_and_the_index_is_parsed_once() {
        let (base, hits) = serve(HashMap::from([
            ("/ok", (200, XML.as_bytes().to_vec())),
            ("/500", (500, vec![])),
        ]));
        let d = tempfile::tempdir().unwrap();
        lookup_at(d.path(), Some(&format!("{base}/ok")), "RSBP01", 1000).unwrap();
        let p0 = PARSES.with(std::cell::Cell::get);
        let dead = format!("{base}/500");
        let at = 1000 + 2 * TTL_S;
        for id in ["RSBP01", "GM8E01", "RMCE01", "RSBP01"] {
            let _ = lookup_at(d.path(), Some(&dead), id, at);
        }
        // one failed download (after the one good one), the stale cache answered, parsed once
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert!(PARSES.with(std::cell::Cell::get) - p0 <= 1);
        // after the cool-down it tries again; no cache and a recent failure is an error, not a fetch
        let _ = lookup_at(d.path(), Some(&dead), "RSBP01", at + FAIL_COOLDOWN_S + 1);
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        let e = tempfile::tempdir().unwrap();
        assert!(lookup_at(e.path(), Some(&dead), "RSBP01", 5).is_err());
        assert!(lookup_at(e.path(), Some(&dead), "GM8E01", 6).is_err());
        assert_eq!(hits.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn lookup_errors() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            lookup_at(d.path(), Some("http://127.0.0.1:1/x.xml"), "../etc/x", 0),
            Err(Error::BadId)
        );
        assert_eq!(
            lookup_at(d.path(), None, "RMCE01", 0),
            Err(Error::NotConfigured)
        );
        assert_eq!(
            lookup_at(d.path(), Some("http://example.com/x.xml"), "RMCE01", 0),
            Err(Error::NotConfigured)
        );
        assert_eq!(
            lookup_at(
                d.path(),
                Some("https://example.com/wiitdb.zip"),
                "RMCE01",
                0
            ),
            Err(Error::ZipUnsupported)
        );
        // unreachable: connection refused, error text has no URL
        let e = lookup_at(
            d.path(),
            Some("http://127.0.0.1:1/secret-path.xml"),
            "RMCE01",
            0,
        )
        .unwrap_err();
        assert!(matches!(e, Error::Network(_)));
        assert!(!e.to_string().contains("secret-path"));
        let (base, _) = serve(HashMap::from([
            ("/500", (500, vec![])),
            ("/junk", (200, b"<html>not a db</html>".to_vec())),
            ("/big", (200, vec![b'x'; (MAX_DB + 1) as usize])),
        ]));
        assert_eq!(
            lookup_at(
                tempfile::tempdir().unwrap().path(),
                Some(&format!("{base}/500")),
                "RMCE01",
                0
            ),
            Err(Error::Network("HTTP 500".into()))
        );
        assert_eq!(
            lookup_at(
                tempfile::tempdir().unwrap().path(),
                Some(&format!("{base}/junk")),
                "RMCE01",
                0
            ),
            Err(Error::BadContent)
        );
        assert_eq!(
            lookup_at(
                tempfile::tempdir().unwrap().path(),
                Some(&format!("{base}/big")),
                "RMCE01",
                0
            ),
            Err(Error::TooLarge)
        );
    }

    #[test]
    fn lookup_survives_cache_write_failure() {
        let (base, _) = serve(HashMap::from([("/db.xml", (200, XML.as_bytes().to_vec()))]));
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("gametdb"), b"a file where the dir should be").unwrap();
        assert!(lookup_at(d.path(), Some(&format!("{base}/db.xml")), "RMCE01", 0).is_ok());
    }
}
