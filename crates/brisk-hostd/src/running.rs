//! Running Steam games and their sway windows. Steam starts every game under a launcher reaper
//! (`reaper SteamLaunch AppId=<id> -- ...`); the reaper's descendants are the game's processes.

use crate::host::Runner;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One process as `/proc/<pid>` shows it. `start` (boot-relative start time) plus the pid is what
/// tells a process apart from a later one that reuses its pid.
#[derive(Debug, PartialEq)]
struct Proc {
    pid: u32,
    ppid: u32,
    start: Option<u64>,
    argv: Vec<String>,
}

fn read_proc(dir: &Path, pid: u32) -> Option<Proc> {
    let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
    // comm may hold spaces and parens: the fields start after the last ')', with state first.
    let f: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ppid = f.get(1)?.parse().ok()?;
    // starttime is stat field 22, the 20th after comm.
    let start = f.get(19).and_then(|s| s.parse().ok());
    let argv = std::fs::read(dir.join("cmdline")).unwrap_or_default();
    let argv = argv
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    Some(Proc {
        pid,
        ppid,
        start,
        argv,
    })
}

/// Every process under `proc` (normally `/proc`) that can be read.
fn procs(proc: &Path) -> Vec<Proc> {
    let Ok(dir) = std::fs::read_dir(proc) else {
        return vec![];
    };
    dir.flatten()
        .filter_map(|e| read_proc(&e.path(), e.file_name().to_str()?.parse().ok()?))
        .collect()
}

/// True if `/proc/<pid>` is still the process `seen` in the scan: same start time and argv.
/// The ppid is not compared: a game's helpers are reparented when run.sh exits mid-quit.
fn same_process(proc: &Path, seen: &Proc) -> bool {
    read_proc(&proc.join(seen.pid.to_string()), seen.pid)
        .is_some_and(|now| now.start.is_some() && now.start == seen.start && now.argv == seen.argv)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sig {
    Term,
    Kill,
}

/// What became of one signal.
#[derive(Debug, PartialEq)]
pub enum Sent {
    Signalled,
    /// No such process (ESRCH): it already exited, which is what the signal was for.
    Gone,
    /// The pid now belongs to another process: not signalled.
    Recycled,
    /// EPERM.
    Denied,
    /// No pidfd on this kernel (ENOSYS): the round stops, with no fallback to kill(2).
    Unsupported,
    /// Any other error for this pid (EINVAL, EMFILE, ...): counted, the round goes on.
    Failed(String),
}

/// Delivers signals to game processes. The real one is [`Pidfd`]; tests use a fake.
pub trait Signaller: Send + Sync {
    /// Signals `pid` through a handle that pins the process, and only if `same` (called once the
    /// handle is held) confirms it is still the process that was scanned.
    fn signal(&self, pid: u32, sig: Sig, same: &dyn Fn() -> bool) -> Sent;
}

/// `pidfd_open` + `pidfd_send_signal`: once the pidfd is open it names that one process, so the
/// identity check after opening it closes the pid-reuse race of kill(2) by number. Kernels
/// without pidfd (before 5.3) get a clear error, never a fallback to kill(2).
pub struct Pidfd;

impl Signaller for Pidfd {
    fn signal(&self, pid: u32, sig: Sig, same: &dyn Fn() -> bool) -> Sent {
        use rustix::io::Errno;
        use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};
        let Some(p) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
            return Sent::Gone;
        };
        let fd = match pidfd_open(p, PidfdFlags::empty()) {
            Ok(fd) => fd,
            Err(Errno::SRCH) => return Sent::Gone,
            Err(Errno::NOSYS) => return Sent::Unsupported,
            Err(e) => return Sent::Failed(format!("pidfd_open: {e}")),
        };
        if !same() {
            return Sent::Recycled;
        }
        let s = match sig {
            Sig::Term => Signal::TERM,
            Sig::Kill => Signal::KILL,
        };
        match pidfd_send_signal(&fd, s) {
            Ok(()) => Sent::Signalled,
            Err(Errno::SRCH) => Sent::Gone,
            Err(Errno::PERM) => Sent::Denied,
            Err(Errno::NOSYS) => Sent::Unsupported,
            Err(e) => Sent::Failed(format!("pidfd_send_signal: {e}")),
        }
    }
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
        .filter_map(|p| reaper_appid(&p.argv))
        .collect()
}

/// The descendants of `appid`'s reapers, not the reapers themselves (so never Steam, their
/// parent); `None` when the game is not running.
fn game_pids(proc: &Path, appid: u32) -> Option<HashSet<u32>> {
    game_tree(proc, appid).map(|t| t.1)
}

/// `(reapers, their descendants)` of `appid`; `None` when no reaper is alive.
fn game_tree(proc: &Path, appid: u32) -> Option<(HashSet<u32>, HashSet<u32>)> {
    tree(&procs(proc), appid)
}

fn tree(ps: &[Proc], appid: u32) -> Option<(HashSet<u32>, HashSet<u32>)> {
    let reapers: HashSet<u32> = ps
        .iter()
        .filter(|p| reaper_appid(&p.argv) == Some(appid))
        .map(|p| p.pid)
        .collect();
    if reapers.is_empty() {
        return None;
    }
    let mut pids = reapers.clone();
    loop {
        let n = pids.len();
        for p in ps {
            if pids.contains(&p.ppid) {
                pids.insert(p.pid);
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
        .unwrap_or_else(|| format!("/run/user/{}", rustix::process::getuid().as_raw()));
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
    /// [`Pidfd`]; tests swap in a fake.
    pub signaller: Arc<dyn Signaller>,
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
        let as_root = rustix::process::geteuid().is_root();
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
            signaller: Arc::new(Pidfd),
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

    /// `sig` to the game's processes as they are now (a fresh scan): the reapers' descendants, or
    /// the reapers themselves when nothing is left under them. Never their parent (Steam). Each
    /// pid is signalled through a pidfd and only if it is still the scanned process.
    async fn signal(&self, appid: u32, sig: Sig) {
        let me = std::process::id();
        let s = self.signaller.clone();
        self.blocking(move |_, p| {
            let ps = procs(p);
            let Some((reapers, kids)) = tree(&ps, appid) else {
                return;
            };
            let targets = if kids.is_empty() { reapers } else { kids };
            let mut seen: Vec<&Proc> = ps
                .iter()
                .filter(|x| targets.contains(&x.pid) && x.pid > 1 && x.pid != me)
                .collect();
            seen.sort_unstable_by_key(|x| x.pid);
            let (mut denied, mut recycled, mut failed) = (0, 0, 0);
            for x in seen {
                match s.signal(x.pid, sig, &|| same_process(p, x)) {
                    Sent::Signalled | Sent::Gone => {}
                    Sent::Recycled => recycled += 1,
                    Sent::Denied => denied += 1,
                    Sent::Failed(e) => {
                        eprintln!("warning: {sig:?} to pid {} of game {appid}: {e}", x.pid);
                        failed += 1;
                    }
                    Sent::Unsupported => {
                        eprintln!(
                            "error: cannot quit game {appid}: this kernel has no pidfd (Linux 5.3+); no fallback to kill by pid"
                        );
                        return;
                    }
                }
            }
            if denied + recycled + failed > 0 {
                eprintln!(
                    "warning: {sig:?} for game {appid}: {denied} pids refused (EPERM), {recycled} skipped (pid reused), {failed} failed"
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
        self.signal(appid, Sig::Term).await;
        let wait = if force || lone { BRIEF } else { self.wait };
        if self.gone_within(appid, wait).await {
            return Ok(Quit::Closed);
        }
        if force {
            eprintln!("info: game {appid} still running after SIGTERM; SIGKILL (forced)");
            self.signal(appid, Sig::Kill).await;
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

    /// A real `sleep 60` in its own process group, so nothing else can be hit.
    /// Kills and reaps the child when dropped, so a failed assert never leaves a `sleep 60`.
    struct Kid(std::process::Child);
    impl Drop for Kid {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn sleeper() -> (Kid, Proc) {
        use std::os::unix::process::CommandExt;
        let c = Kid(std::process::Command::new("sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .unwrap());
        let pid = c.0.id();
        // The child may still be pre-exec for a moment: wait for sleep's own argv.
        for _ in 0..200 {
            if let Some(p) = read_proc(&Path::new("/proc").join(pid.to_string()), pid)
                && p.argv == ["sleep", "60"]
            {
                return (c, p);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("sleep never exec'd");
    }

    #[test]
    fn pidfd_sigterms_a_real_process_that_is_still_the_scanned_one() {
        let (mut c, seen) = sleeper();
        assert!(seen.start.is_some());
        let same = || same_process(Path::new("/proc"), &seen);
        assert_eq!(Pidfd.signal(seen.pid, Sig::Term, &same), Sent::Signalled);
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(c.0.wait().unwrap().signal(), Some(15));
        // Reaped: the pid names nothing now (or, rarely, a newer process: never signalled).
        let r = Pidfd.signal(seen.pid, Sig::Term, &same);
        assert!(matches!(r, Sent::Gone | Sent::Recycled), "{r:?}");
    }

    #[test]
    fn pidfd_refuses_a_recycled_pid() {
        let (mut c, mut seen) = sleeper();
        // What the scan would have seen had the pid belonged to an earlier process.
        seen.start = seen.start.map(|s| s.saturating_sub(1));
        let same = || same_process(Path::new("/proc"), &seen);
        assert_eq!(Pidfd.signal(seen.pid, Sig::Term, &same), Sent::Recycled);
        assert!(
            c.0.try_wait().unwrap().is_none(),
            "the newer process was signalled"
        );
    }

    #[test]
    fn stat_fields_after_a_hostile_comm() {
        let d = tempfile::tempdir().unwrap();
        let stat = "7 (a) S 9 (b) S 3 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 4242 0 0";
        std::fs::write(d.path().join("stat"), stat).unwrap();
        std::fs::write(d.path().join("cmdline"), "x\0y\0").unwrap();
        let p = read_proc(d.path(), 7).unwrap();
        assert_eq!(
            (p.ppid, p.start, p.argv),
            (3, Some(4242), vec!["x".into(), "y".into()])
        );
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
