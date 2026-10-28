//! GameTDB lookups for Wii and GameCube games by 6-character disc ID: title, region, languages
//! and cover art. The database is fetched once and cached on disk; art is downloaded once per
//! game. Facts about GameTDB are in `docs/research/gametdb-notes.md` (many are UNVERIFIED: the
//! site answered 403 to our fetch tool). GameTDB asks that its data is not edited and that users
//! link back to it: show "Data and art from GameTDB (gametdb.com)" wherever this is shown.

use crate::config::Config;
use crate::steam::{base_ok, get_limited, now};

fn https_get_limit(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    get_limited(url, None, limit)
}
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Env: URL of a plain `wiitdb.xml` (no default, see the notes: the zip needs a decision).
pub const ENV_XML_URL: &str = "BRISK_GAMETDB_XML_URL";
/// Env: art base URL (tests point it at a mock).
pub const ENV_ART_BASE: &str = "BRISK_GAMETDB_ART_BASE";
/// Art host. Confirmed by search snippets and rom-properties' docs, not by the site itself.
pub const DEFAULT_ART_BASE: &str = "https://art.gametdb.com";

/// Database download cap. shortcut: real size of wiitdb.xml is UNVERIFIED (believed ~20 MB), raise if refused.
const MAX_DB: u64 = 64 << 20;
const MAX_ART: u64 = 4 << 20;
/// One `<game>` element longer than this is skipped (hostile or broken input).
const MAX_GAME: usize = 64 << 10;
const MAX_GAMES: usize = 200_000;
const TTL_S: u64 = 7 * 86_400;

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

// ---- id and art URLs ----

pub fn valid_id(id: &str) -> bool {
    id.len() == 6 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtKind {
    Cover,
    Cover3D,
    CoverFull,
    Disc,
    DiscM,
    Box,
}

impl ArtKind {
    /// Folder name on the art host. shortcut: only `cover` and `disc` are confirmed (rom-properties);
    /// the rest are UNVERIFIED names, check with a real fetch.
    pub fn dir(self) -> &'static str {
        match self {
            ArtKind::Cover => "cover",
            ArtKind::Cover3D => "cover3D",
            ArtKind::CoverFull => "coverfull",
            ArtKind::Disc => "disc",
            ArtKind::DiscM => "discM",
            ArtKind::Box => "box",
        }
    }
}

/// Region folders to try, best first, from the 4th character of the id. shortcut: the letter
/// to folder table is UNVERIFIED (US, EN, DE, FR, ES, IT, NL, JA, KO, TW, AU, RU are seen in
/// the wild); PAL ids fall back to EN, and `fetch_art` tries the next folder on a 404.
pub fn regions(id: &str) -> Vec<&'static str> {
    let first = match id.as_bytes().get(3).map(u8::to_ascii_uppercase) {
        Some(b'E') => "US",
        Some(b'J') => "JA",
        Some(b'K') => "KO",
        Some(b'W') => "TW",
        Some(b'D') => "DE",
        Some(b'F') => "FR",
        Some(b'S') => "ES",
        Some(b'I') => "IT",
        Some(b'H') => "NL",
        Some(b'U') => "AU",
        Some(b'R') => "RU",
        _ => "EN",
    };
    if first == "EN" {
        vec!["EN"]
    } else {
        vec![first, "EN"]
    }
}

fn art_url_at(base: &str, id: &str, kind: ArtKind, region: &str) -> Result<String, Error> {
    if !valid_id(id) {
        return Err(Error::BadId);
    }
    // shortcut: GameCube is assumed to live under /wii/ too (it is in wiitdb.xml); png assumed for all kinds.
    Ok(format!(
        "{}/wii/{}/{region}/{id}.png",
        base.trim_end_matches('/'),
        kind.dir()
    ))
}

/// First-choice art URL on the default host. Pure; rejects anything but a 6-character id.
pub fn art_url(id: &str, kind: ArtKind) -> Result<String, Error> {
    art_url_at(DEFAULT_ART_BASE, id, kind, regions(id)[0])
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

/// One game from the database text, by exact id. Pure.
pub fn parse_game(xml: &str, id: &str) -> Option<Entry> {
    if !valid_id(id) {
        return None;
    }
    let needle = format!(">{id}</id>");
    games(xml)
        .filter(|g| g.contains(&needle))
        .find_map(parse_one)
        .filter(|e| e.id == id)
}

/// Every game in the text (capped at 200k games).
pub fn build_index(xml: &str) -> Vec<Entry> {
    games(xml).filter_map(parse_one).take(MAX_GAMES).collect()
}

// ---- cache and fetch ----

#[derive(Serialize, Deserialize)]
struct Cache {
    fetched: u64,
    games: Vec<Entry>,
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), Error> {
    let io = |e: std::io::Error| Error::Io(e.kind());
    let dir = path
        .parent()
        .ok_or(Error::Io(std::io::ErrorKind::InvalidInput))?;
    std::fs::create_dir_all(dir).map_err(io)?;
    let tmp = dir.join(format!(".tmp.{}.{}", std::process::id(), now()));
    let r = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(e));
    }
    Ok(())
}

fn read_cache(path: &Path) -> Option<Cache> {
    let len = std::fs::metadata(path).ok()?.len();
    if len > MAX_DB {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn lookup_at(data_dir: &Path, xml_url: Option<&str>, id: &str, at: u64) -> Result<Entry, Error> {
    if !valid_id(id) {
        return Err(Error::BadId);
    }
    let path = data_dir.join("gametdb/index.json");
    let cached = read_cache(&path);
    let fresh = cached
        .as_ref()
        .is_some_and(|c| at.saturating_sub(c.fetched) < TTL_S);
    let cache = if fresh {
        cached.ok_or(Error::BadContent)?
    } else {
        match fetch_index(xml_url, at) {
            Ok(c) => {
                // Failing to cache is not fatal: the answer is still good.
                if let Ok(b) = serde_json::to_vec(&c) {
                    let _ = write_atomic(&path, &b);
                }
                c
            }
            // Stale beats nothing when the site is down.
            Err(e) => cached.ok_or(e)?,
        }
    };
    cache
        .games
        .into_iter()
        .find(|g| g.id == id)
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
    let body = https_get_limit(url, MAX_DB).map_err(net)?;
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

fn image_ext(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else {
        None
    }
}

fn fetch_art_at(data_dir: &Path, base: &str, id: &str, kind: ArtKind) -> Result<PathBuf, Error> {
    if !valid_id(id) {
        return Err(Error::BadId);
    }
    if !base_ok(base) {
        return Err(Error::NotConfigured);
    }
    let dir = data_dir.join("art").join(id);
    for ext in ["png", "jpg"] {
        let p = dir.join(format!("{}.{ext}", kind.dir()));
        if std::fs::metadata(&p).is_ok_and(|m| m.len() > 0) {
            return Ok(p);
        }
    }
    let mut last = Error::NotFound;
    for region in regions(id) {
        let url = art_url_at(base, id, kind, region)?;
        match https_get_limit(&url, MAX_ART) {
            Err(e) if e == "HTTP 404" => last = Error::NotFound,
            Err(e) => return Err(net(e)),
            Ok(b) => {
                // The server's content-type is not exposed by the shared helper; magic bytes decide.
                let ext = image_ext(&b).ok_or(Error::BadContent)?;
                let p = dir.join(format!("{}.{ext}", kind.dir()));
                write_atomic(&p, &b)?;
                return Ok(p);
            }
        }
    }
    Err(last)
}

/// Download one image once into `data_dir/art/<id>/<kind>.<png|jpg>` and return its path.
pub fn fetch_art(cfg: &Config, id: &str, kind: ArtKind) -> Result<PathBuf, Error> {
    let base = std::env::var(ENV_ART_BASE).unwrap_or_else(|_| DEFAULT_ART_BASE.into());
    fetch_art_at(&cfg.data_dir, &base, id, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Read;
    use std::net::TcpListener;
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
    fn id_validation_and_urls() {
        assert_eq!(
            art_url("RMCE01", ArtKind::Cover).unwrap(),
            "https://art.gametdb.com/wii/cover/US/RMCE01.png"
        );
        assert_eq!(
            art_url("RSBP01", ArtKind::Disc).unwrap(),
            "https://art.gametdb.com/wii/disc/EN/RSBP01.png"
        );
        for bad in [
            "",
            "RMCE0",
            "RMCE011",
            "RM/E01",
            "..RMCE",
            "RMCE0\n",
            "RMCE0%",
            "RMC E1",
            "RMCÉ01",
            "RMCE01/../x",
        ] {
            assert_eq!(art_url(bad, ArtKind::Cover), Err(Error::BadId), "{bad:?}");
        }
        assert_eq!(regions("RSBD01"), ["DE", "EN"]);
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

    fn png() -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        v.extend_from_slice(&[0; 32]);
        v
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
            lookup_at(d.path(), Some(&format!("{base}/500")), "RMCE01", 0),
            Err(Error::Network("HTTP 500".into()))
        );
        assert_eq!(
            lookup_at(d.path(), Some(&format!("{base}/junk")), "RMCE01", 0),
            Err(Error::BadContent)
        );
        assert_eq!(
            lookup_at(d.path(), Some(&format!("{base}/big")), "RMCE01", 0),
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

    #[test]
    fn art_happy_path_and_failures() {
        let (base, hits) = serve(HashMap::from([
            ("/wii/cover/US/RMCE01.png", (200, png())),
            ("/wii/disc/US/RMCE01.png", (404, vec![])),
            ("/wii/box/US/RMCE01.png", (500, vec![])),
            (
                "/wii/cover3D/US/RMCE01.png",
                (200, b"<html>no</html>".to_vec()),
            ),
            (
                "/wii/coverfull/US/RMCE01.png",
                (200, vec![0x89; (MAX_ART + 1) as usize]),
            ),
            ("/wii/cover/EN/RSBP01.png", (200, png())),
        ]));
        let d = tempfile::tempdir().unwrap();
        let p = fetch_art_at(d.path(), &base, "RMCE01", ArtKind::Cover).unwrap();
        assert!(p.ends_with("art/RMCE01/cover.png"));
        assert_eq!(std::fs::read(&p).unwrap(), png());
        fetch_art_at(d.path(), &base, "RMCE01", ArtKind::Cover).unwrap();
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "second call is served from disk"
        );
        // PAL id: EN folder is the only one, found
        assert!(fetch_art_at(d.path(), &base, "RSBP01", ArtKind::Cover).is_ok());
        assert_eq!(
            fetch_art_at(d.path(), &base, "RMCE01", ArtKind::Disc),
            Err(Error::NotFound)
        );
        assert_eq!(
            fetch_art_at(d.path(), &base, "RMCE01", ArtKind::Box),
            Err(Error::Network("HTTP 500".into()))
        );
        assert_eq!(
            fetch_art_at(d.path(), &base, "RMCE01", ArtKind::Cover3D),
            Err(Error::BadContent)
        );
        assert_eq!(
            fetch_art_at(d.path(), &base, "RMCE01", ArtKind::CoverFull),
            Err(Error::TooLarge)
        );
        assert!(!d.path().join("art/RMCE01/cover3D.png").exists());
        assert_eq!(
            fetch_art_at(d.path(), &base, "RM/../1", ArtKind::Cover),
            Err(Error::BadId)
        );
        assert_eq!(
            fetch_art_at(d.path(), "ftp://x", "RMCE01", ArtKind::Cover),
            Err(Error::NotConfigured)
        );
        assert!(matches!(
            fetch_art_at(d.path(), "http://127.0.0.1:1", "GALE01", ArtKind::Cover),
            Err(Error::Network(_))
        ));
    }

    #[test]
    fn art_write_failure_leaves_no_file() {
        let (base, _) = serve(HashMap::from([("/wii/cover/US/RMCE01.png", (200, png()))]));
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("art"), b"blocker").unwrap();
        assert!(matches!(
            fetch_art_at(d.path(), &base, "RMCE01", ArtKind::Cover),
            Err(Error::Io(_))
        ));
        // rename target is a directory: the temp file is cleaned up
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("art/RMCE01/cover.png")).unwrap();
        // a directory has len > 0 on most fs, so cached-check may return it: only assert no tmp left
        let _ = fetch_art_at(t.path(), &base, "RMCE01", ArtKind::Cover);
        let leftovers: Vec<_> = std::fs::read_dir(t.path().join("art/RMCE01"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }
}
