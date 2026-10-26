//! Steam library: installed games from local manifests, owned games from `GetOwnedGames`.
//! The API key is read from `cfg.steam_key_file` per fetch, never stored or logged.

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

const OWNED_TTL_S: u64 = 6 * 3600;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub appid: u32,
    pub name: String,
    pub installed: bool,
    pub size_bytes: Option<u64>,
    pub last_played: Option<u64>,
    pub playtime_min: u64,
    pub cover: String,
    /// Steam is running it now (its launcher reaper is alive).
    pub running: bool,
}

#[derive(Clone, Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GamesResult {
    pub games: Vec<Game>,
    pub owned_available: bool,
}

impl GamesResult {
    pub fn is_known_appid(&self, id: u32) -> bool {
        self.games.iter().any(|g| g.appid == id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub appid: u32,
    pub name: String,
    pub size: u64,
    pub state_flags: u32,
    pub last_played: u64,
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
    Some(Installed {
        appid: get_s(a, "appid")?.parse().ok()?,
        name: name.to_string(),
        size: num("SizeOnDisk").unwrap_or(0),
        state_flags: num("StateFlags").unwrap_or(0) as u32,
        last_played: num("LastPlayed").unwrap_or(0),
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
                let better = |o: &Installed| i.state_flags & 4 != 0 && o.state_flags & 4 == 0;
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
    rtime_last_played: u64,
}

#[derive(Serialize, Deserialize)]
struct OwnedCache {
    fetched: u64,
    games: Vec<Owned>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// One GetOwnedGames call. curl takes the URL (and so the key) on stdin, never in argv.
/// shortcut: shells out to curl for TLS, since the dependency graph must stay pure Rust; swap in
/// a rustls client if hostd ever ships where curl is absent.
async fn fetch_owned(cfg: &Config) -> Option<Vec<Owned>> {
    let key = std::fs::read_to_string(&cfg.steam_key_file).ok()?;
    let key = key.trim();
    // The key goes into a curl config line: only plain key characters are allowed.
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let url = format!(
        "{}/IPlayerService/GetOwnedGames/v1/?key={key}&steamid={}&include_appinfo=1&include_played_free_games=1&format=json",
        cfg.steam_api_base.trim_end_matches('/'),
        steam_id(&cfg.steam_dir)?
    );
    let mut child = tokio::process::Command::new("curl")
        .args(["-sSf", "--max-time", "15", "-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    stdin
        .write_all(format!("url = \"{url}\"\n").as_bytes())
        .await
        .ok()?;
    drop(stdin);
    let out = tokio::time::timeout(Duration::from_secs(20), child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "warning: GetOwnedGames failed (curl exit {:?})",
            out.status.code()
        );
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    serde_json::from_value(v.get("response")?.get("games")?.clone()).ok()
}

/// Steam account id: the entry marked MostRecent, else (newer clients drop that field) the one
/// with the newest `timestamp`.
fn steam_id(steam_dir: &Path) -> Option<String> {
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
        // It goes into a curl config line: digits only.
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
        Some(games) => {
            let c = OwnedCache {
                fetched: now(),
                games,
            };
            let _ = tokio::fs::create_dir_all(&cfg.data_dir).await;
            let _ = tokio::fs::write(&path, serde_json::to_vec(&c).unwrap_or_default()).await;
            (c.games, true)
        }
        // Stale cache is better than nothing, but the UI still says Steam was unreachable.
        None => (cache.map(|c| c.games).unwrap_or_default(), false),
    }
}

pub async fn load_games(cfg: &Config) -> GamesResult {
    load_games_with(cfg, false).await
}

pub async fn load_games_with(cfg: &Config, refresh: bool) -> GamesResult {
    let steam_dir = cfg.steam_dir.clone();
    let installed = tokio::task::spawn_blocking(move || installed_manifests(&steam_dir))
        .await
        .unwrap_or_default();
    let (owned, owned_available) = owned(cfg, refresh).await;

    let mut by_id: HashMap<u32, Game> = HashMap::new();
    let cover =
        |id| format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/library_600x900.jpg");
    for o in owned {
        by_id.insert(
            o.appid,
            Game {
                appid: o.appid,
                name: o.name,
                installed: false,
                size_bytes: None,
                last_played: (o.rtime_last_played > 0).then_some(o.rtime_last_played),
                playtime_min: o.playtime_forever,
                cover: cover(o.appid),
                running: false,
            },
        );
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
        if if owned_available {
            !by_id.contains_key(&i.appid)
        } else {
            tool
        } {
            continue;
        }
        let g = by_id.entry(i.appid).or_insert_with(|| Game {
            appid: i.appid,
            name: i.name.clone(),
            installed: false,
            size_bytes: None,
            last_played: None,
            playtime_min: 0,
            cover: cover(i.appid),
            running: false,
        });
        g.installed = i.state_flags & 4 != 0;
        if g.installed {
            g.size_bytes = Some(i.size);
        }
        if i.last_played > g.last_played.unwrap_or(0) {
            g.last_played = Some(i.last_played);
        }
    }
    let proc = cfg.proc_dir.clone();
    let running = tokio::task::spawn_blocking(move || crate::running::running_appids(&proc))
        .await
        .unwrap_or_default();
    let mut games: Vec<Game> = by_id.into_values().collect();
    for g in &mut games {
        g.running = running.contains(&g.appid);
    }
    games.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    GamesResult {
        games,
        owned_available,
    }
}

#[cfg(test)]
mod steam_id_tests {
    use super::steam_id;

    #[test]
    fn hostile_id_is_rejected() {
        let d = std::env::temp_dir().join(format!("brisk-sid-bad-{}", std::process::id()));
        std::fs::create_dir_all(d.join("config")).unwrap();
        let vdf = "\"users\"\n{\n\t\"1\\\"\\nurl=x\"\n\t{\n\t\t\"timestamp\"\t\"5\"\n\t}\n}\n";
        std::fs::write(d.join("config/loginusers.vdf"), vdf).unwrap();
        assert_eq!(steam_id(&d), None);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn newest_timestamp_when_no_mostrecent() {
        let d = std::env::temp_dir().join(format!("brisk-sid-{}", std::process::id()));
        std::fs::create_dir_all(d.join("config")).unwrap();
        let u = |id: &str, t: u32| format!("\t\"{id}\"\n\t{{\n\t\t\"timestamp\"\t\"{t}\"\n\t}}\n");
        let vdf = format!("\"users\"\n{{\n{}{}}}\n", u("111", 5), u("222", 9));
        std::fs::write(d.join("config/loginusers.vdf"), vdf).unwrap();
        assert_eq!(steam_id(&d).as_deref(), Some("222"));
        std::fs::remove_dir_all(&d).ok();
    }
}
