//! Running Steam games and their sway windows. Steam starts every game under a launcher reaper
//! (`reaper SteamLaunch AppId=<id> -- ...`); the reaper's descendants are the game's processes.

use crate::host::Runner;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// `(pid, ppid, argv)` of every process under `proc` (normally `/proc`) that can be read.
fn procs(proc: &Path) -> Vec<(u32, u32, Vec<String>)> {
    let Ok(dir) = std::fs::read_dir(proc) else {
        return vec![];
    };
    dir.flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
            // comm may hold spaces and parens: the fields start after the last ')'.
            let ppid = stat
                .rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()?;
            let argv = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
            let argv = argv
                .split(|b| *b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            Some((pid, ppid, argv))
        })
        .collect()
}

/// The AppId of a Steam launcher reaper's argv.
fn reaper_appid(argv: &[String]) -> Option<u32> {
    argv.windows(2)
        .find(|w| w[0] == "SteamLaunch")?
        .get(1)?
        .strip_prefix("AppId=")?
        .parse()
        .ok()
}

/// AppIds of every game Steam is running now.
pub fn running_appids(proc: &Path) -> HashSet<u32> {
    procs(proc)
        .iter()
        .filter_map(|(_, _, a)| reaper_appid(a))
        .collect()
}

/// The reaper of `appid` and all its descendants; empty when the game is not running.
fn game_pids(proc: &Path, appid: u32) -> HashSet<u32> {
    let ps = procs(proc);
    let mut pids: HashSet<u32> = ps
        .iter()
        .filter(|(_, _, a)| reaper_appid(a) == Some(appid))
        .map(|p| p.0)
        .collect();
    loop {
        let n = pids.len();
        for (pid, ppid, _) in &ps {
            if pids.contains(ppid) {
                pids.insert(*pid);
            }
        }
        if pids.len() == n {
            return pids;
        }
    }
}

/// The newest window (highest con id) whose pid is one of `pids`.
fn find_window(tree: &Value, pids: &HashSet<u32>) -> Option<u64> {
    let own = tree
        .get("pid")
        .and_then(Value::as_u64)
        .filter(|p| {
            matches!(tree["type"].as_str(), Some("con" | "floating_con"))
                && pids.contains(&(*p as u32))
        })
        .and_then(|_| tree["id"].as_u64());
    ["nodes", "floating_nodes"]
        .iter()
        .filter_map(|k| tree.get(k)?.as_array())
        .flatten()
        .filter_map(|n| find_window(n, pids))
        .chain(own)
        .max()
}

/// `-s <socket>` for swaymsg: SWAYSOCK of the desktop session, else the first sway socket in the
/// runtime dir; none leaves swaymsg to hostd's own environment.
fn sway_sock(env: &[(String, String)]) -> Vec<String> {
    if let Some((_, s)) = env.iter().find(|(k, _)| k == "SWAYSOCK") {
        return vec!["-s".into(), s.clone()];
    }
    let run = env
        .iter()
        .find(|(k, _)| k == "XDG_RUNTIME_DIR")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| format!("/run/user/{}", unsafe { libc::getuid() }));
    std::fs::read_dir(run)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("sway-ipc.") && n.ends_with(".sock"))
        })
        .map(|p| vec!["-s".into(), p.display().to_string()])
        .unwrap_or_default()
}

/// One try: focus and fullscreen the game's window. False if the game, its window or sway is missing.
fn focus_once(runner: &dyn Runner, proc: &Path, sock: &[String], appid: u32) -> bool {
    let pids = game_pids(proc, appid);
    if pids.is_empty() {
        return false;
    }
    let mut args: Vec<&str> = sock.iter().map(String::as_str).collect();
    let tree = {
        let mut a = args.clone();
        a.extend(["-t", "get_tree"]);
        runner.run("swaymsg", &a)
    };
    let Some(id) = tree
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|t| find_window(&t, &pids))
    else {
        return false;
    };
    // `id` is a number from sway's JSON, so the criteria cannot carry anything else.
    let cmd = format!("[con_id={id}] focus, fullscreen enable");
    args.push(&cmd);
    runner.run("swaymsg", &args).is_some()
}

/// Polls every second, up to `wait`, until the game's window is focused and fullscreen. Async
/// between tries, so hostd's shutdown never waits on it. Never fails: a miss is only logged.
pub async fn focus_game(
    runner: Arc<dyn Runner>,
    proc: PathBuf,
    env: Vec<(String, String)>,
    appid: u32,
    wait: Duration,
) {
    let sock = Arc::new(sway_sock(&env));
    let end = Instant::now() + wait;
    loop {
        let (r, p, s) = (runner.clone(), proc.clone(), sock.clone());
        let done = tokio::task::spawn_blocking(move || focus_once(&*r, &p, &s, appid)).await;
        if done.unwrap_or(false) {
            return;
        }
        if Instant::now() >= end {
            eprintln!("warning: no sway window for game {appid} after {wait:?}; left as is");
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_windows_of_the_game() {
        let t: Value =
            serde_json::from_str(include_str!("../tests/fixtures/sway-tree.json")).unwrap();
        assert_eq!(find_window(&t, &[501, 502].into()), Some(16));
        assert_eq!(find_window(&t, &[400].into()), Some(6));
        assert_eq!(find_window(&t, &[999].into()), None);
        assert_eq!(find_window(&t, &HashSet::new()), None);
    }

    #[test]
    fn reaper_appid_needs_steamlaunch() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            reaper_appid(&a(&["reaper", "SteamLaunch", "AppId=686060", "--"])),
            Some(686060)
        );
        assert_eq!(reaper_appid(&a(&["vim", "AppId=5"])), None);
        assert_eq!(
            reaper_appid(&a(&["reaper", "SteamLaunch", "AppId=x"])),
            None
        );
    }
}
