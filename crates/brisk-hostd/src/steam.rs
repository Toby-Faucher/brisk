//! Steam library: installed games from local manifests, owned games from `GetOwnedGames`.
//! The API key is read from `cfg.steam_key_file` per fetch, never stored or logged.

use crate::config::Config;
pub use crate::game::Game;
use crate::game::GameId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const OWNED_TTL_S: u64 = 6 * 3600;

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GamesResult {
    pub games: Vec<Game>,
    pub owned_available: bool,
    /// 1: games carry string `id`s and the id routes work. An older hostd leaves it out.
    pub id_scheme: u32,
}

impl GamesResult {
    /// The game with this id.
    pub fn find(&self, id: &GameId) -> Option<&Game> {
        self.games.iter().find(|g| g.id == *id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub appid: u32,
    pub name: String,
    pub size: u64,
    pub state_flags: u32,
    pub last_played: u64,
    pub update: Option<String>,
}

// ---- VDF / ACF ----

enum V {
    S(String),
    M(Vec<(String, V)>),
}

fn tokens(text: &str) -> Vec<String> {
    // Quoted strings (with \ escapes) and bare `{` / `}`; `//` comments are skipped.
    let mut out = vec![];
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' | '}' => out.push(c.to_string()),
            '/' if it.peek() == Some(&'/') => {
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                let mut s = String::new();
                while let Some(c) = it.next() {
                    match c {
                        '"' => break,
                        '\\' => match it.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(o) => s.push(o),
                            None => break,
                        },
                        _ => s.push(c),
                    }
                }
                // Marked so a quoted "{" is not a brace.
                out.push(format!("\"{s}"));
            }
            _ => {}
        }
    }
    out
}

fn block(t: &[String], i: &mut usize, depth: u8) -> Option<Vec<(String, V)>> {
    let mut m = vec![];
    while *i < t.len() {
        let k = &t[*i];
        if k == "}" {
            *i += 1;
            return Some(m);
        }
        let key = k.strip_prefix('"')?.to_string();
        *i += 1;
        let v = t.get(*i)?;
        if v == "{" {
            *i += 1;
            if depth > 16 {
                return None;
            }
            m.push((key, V::M(block(t, i, depth + 1)?)));
        } else {
            m.push((key, V::S(v.strip_prefix('"')?.to_string())));
            *i += 1;
        }
    }
    // Ran out of input: only the top level may end without a `}`.
    (depth == 0).then_some(m)
}

fn parse_vdf(text: &str) -> Option<Vec<(String, V)>> {
    block(&tokens(text), &mut 0, 0)
}

fn get<'a>(m: &'a [(String, V)], k: &str) -> Option<&'a V> {
    m.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(k))
        .map(|(_, v)| v)
}

fn get_s<'a>(m: &'a [(String, V)], k: &str) -> Option<&'a str> {
    match get(m, k)? {
        V::S(s) => Some(s),
        V::M(_) => None,
    }
}

// Steam's EAppState bits. Seen on this host: only 4 (FullyInstalled). The other bit values, including the in-progress ones (256, 1024, 1048576, 2097152, 4194304),
// are from memory and unverified against a live update. 512 (UpdatePaused) counts as needed and wins over the running bits.
const FULLY_INSTALLED: u32 = 4;
const UPDATE_REQUIRED: u32 = 2;
const UPDATE_PAUSED: u32 = 512;
const UPDATE_RUNNING: u32 = 256 | 1024 | 1_048_576 | 2_097_152 | 4_194_304;

/// Only installed games get a state: an install in progress is shown by the install flow.
fn update_state(flags: u32) -> Option<String> {
    if flags & FULLY_INSTALLED == 0 {
        return None;
    }
    if flags & UPDATE_PAUSED != 0 {
        Some("needed".into())
    } else if flags & UPDATE_RUNNING != 0 {
        Some("running".into())
    } else if flags & UPDATE_REQUIRED != 0 {
        Some("needed".into())
    } else {
        None
    }
}

/// `None` for anything that is not a usable `appmanifest_*.acf` (empty, garbage, no appid or name).
pub fn parse_acf(text: &str) -> Option<Installed> {
    let top = parse_vdf(text)?;
    let V::M(a) = get(&top, "AppState")? else {
        return None;
    };
    let num = |k| get_s(a, k).and_then(|s| s.parse::<u64>().ok());
    let name = get_s(a, "name")?.trim();
    if name.is_empty() {
        return None;
    }
    let flags = num("StateFlags").unwrap_or(0) as u32;
    Some(Installed {
        appid: get_s(a, "appid")?.parse().ok()?,
        name: name.to_string(),
        size: num("SizeOnDisk").unwrap_or(0),
        state_flags: flags,
        last_played: num("LastPlayed").unwrap_or(0),
        update: update_state(flags),
    })
}

/// `steamapps` dirs: the main one plus every library listed in `libraryfolders.vdf`.
fn library_dirs(steam_dir: &Path) -> Vec<PathBuf> {
    let main = steam_dir.join("steamapps");
    let mut dirs = vec![main.clone()];
    let text = std::fs::read_to_string(main.join("libraryfolders.vdf")).unwrap_or_default();
    if let Some(top) = parse_vdf(&text)
        && let Some(V::M(folders)) = get(&top, "libraryfolders")
    {
        for (_, f) in folders {
            if let V::M(f) = f
                && let Some(p) = get_s(f, "path")
            {
                let d = Path::new(p).join("steamapps");
                if !dirs.contains(&d) {
                    dirs.push(d);
                }
            }
        }
    }
    dirs
}

fn installed_manifests(steam_dir: &Path) -> Vec<Installed> {
    let mut seen: HashMap<u32, Installed> = HashMap::new();
    for d in library_dirs(steam_dir) {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        for f in files {
            let n = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
                continue;
            }
            if let Some(i) = std::fs::read_to_string(&f)
                .ok()
                .as_deref()
                .and_then(parse_acf)
            {
                // Same appid in two libraries: prefer the fully installed one, else the first.
                let better = |o: &Installed| {
                    i.state_flags & FULLY_INSTALLED != 0 && o.state_flags & FULLY_INSTALLED == 0
                };
                if seen.get(&i.appid).is_none_or(better) {
                    seen.insert(i.appid, i);
                }
            }
        }
    }
    seen.into_values().collect()
}

// ---- owned games ----

#[derive(Serialize, Deserialize, Clone)]
struct Owned {
    appid: u32,
    name: String,
    playtime_forever: u64,
    #[serde(default)]
    playtime_2weeks: u64,
    #[serde(default)]
    rtime_last_played: u64,
}

#[derive(Serialize, Deserialize)]
struct OwnedCache {
    fetched: u64,
    games: Vec<Owned>,
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

const MAX_BODY: u64 = 8 << 20;

/// HTTPS only, except plain http to loopback (the tests' mock server).
pub(crate) fn base_ok(base: &str) -> bool {
    if base.starts_with("https://") {
        return true;
    }
    let Some(rest) = base.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split([':', '/']).next().unwrap_or("");
    host == "127.0.0.1" || host == "localhost"
}

/// Blocking GET with strict limits: connect 5 s, total 15 s, no redirects, no proxy, 8 MiB body,
/// certificates checked against the system roots (/etc/ssl/certs via rustls-platform-verifier).
/// Key safety: ureq logs every request through the `log` crate, and at trace level (and in some
/// debug lines) with the full path and query, i.e. `key=`. hostd installs no `log` logger, so
/// nothing is printed; never add one (env_logger, tracing-log, ...) without filtering ureq out.
pub(crate) fn https_get(url: &str) -> Result<Vec<u8>, String> {
    get_limited(url, None, MAX_BODY)
}

/// `https_get` with an optional bearer token (sent only as the Authorization header, never in the
/// URL) and a caller-chosen body cap. Errors still carry only the kind of failure.
pub(crate) fn get_limited(url: &str, bearer: Option<&str>, max: u64) -> Result<Vec<u8>, String> {
    get_limited_for(url, bearer, max, Duration::from_secs(15))
}

/// `get_limited` with its own total time limit (a database download needs minutes, not 15 s).
pub(crate) fn get_limited_for(
    url: &str,
    bearer: Option<&str>,
    max: u64,
    total: Duration,
) -> Result<Vec<u8>, String> {
    let agent = crate::net::agent(
        ureq::tls::RootCerts::PlatformVerifier,
        Some(Duration::from_secs(5)),
        total,
        true,
    );
    let mut req = agent.get(url);
    if let Some(t) = bearer {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let mut r = req.call().map_err(|e| crate::net::safe_err(&e))?;
    if r.status() != 200 {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    r.body_mut()
        .with_config()
        .limit(max)
        .read_to_vec()
        .map_err(|e| crate::net::safe_err(&e))
}

/// The key from its file; only plain key characters, so it cannot change the shape of the URL.
pub(crate) fn read_key(cfg: &Config) -> Result<String, &'static str> {
    let key = std::fs::read_to_string(&cfg.steam_key_file).map_err(|_| "no API key file")?;
    plain_key(&key)
        .map(str::to_string)
        .ok_or("API key file is not a plain key")
}

/// The key file's text, trimmed, when it is one non-empty run of `A-Za-z0-9_-`.
pub(crate) fn plain_key(raw: &str) -> Option<&str> {
    let key = raw.trim();
    let plain = key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (!key.is_empty() && plain).then_some(key)
}

/// A `steam_get` error that will not go away by waiting: Steam refused the key or the list is private
/// (the texts come from `net::safe_err` / `https_get` above).
pub(crate) fn is_refusal(err: &str) -> bool {
    matches!(err, "HTTP 401" | "HTTP 403")
}

/// One Steam Web API GET (`path_query` like `ISteamUser/GetPlayerSummaries/v2/?steamids=1,2`).
/// The key is read from its file here and goes only into the request's query string (that is how
/// Steam's Web API takes it); `format=json` is appended too. Errors never carry the URL.
pub(crate) async fn steam_get(cfg: &Config, path_query: &str) -> Result<serde_json::Value, String> {
    let base = cfg.steam_api_base.trim_end_matches('/');
    if !base_ok(base) {
        return Err("steam_api_base must be https (or http to loopback)".into());
    }
    let key = read_key(cfg)?;
    let sep = if path_query.contains('?') { '&' } else { '?' };
    json_get(format!("{base}/{path_query}{sep}key={key}&format=json")).await
}

/// One store.steampowered.com GET: keyless, so nothing to hide in the URL.
pub(crate) async fn store_get(cfg: &Config, path_query: &str) -> Result<serde_json::Value, String> {
    let base = cfg.steam_store_base.trim_end_matches('/');
    if !base_ok(base) {
        return Err("steam_store_base must be https (or http to loopback)".into());
    }
    json_get(format!("{base}/{path_query}")).await
}

async fn json_get(url: String) -> Result<serde_json::Value, String> {
    let body = tokio::task::spawn_blocking(move || https_get(&url))
        .await
        .map_err(|_| "request task failed")??;
    serde_json::from_slice(&body).map_err(|_| "response is not JSON".into())
}

/// One GetOwnedGames call.
async fn fetch_owned(cfg: &Config) -> Result<Vec<Owned>, String> {
    let id = steam_id(&cfg.steam_dir).ok_or("no Steam account id")?;
    let v = steam_get(
        cfg,
        &format!(
            "IPlayerService/GetOwnedGames/v1/?steamid={id}&include_appinfo=1&include_played_free_games=1"
        ),
    )
    .await?;
    v.get("response")
        .and_then(|r| r.get("games"))
        .and_then(|g| serde_json::from_value(g.clone()).ok())
        .ok_or_else(|| "response has no games list".into())
}

/// 64-bit SteamID to the 32-bit account id the store and some endpoints use.
pub(crate) fn account_id(steam_id64: &str) -> Option<u32> {
    steam_id64
        .parse::<u64>()
        .ok()?
        .checked_sub(76561197960265728)?
        .try_into()
        .ok()
}

/// Steam account id: the entry marked MostRecent, else (newer clients drop that field) the one
/// with the newest `timestamp`.
pub(crate) fn steam_id(steam_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(steam_dir.join("config/loginusers.vdf")).ok()?;
    let top = parse_vdf(&text)?;
    let V::M(users) = get(&top, "users")? else {
        return None;
    };
    let ts = |u: &[(String, V)]| get_s(u, "timestamp").and_then(|t| t.parse::<u64>().ok());
    users
        .iter()
        .filter_map(|(id, u)| match u {
            V::M(u) => Some((get_s(u, "MostRecent") == Some("1"), ts(u), id)),
            _ => None,
        })
        .max_by_key(|(recent, t, _)| (*recent, *t))
        .map(|(_, _, id)| id.clone())
        // It goes into the query string: digits only.
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

async fn owned(cfg: &Config, refresh: bool) -> (Vec<Owned>, bool) {
    let path = cfg.data_dir.join("owned.json");
    let cache: Option<OwnedCache> = tokio::fs::read(&path)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(c) = &cache
        && !refresh
        && now().saturating_sub(c.fetched) < OWNED_TTL_S
    {
        return (c.games.clone(), true);
    }
    match fetch_owned(cfg).await {
        Ok(games) => {
            let c = OwnedCache {
                fetched: now(),
                games,
            };
            // Atomic: other requests read this file while we write.
            if let Ok(b) = serde_json::to_vec(&c) {
                let _ = tokio::task::spawn_blocking(move || crate::fsutil::write_atomic(&path, &b))
                    .await;
            }
            (c.games, true)
        }
        // Stale cache is better than nothing, but the UI still says Steam was unreachable.
        Err(e) => {
            tracing::warn!("GetOwnedGames failed: {e}");
            (cache.map(|c| c.games).unwrap_or_default(), false)
        }
    }
}

/// `(cover, hero, logo)` URLs on Steam's CDN.
fn steam_art(id: u32) -> (String, String, String) {
    let art = |file: &str| format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/{file}");
    (
        art("library_600x900.jpg"),
        art("library_hero.jpg"),
        art("logo.png"),
    )
}

/// The games list, from cache where it can.
pub(crate) async fn load_games(cfg: &Config) -> GamesResult {
    load_games_with(cfg, false).await
}

/// The games list; `refresh` skips the owned games cache.
pub(crate) async fn load_games_with(cfg: &Config, refresh: bool) -> GamesResult {
    let steam_dir = cfg.steam_dir.clone();
    let installed = tokio::task::spawn_blocking(move || installed_manifests(&steam_dir))
        .await
        .unwrap_or_default();
    let (owned, owned_available) = owned(cfg, refresh).await;

    let mut by_id: HashMap<u32, Game> = HashMap::new();
    for o in owned {
        let mut g = Game::steam(o.appid, &o.name);
        g.last_played = (o.rtime_last_played > 0).then_some(o.rtime_last_played);
        g.playtime_min = o.playtime_forever;
        g.playtime2wk_min = o.playtime_2weeks;
        (g.cover, g.hero, g.logo) = steam_art(o.appid);
        by_id.insert(o.appid, g);
    }
    for i in installed {
        // Steam tools (runtimes, Proton) have manifests but are not games: the owned list is the
        // authority; without it, fall back to name prefixes.
        let tool = [
            "Steam Linux Runtime",
            "Proton ",
            "Proton Experimental",
            "Steamworks Common Redistributables",
        ]
        .iter()
        .any(|p| i.name.starts_with(p));
        let skip = if owned_available {
            !by_id.contains_key(&i.appid)
        } else {
            tool
        };
        if skip {
            continue;
        }
        let g = by_id.entry(i.appid).or_insert_with(|| {
            let mut g = Game::steam(i.appid, &i.name);
            (g.cover, g.hero, g.logo) = steam_art(i.appid);
            g
        });
        g.installed = i.state_flags & FULLY_INSTALLED != 0;
        g.update = i.update.clone();
        if g.installed {
            g.size_bytes = Some(i.size);
        }
        if i.last_played > g.last_played.unwrap_or(0) {
            g.last_played = Some(i.last_played);
        }
    }
    let proc = cfg.proc_dir.clone();
    let cfg2 = cfg.clone();
    let ids: Vec<u32> = by_id.keys().copied().collect();
    let ids2 = ids.clone();
    let played: Vec<(u32, u64)> = by_id
        .values()
        .filter(|g| g.installed)
        .filter_map(|g| Some((g.appid, g.last_played?)))
        .collect();
    let (running, counts, updated, controls) = tokio::task::spawn_blocking(move || {
        let counts: HashMap<u32, (u32, u32)> = ids
            .into_iter()
            .filter_map(|id| Some((id, crate::steam_info::ach_counts(&cfg2, id)?)))
            .collect();
        let controls: HashMap<u32, (Option<String>, bool)> = ids2
            .into_iter()
            .filter_map(|id| Some((id, crate::steam_info::cached_controls(&cfg2, id)?)))
            .collect();
        let t = now();
        let updated: std::collections::HashSet<u32> = played
            .into_iter()
            .filter(|&(id, lp)| crate::steam_info::cached_updated(&cfg2, id, lp, t))
            .map(|(id, _)| id)
            .collect();
        (
            crate::running::running_appids(&proc),
            counts,
            updated,
            controls,
        )
    })
    .await
    .unwrap_or_default();
    let mut games: Vec<Game> = by_id.into_values().collect();
    for g in &mut games {
        g.running = running.contains(&g.appid);
        g.updated = updated.contains(&g.appid);
        if let Some((c, co)) = controls.get(&g.appid) {
            (g.controller, g.coop) = (c.clone(), *co);
        }
        if let Some(&(u, t)) = counts.get(&g.appid) {
            (g.ach_unlocked, g.ach_total) = (Some(u), Some(t));
        }
    }
    let mut emu = crate::emu::games(
        &crate::emu::load(&cfg.data_dir),
        &cfg.proc_dir,
        &cfg.data_dir,
    );
    crate::ra_game::caps_on(cfg, &mut emu);
    games.extend(emu);
    games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    GamesResult {
        games,
        owned_available,
        id_scheme: 1,
    }
}

#[cfg(test)]
mod steam_id_tests {
    use super::steam_id;

    #[test]
    fn hostile_id_is_rejected() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("config")).unwrap();
        let vdf = "\"users\"\n{\n\t\"1\\\"\\nurl=x\"\n\t{\n\t\t\"timestamp\"\t\"5\"\n\t}\n}\n";
        std::fs::write(d.join("config/loginusers.vdf"), vdf).unwrap();
        assert_eq!(steam_id(d), None);
    }

    #[test]
    fn newest_timestamp_when_no_mostrecent() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        std::fs::create_dir_all(d.join("config")).unwrap();
        let u = |id: &str, t: u32| format!("\t\"{id}\"\n\t{{\n\t\t\"timestamp\"\t\"{t}\"\n\t}}\n");
        let vdf = format!("\"users\"\n{{\n{}{}}}\n", u("111", 5), u("222", 9));
        std::fs::write(d.join("config/loginusers.vdf"), vdf).unwrap();
        assert_eq!(steam_id(d).as_deref(), Some("222"));
    }
}

#[cfg(test)]
mod fetch_tests {
    use super::*;

    const KEY: &str = "SENTINELKEY-feedfacecafe";

    fn cfg_with(base: &str, d: &Path) -> Config {
        std::fs::create_dir_all(d.join("steam/config")).unwrap();
        std::fs::write(
            d.join("steam/config/loginusers.vdf"),
            "\"users\"\n{\n\t\"76561198000000000\"\n\t{\n\t\t\"MostRecent\"\t\"1\"\n\t}\n}\n",
        )
        .unwrap();
        std::fs::write(d.join("key"), format!("{KEY}\n")).unwrap();
        let mut c = Config::from_env(d.join("dist"));
        c.steam_dir = d.join("steam");
        c.steam_key_file = d.join("key");
        c.steam_api_base = base.into();
        c
    }

    #[test]
    fn account_id_is_steamid_minus_base() {
        assert_eq!(account_id("76561197960265729"), Some(1));
        assert_eq!(account_id("76561198000000000"), Some(39734272));
        assert_eq!(account_id("5"), None);
        assert_eq!(account_id("x"), None);
    }

    #[test]
    fn only_https_or_loopback_http() {
        for ok in [
            "https://api.steampowered.com",
            "http://127.0.0.1:9",
            "http://localhost",
            "http://localhost/x",
        ] {
            assert!(base_ok(ok), "{ok}");
        }
        for bad in [
            "http://api.steampowered.com",
            "http://127.0.0.1.evil.example",
            "http://127.0.0.1@evil.example",
            "http://localhost.evil.example:80",
            "ftp://127.0.0.1",
            "",
        ] {
            assert!(!base_ok(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn errors_never_contain_the_key() {
        let d = tempfile::tempdir().unwrap();
        // Refused connection, unresolvable host, TLS to a plain listener, non-loopback http,
        // and a malformed URL.
        let plain = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let tls_to_plain = format!("https://{}", plain.local_addr().unwrap());
        std::thread::spawn(move || {
            use std::io::Write;
            if let Ok((mut c, _)) = plain.accept() {
                let _ = c.write_all(b"HTTP/1.1 400 not tls\r\n\r\n");
            }
        });
        for base in [
            "http://127.0.0.1:1",
            "https://nonexistent.invalid",
            tls_to_plain.as_str(),
            "http://example.com",
            "https://[::1",
        ] {
            let Err(e) = fetch_owned(&cfg_with(base, d.path())).await else {
                panic!("{base}: fetch succeeded");
            };
            assert!(!e.contains(KEY) && !e.contains("feedface"), "{base}: {e}");
        }
    }

    /// A loopback server that answers every connection with `head` and then `body_len` bytes of
    /// `x` (streamed in 64 KiB writes, no content-length unless `head` has one). Counts requests.
    fn mock(
        head: &'static str,
        body_len: usize,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let n = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits = n.clone();
        std::thread::spawn(move || {
            for c in l.incoming() {
                let Ok(mut c) = c else { return };
                hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = c.read(&mut [0u8; 8192]);
                let _ = c.write_all(head.as_bytes());
                let chunk = vec![b'x'; 64 << 10];
                let mut left = body_len;
                while left > 0 {
                    let k = left.min(chunk.len());
                    if c.write_all(&chunk[..k]).is_err() {
                        break;
                    }
                    left -= k;
                }
            }
        });
        (base, n)
    }

    #[tokio::test]
    async fn key_never_in_errors() {
        let (base, _) = mock(
            "HTTP/1.1 500 X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            0,
        );
        let d = tempfile::tempdir().unwrap();
        let cfg = cfg_with(&base, d.path());
        let e = steam_get(&cfg, "ISteamUser/GetPlayerSummaries/v2/?steamids=1")
            .await
            .unwrap_err();
        assert_eq!(e, "HTTP 500");
        assert!(!e.contains("SENTINELKEY"));
        // store_get has the same error shape.
        let mut cfg = cfg;
        cfg.steam_store_base = base;
        assert_eq!(
            store_get(&cfg, "api/appdetails?appids=1")
                .await
                .unwrap_err(),
            "HTTP 500"
        );
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let (base, hits) = mock(
            "HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:1/elsewhere\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            0,
        );
        let d = tempfile::tempdir().unwrap();
        let Err(e) = fetch_owned(&cfg_with(&base, d.path())).await else {
            panic!("a redirect was accepted");
        };
        assert_eq!(e, "HTTP 302");
        assert!(!e.contains(KEY));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn bodies_over_8_mib_are_cut_while_streaming() {
        // No content-length: ureq cannot refuse up front, it has to stop reading at the cap.
        let (base, _) = mock("HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n", 9 << 20);
        let d = tempfile::tempdir().unwrap();
        let Err(e) = fetch_owned(&cfg_with(&base, d.path())).await else {
            panic!("an oversized body was accepted");
        };
        assert_eq!(e, "response too large");
        // Just under the cap is fine (not JSON, so that error instead).
        let (base, _) = mock(
            "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n",
            (8 << 20) - 1,
        );
        let Err(e) = fetch_owned(&cfg_with(&base, d.path())).await else {
            panic!("x... is not JSON");
        };
        assert_eq!(e, "response is not JSON");
    }

    /// Real network: proves the graviola + platform-verifier TLS stack against Steam's public,
    /// keyless endpoint. Run with `cargo test -p brisk-hostd -- --ignored`.
    #[test]
    #[ignore = "needs the internet"]
    fn live_tls_to_steam() {
        let b = https_get("https://api.steampowered.com/ISteamWebAPIUtil/GetServerInfo/v1/")
            .expect("GET over TLS");
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v["servertime"].as_u64().unwrap() > 1_700_000_000, "{v}");
        // Verification is on: an untrusted certificate is refused.
        assert_eq!(
            https_get("https://self-signed.badssl.com/").unwrap_err(),
            "TLS error"
        );
    }
}
