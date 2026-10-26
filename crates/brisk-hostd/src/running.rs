//! Running Steam games and their sway windows. Steam starts every game under a launcher reaper
//! (`reaper SteamLaunch AppId=<id> -- ...`); the reaper's descendants are the game's processes.

use crate::host::Runner;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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

/// The descendants of `appid`'s reapers, not the reapers themselves (so never Steam, their
/// parent); `None` when the game is not running.
fn game_pids(proc: &Path, appid: u32) -> Option<HashSet<u32>> {
    game_tree(proc, appid).map(|t| t.1)
}

/// `(reapers, their descendants)` of `appid`; `None` when no reaper is alive.
fn game_tree(proc: &Path, appid: u32) -> Option<(HashSet<u32>, HashSet<u32>)> {
    let ps = procs(proc);
    let reapers: HashSet<u32> = ps
        .iter()
        .filter(|(_, _, a)| reaper_appid(a) == Some(appid))
        .map(|p| p.0)
        .collect();
    if reapers.is_empty() {
        return None;
    }
    let mut pids = reapers.clone();
    loop {
        let n = pids.len();
        for (pid, ppid, _) in &ps {
            if pids.contains(ppid) {
                pids.insert(*pid);
            }
        }
        if pids.len() == n {
            break;
        }
    }
    let kids = &pids - &reapers;
    Some((reapers, kids))
}

/// Every window (con id) whose pid is one of `pids`, in tree order.
fn windows(tree: &Value, pids: &HashSet<u32>, out: &mut Vec<u64>) {
    let own = tree
        .get("pid")
        .and_then(Value::as_u64)
        .filter(|p| {
            matches!(tree["type"].as_str(), Some("con" | "floating_con"))
                && pids.contains(&(*p as u32))
        })
        .and_then(|_| tree["id"].as_u64());
    out.extend(own);
    for n in ["nodes", "floating_nodes"]
        .iter()
        .filter_map(|k| tree.get(k)?.as_array())
        .flatten()
    {
        windows(n, pids, out);
    }
}

/// The newest window (highest con id) whose pid is one of `pids`.
#[cfg(test)]
fn find_window(tree: &Value, pids: &HashSet<u32>) -> Option<u64> {
    let mut v = vec![];
    windows(tree, pids, &mut v);
    v.into_iter().max()
}

/// The game's windows in sway's tree, lowest con id first.
fn game_windows(runner: &dyn Runner, sock: &[String], pids: &HashSet<u32>) -> Vec<u64> {
    let mut a: Vec<&str> = sock.iter().map(String::as_str).collect();
    a.extend(["-t", "get_tree"]);
    let mut v = vec![];
    if let Some(t) = runner
        .run("swaymsg", &a)
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        windows(&t, pids, &mut v);
    }
    v.sort_unstable();
    v
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
        .unwrap_or_else(|| {
            // SAFETY: getuid cannot fail.
            format!("/run/user/{}", unsafe { libc::getuid() })
        });
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
    let Some(pids) = game_pids(proc, appid) else {
        return false;
    };
    let Some(&id) = game_windows(runner, sock, &pids).last() else {
        return false;
    };
    let mut args: Vec<&str> = sock.iter().map(String::as_str).collect();
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

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Quit {
    Closed,
    StillRunning,
}

#[derive(Debug, PartialEq)]
pub enum QuitError {
    NotRunning,
    /// A quit of this game is already under way.
    Busy,
    /// hostd runs as root, where a wrong pid would hurt far more: quit is off.
    Root,
}

/// Quits running games: sway's polite close on the game's windows, then SIGTERM, then (forced
/// only) SIGKILL, each to the reaper's descendants only. One quit per game at a time.
pub struct Quitter {
    runner: Arc<dyn Runner>,
    proc: PathBuf,
    busy: Mutex<HashSet<u32>>,
    /// How long each of the close and SIGTERM steps waits for the game to exit.
    pub wait: Duration,
    /// euid 0 at startup; tests set it.
    pub as_root: bool,
}

/// The wait after SIGTERM when forcing (or for a lone reaper), and after SIGKILL.
const BRIEF: Duration = Duration::from_secs(3);

/// Frees the game's slot however the quit ends.
struct Claim<'a>(&'a Mutex<HashSet<u32>>, u32);
impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.0.lock().unwrap().remove(&self.1);
    }
}

impl Quitter {
    pub fn new(runner: Arc<dyn Runner>, proc: PathBuf) -> Self {
        // SAFETY: geteuid has no preconditions.
        let as_root = unsafe { libc::geteuid() } == 0;
        if as_root {
            eprintln!(
                "warning: hostd runs as root; POST /api/quit is off (run it as the desktop user)"
            );
        }
        Quitter {
            runner,
            proc,
            busy: Mutex::default(),
            wait: Duration::from_secs(10),
            as_root,
        }
    }

    /// Off the async threads: every step reads /proc or runs a command.
    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(&dyn Runner, &Path) -> T + Send + 'static,
    ) -> Option<T> {
        let (r, p) = (self.runner.clone(), self.proc.clone());
        tokio::task::spawn_blocking(move || f(&*r, &p)).await.ok()
    }

    /// True once the game's reapers are gone, polling every 100 ms up to `wait`.
    async fn gone_within(&self, appid: u32, wait: Duration) -> bool {
        let end = tokio::time::Instant::now() + wait;
        loop {
            let gone = self.blocking(move |_, p| game_tree(p, appid).is_none());
            if gone.await.unwrap_or(false) {
                return true;
            }
            if tokio::time::Instant::now() >= end {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// `kill <sig>` to the game's processes as they are now (a fresh scan, so pids gone since the
    /// last step are not reused): the reapers' descendants, or the reapers themselves when nothing
    /// is left under them. Never their parent (Steam). shortcut: a pid can still be recycled
    /// between this scan and the signal (microseconds); upgrade to pidfd_send_signal if that matters.
    async fn signal(&self, appid: u32, sig: &'static str) {
        let me = std::process::id();
        self.blocking(move |r, p| {
            let Some((reapers, kids)) = game_tree(p, appid) else {
                return;
            };
            let mut pids: Vec<u32> = if kids.is_empty() { reapers } else { kids }
                .into_iter()
                .filter(|&p| p > 1 && p != me)
                .collect();
            if pids.is_empty() {
                return;
            }
            pids.sort_unstable();
            let pids: Vec<String> = pids.iter().map(u32::to_string).collect();
            let mut a = vec![sig];
            a.extend(pids.iter().map(String::as_str));
            if r.run("kill", &a).is_none() {
                // Often only a pid that exited meanwhile; EPERM or a missing kill binary look the same.
                eprintln!(
                    "warning: kill {sig} for game {appid} failed for some of {} pids",
                    pids.len()
                );
            }
        })
        .await;
    }

    /// Closes `appid` politely, escalates to SIGTERM, and with `force` to SIGKILL. A forced quit
    /// skips the polite close: SIGTERM, 3 s, SIGKILL.
    pub async fn quit(&self, appid: u32, force: bool) -> Result<Quit, QuitError> {
        if self.as_root {
            return Err(QuitError::Root);
        }
        let _claim = {
            let mut b = self.busy.lock().unwrap();
            if !b.insert(appid) {
                return Err(QuitError::Busy);
            }
            Claim(&self.busy, appid)
        };
        let polite = !force;
        let asked = self.blocking(move |r, p| {
            let (_, kids) = game_tree(p, appid)?;
            if polite && !kids.is_empty() {
                let sock = sway_sock(&crate::stream::graphical_env());
                // sway's `kill` is the polite close request, the same as the window's X button.
                for id in game_windows(r, &sock, &kids) {
                    let cmd = format!("[con_id={id}] kill");
                    let mut a: Vec<&str> = sock.iter().map(String::as_str).collect();
                    a.push(&cmd);
                    r.run("swaymsg", &a);
                }
            }
            Some(kids.is_empty())
        });
        let Some(lone) = asked.await.flatten() else {
            return Err(QuitError::NotRunning);
        };
        if polite && !lone {
            if self.gone_within(appid, self.wait).await {
                return Ok(Quit::Closed);
            }
            eprintln!("info: game {appid} still running after closing its windows; SIGTERM");
        }
        self.signal(appid, "-TERM").await;
        let wait = if force || lone { BRIEF } else { self.wait };
        if self.gone_within(appid, wait).await {
            return Ok(Quit::Closed);
        }
        if force {
            eprintln!("info: game {appid} still running after SIGTERM; SIGKILL (forced)");
            self.signal(appid, "-KILL").await;
            if self.gone_within(appid, BRIEF).await {
                return Ok(Quit::Closed);
            }
        }
        Ok(Quit::StillRunning)
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
