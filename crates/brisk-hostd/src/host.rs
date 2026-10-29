//! Host status (`GET /api/host`) and the rate-limited Sunshine restart. Every source fails to `None`.

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const RESTART_GAP_S: u64 = 100;
/// Same port and `/serverinfo` probe as `sunshine_idle` in scripts/brisk-common.sh. That helper
/// also needs `currentgame` 0; this check only looks for the FREE state.
const SUNSHINE_ADDR: &str = "127.0.0.1:47989";

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
/// Sunshine as `/serverinfo` reports it.
pub enum Sunshine {
    Ready,
    Busy,
    Down,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
/// The answer of `GET /api/host`.
pub struct HostInfo {
    pub name: String,
    pub sunshine: Sunshine,
    pub uptime_s: Option<u64>,
    pub cpu_temp_c: Option<f64>,
    pub gpu_temp_c: Option<f64>,
    pub path: Option<&'static str>,
    pub relay: Option<String>,
}

/// Runs a command and returns its stdout on success. Blocking; call from `spawn_blocking`.
pub trait Runner: Send + Sync {
    /// Runs `prog` and waits; stdout on success.
    fn run(&self, prog: &str, args: &[&str]) -> Option<String>;
    /// Starts a command detached and returns once it is running (false if it could not start).
    /// The default only suits fakes; `RealRunner` never waits.
    fn spawn(&self, prog: &str, args: &[&str], _env: &[(String, String)]) -> bool {
        self.run(prog, args).is_some()
    }
}

/// Runs real programs.
pub struct RealRunner;
impl Runner for RealRunner {
    /// Own process group and null stdio, so it outlives hostd's terminal and signals; a thread reaps it.
    /// With `BRISK_SPAWN_SCOPE=1` (set by deploy/brisk-hostd.service) it also gets its own scope:
    /// a Steam that `steam steam://…` cold-starts must not sit in hostd's cgroup (MemoryMax,
    /// killed on restart).
    fn spawn(&self, prog: &str, args: &[&str], env: &[(String, String)]) -> bool {
        use std::os::unix::process::CommandExt;
        let scoped = std::env::var_os("BRISK_SPAWN_SCOPE").is_some_and(|v| v == "1");
        // systemd-run would start fine for a missing program; keep "could not start" honest.
        if scoped && !on_path(prog) {
            return false;
        }
        let mut c = std::process::Command::new(if scoped { "systemd-run" } else { prog });
        if scoped {
            c.args(["--user", "--scope", "--quiet", "--collect", "--", prog]);
        }
        c.args(args).envs(env.iter().map(|(k, v)| (k, v)));
        let c = crate::notify::scrub(&mut c)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn();
        match c {
            Ok(mut c) => {
                std::thread::spawn(move || c.wait());
                true
            }
            Err(_) => false,
        }
    }

    fn run(&self, prog: &str, args: &[&str]) -> Option<String> {
        use std::io::Read;
        let mut c = crate::notify::scrub(std::process::Command::new(prog).args(args))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        // Drain stdout on its own thread so a big output cannot fill the pipe and stall the child.
        let mut so = c.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut out = String::new();
            so.read_to_string(&mut out).ok().map(|_| out)
        });
        let end = std::time::Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(st) = c.try_wait().ok()? {
                break st;
            }
            if std::time::Instant::now() > end {
                let _ = c.kill(); // closes the pipe, so the reader ends too
                let _ = c.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let out = reader.join().ok()??;
        status.success().then_some(out)
    }
}

fn on_path(prog: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let exec = |p: &Path| {
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if prog.contains('/') {
        return exec(Path::new(prog));
    }
    std::env::var_os("PATH")
        .is_some_and(|ps| std::env::split_paths(&ps).any(|d| exec(&d.join(prog))))
}

/// Seconds since some fixed start; tests use a fake.
pub trait Clock: Send + Sync {
    /// Seconds since the clock started.
    fn now_s(&self) -> u64;
}
/// The real clock.
pub struct SystemClock(std::time::Instant);
impl Default for SystemClock {
    fn default() -> Self {
        SystemClock(std::time::Instant::now())
    }
}
impl Clock for SystemClock {
    fn now_s(&self) -> u64 {
        self.0.elapsed().as_secs()
    }
}

/// Where host facts come from; tests point these at fixtures.
pub struct Sources {
    pub hwmon: PathBuf,
    pub uptime: PathBuf,
    pub sunshine_addr: String,
    pub tailscale_timeout: Duration,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            hwmon: "/sys/class/hwmon".into(),
            uptime: "/proc/uptime".into(),
            sunshine_addr: SUNSHINE_ADDR.into(),
            tailscale_timeout: Duration::from_secs(3),
        }
    }
}

/// Host facts from the real sources.
pub(crate) async fn host_info() -> HostInfo {
    host_info_with(&Sources::default(), Arc::new(RealRunner)).await
}

/// Host facts from `src`, with `runner` for tailscale.
async fn host_info_with(src: &Sources, runner: Arc<dyn Runner>) -> HostInfo {
    let job = tokio::task::spawn_blocking(move || runner.run("tailscale", &["status", "--json"]));
    let ts = tokio::time::timeout(src.tailscale_timeout, job)
        .await
        .ok()
        .and_then(Result::ok)
        .flatten();
    let (path, relay) = ts.as_deref().map_or((None, None), parse_tailscale);
    let (cpu, gpu) = read_temps(&src.hwmon);
    HostInfo {
        name: hostname(),
        sunshine: sunshine_state(&src.sunshine_addr).await,
        uptime_s: std::fs::read_to_string(&src.uptime)
            .ok()
            .and_then(|s| s.split('.').next()?.parse().ok()),
        cpu_temp_c: cpu,
        gpu_temp_c: gpu,
        path,
        relay,
    }
}

/// This machine's hostname, or "unknown" when it cannot be read.
pub(crate) fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map_or_else(|_| "unknown".into(), |s| s.trim().to_string())
}

async fn sunshine_state(addr: &str) -> Sunshine {
    let probe = async {
        let mut s = tokio::net::TcpStream::connect(addr).await.ok()?;
        s.write_all(b"GET /serverinfo HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .await
            .ok()?;
        let mut buf = String::new();
        s.read_to_string(&mut buf).await.ok()?;
        Some(buf)
    };
    match tokio::time::timeout(Duration::from_secs(2), probe).await {
        Ok(Some(b)) if b.contains("<state>SUNSHINE_SERVER_FREE</state>") => Sunshine::Ready,
        Ok(Some(_)) => Sunshine::Busy,
        _ => Sunshine::Down,
    }
}

fn read_temps(root: &Path) -> (Option<f64>, Option<f64>) {
    let (mut cpu, mut gpu) = (None, None);
    let Ok(dirs) = std::fs::read_dir(root) else {
        return (None, None);
    };
    let mut dirs: Vec<_> = dirs.flatten().map(|d| d.path()).collect();
    dirs.sort();
    for d in dirs {
        let name = std::fs::read_to_string(d.join("name")).unwrap_or_default();
        let read = |n: &str| -> Option<f64> {
            let v: f64 = std::fs::read_to_string(d.join(n))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            Some(v / 1000.0)
        };
        match name.trim() {
            "coretemp" | "k10temp" if cpu.is_none() => {
                let labelled = (1..=32).find(|i| {
                    std::fs::read_to_string(d.join(format!("temp{i}_label")))
                        .is_ok_and(|l| l.starts_with("Package") || l.trim() == "Tctl")
                });
                cpu = read(&format!("temp{}_input", labelled.unwrap_or(1)));
            }
            "amdgpu" | "i915" if gpu.is_none() => gpu = read("temp1_input"),
            _ => {}
        }
    }
    (cpu, gpu)
}

/// First active peer decides: a `CurAddr` means a direct path, else its DERP relay.
fn parse_tailscale(json: &str) -> (Option<&'static str>, Option<String>) {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return (None, None);
    };
    let Some(peers) = v.get("Peer").and_then(Value::as_object) else {
        return (None, None);
    };
    let mut keys: Vec<_> = peers.keys().collect();
    keys.sort();
    for k in keys {
        let p = &peers[k];
        if p.get("Active").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let s = |f: &str| p.get(f).and_then(Value::as_str).filter(|s| !s.is_empty());
        if s("CurAddr").is_some() {
            return (Some("direct"), None);
        }
        if let Some(r) = s("Relay") {
            return (Some("relay"), Some(r.to_string()));
        }
    }
    (None, None)
}

/// `systemctl --user restart sunshine`, at most once per 100 s.
pub struct Restarter {
    runner: Arc<dyn Runner>,
    clock: Arc<dyn Clock>,
    last: Mutex<Option<u64>>,
    pub timeout: Duration,
}

#[derive(Debug, PartialEq)]
/// Why a Sunshine restart did not happen.
pub enum RestartError {
    /// Seconds until the next restart is allowed (the `Retry-After`).
    TooSoon(u64),
    Failed,
}

impl Restarter {
    /// A restarter that runs commands through `runner`.
    pub fn new(runner: Arc<dyn Runner>, clock: Arc<dyn Clock>) -> Self {
        Restarter {
            runner,
            clock,
            last: Mutex::new(None),
            timeout: Duration::from_secs(30),
        }
    }

    /// Restarts Sunshine unless the last restart was too recent.
    pub async fn restart(&self) -> Result<(), RestartError> {
        let now = self.clock.now_s();
        let prev = {
            let mut last = self.last.lock().unwrap();
            if let Some(l) = *last
                && now < l + RESTART_GAP_S
            {
                return Err(RestartError::TooSoon(l + RESTART_GAP_S - now));
            }
            last.replace(now) // claimed before running, so a double click cannot slip through
        };
        let r = self.runner.clone();
        let job = tokio::task::spawn_blocking(move || {
            r.run(
                "systemctl",
                &[
                    "--user",
                    "restart",
                    "app-dev.lizardbyte.app.Sunshine.service",
                ],
            )
        });
        // The runner keeps no exit status or stderr, so "failed" is all it can say.
        let why = match tokio::time::timeout(self.timeout, job).await {
            Err(_) => "timeout",
            Ok(Err(_)) => "restart task panicked",
            Ok(Ok(None)) => "systemctl failed (not found, non-zero exit or over 30 s)",
            Ok(Ok(Some(_))) => return Ok(()),
        };
        eprintln!("warning: sunshine restart failed: {why}");
        *self.last.lock().unwrap() = prev;
        Err(RestartError::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fake {
        calls: Mutex<Vec<String>>,
        out: Option<String>,
    }
    impl Fake {
        fn new(out: Option<&str>) -> Arc<Fake> {
            Arc::new(Fake {
                calls: Mutex::new(vec![]),
                out: out.map(Into::into),
            })
        }
    }
    impl Runner for Fake {
        fn run(&self, prog: &str, args: &[&str]) -> Option<String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{prog} {}", args.join(" ")));
            self.out.clone()
        }
    }
    struct FakeClock(AtomicU64);
    impl Clock for FakeClock {
        fn now_s(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    const DIRECT: &str = r#"{"Peer":{"a":{"Active":false,"CurAddr":"","Relay":"fra"},
        "b":{"Active":true,"CurAddr":"1.2.3.4:41641","Relay":"fra"}}}"#;
    const RELAY: &str = r#"{"Peer":{"b":{"Active":true,"CurAddr":"","Relay":"fra"}}}"#;

    #[tokio::test]
    async fn host_info_degrades_to_nulls() {
        let d = tempfile::tempdir().unwrap();
        let src = Sources {
            hwmon: d.path().join("none"),
            uptime: d.path().join("none"),
            sunshine_addr: "127.0.0.1:1".into(),
            tailscale_timeout: Duration::from_secs(3),
        };
        let i = host_info_with(&src, Fake::new(None)).await;
        assert_eq!(
            (
                i.sunshine,
                i.uptime_s,
                i.cpu_temp_c,
                i.gpu_temp_c,
                i.path,
                i.relay
            ),
            (Sunshine::Down, None, None, None, None, None)
        );
    }

    #[test]
    fn parses_tailscale_direct_and_relay() {
        assert_eq!(parse_tailscale(DIRECT), (Some("direct"), None));
        assert_eq!(parse_tailscale(RELAY), (Some("relay"), Some("fra".into())));
        assert_eq!(parse_tailscale("not json"), (None, None));
    }

    #[test]
    fn picks_cpu_package_temp() {
        let d = tempfile::tempdir().unwrap();
        let mk = |dir: &str, name: &str, files: &[(&str, &str)]| {
            let p = d.path().join(dir);
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("name"), name).unwrap();
            for (f, c) in files {
                std::fs::write(p.join(f), c).unwrap();
            }
        };
        mk("hwmon0", "nvme\n", &[("temp1_input", "99000\n")]);
        mk(
            "hwmon1",
            "coretemp\n",
            &[
                ("temp1_label", "Package id 0\n"),
                ("temp1_input", "54000\n"),
                ("temp2_label", "Core 0\n"),
                ("temp2_input", "40000\n"),
            ],
        );
        mk("hwmon2", "amdgpu\n", &[("temp1_input", "61500\n")]);
        assert_eq!(read_temps(d.path()), (Some(54.0), Some(61.5)));
    }

    #[tokio::test]
    async fn restart_twice_within_100s_is_429() {
        let clock = Arc::new(FakeClock(AtomicU64::new(1000)));
        let r = Restarter::new(Fake::new(Some("")), clock.clone());
        assert_eq!(r.restart().await, Ok(()));
        clock.0.store(1099, Ordering::SeqCst);
        assert_eq!(r.restart().await, Err(RestartError::TooSoon(1)));
        clock.0.store(1100, Ordering::SeqCst);
        assert_eq!(r.restart().await, Ok(()));
    }

    #[tokio::test]
    async fn restart_runs_systemctl_once() {
        let f = Fake::new(Some(""));
        let r = Restarter::new(f.clone(), Arc::new(FakeClock(AtomicU64::new(5))));
        r.restart().await.unwrap();
        let _ = r.restart().await;
        assert_eq!(
            *f.calls.lock().unwrap(),
            ["systemctl --user restart app-dev.lizardbyte.app.Sunshine.service"]
        );
    }

    struct Hang;
    impl Runner for Hang {
        fn run(&self, _: &str, _: &[&str]) -> Option<String> {
            std::thread::sleep(Duration::from_millis(500));
            Some(RELAY.into())
        }
    }

    #[tokio::test]
    async fn hung_tailscale_degrades_to_none() {
        let d = tempfile::tempdir().unwrap();
        let src = Sources {
            hwmon: d.path().join("none"),
            uptime: d.path().join("none"),
            sunshine_addr: "127.0.0.1:1".into(),
            tailscale_timeout: Duration::from_millis(50),
        };
        let i = host_info_with(&src, Arc::new(Hang)).await;
        assert_eq!((i.path, i.relay), (None, None));
    }

    #[tokio::test]
    async fn hung_restart_is_failed_and_slot_restored() {
        let mut r = Restarter::new(Arc::new(Hang), Arc::new(FakeClock(AtomicU64::new(5))));
        r.timeout = Duration::from_millis(50);
        assert_eq!(r.restart().await, Err(RestartError::Failed));
        assert_eq!(*r.last.lock().unwrap(), None);
    }

    #[tokio::test]
    async fn failed_restart_restores_slot() {
        let f = Fake::new(None);
        let r = Restarter::new(f.clone(), Arc::new(FakeClock(AtomicU64::new(5))));
        assert_eq!(r.restart().await, Err(RestartError::Failed));
        assert_eq!(r.restart().await, Err(RestartError::Failed)); // not TooSoon
        assert_eq!(f.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn concurrent_restarts_run_once() {
        let f = Fake::new(Some(""));
        let r = Restarter::new(f.clone(), Arc::new(FakeClock(AtomicU64::new(5))));
        let (a, b) = tokio::join!(r.restart(), r.restart());
        assert_eq!([a, b].iter().filter(|x| **x == Ok(())).count(), 1);
        assert_eq!(f.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn cpu_temp_falls_back_to_temp1() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("hwmon0");
        std::fs::create_dir(&p).unwrap();
        std::fs::write(p.join("name"), "k10temp").unwrap();
        std::fs::write(p.join("temp1_input"), "48000").unwrap();
        assert_eq!(read_temps(d.path()).0, Some(48.0));
    }

    #[test]
    fn real_runner_spawn_does_not_wait() {
        let t = std::time::Instant::now();
        assert!(RealRunner.spawn("sleep", &["5"], &[]));
        assert!(t.elapsed() < Duration::from_secs(1));
        assert!(!RealRunner.spawn("/nonexistent/brisk-steam", &[], &[]));
    }

    #[test]
    fn on_path_finds_executables_only() {
        assert!(on_path("sh") && on_path("/bin/sh"));
        assert!(!on_path("brisk-no-such-prog") && !on_path("/nonexistent/brisk-steam"));
        assert!(!on_path("/etc/hostname"), "not executable");
    }

    #[test]
    fn real_runner_handles_output_over_pipe_buffer() {
        let t = std::time::Instant::now();
        let out = RealRunner
            .run("sh", &["-c", r#"head -c 200000 /dev/zero | tr "\0" a"#])
            .unwrap();
        assert_eq!(out.len(), 200_000);
        assert!(t.elapsed() < Duration::from_secs(5));
    }
}
