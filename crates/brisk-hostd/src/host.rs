//! Host status (`GET /api/host`) and the rate-limited Sunshine restart. Every source fails to `None`.

use crate::config::Config;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const RESTART_GAP_S: u64 = 100;
/// Same port and probe as `serverinfo_free` in scripts/brisk-common.sh.
const SUNSHINE_ADDR: &str = "127.0.0.1:47989";

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Sunshine {
    Ready,
    Busy,
    Down,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
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
    fn run(&self, prog: &str, args: &[&str]) -> Option<String>;
}

pub struct RealRunner;
impl Runner for RealRunner {
    fn run(&self, prog: &str, args: &[&str]) -> Option<String> {
        let o = std::process::Command::new(prog).args(args).output().ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }
}

pub trait Clock: Send + Sync {
    fn now_s(&self) -> u64;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_s(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// Where host facts come from; tests point these at fixtures.
pub struct Sources {
    pub hwmon: PathBuf,
    pub uptime: PathBuf,
    pub sunshine_addr: String,
}

impl Default for Sources {
    fn default() -> Self {
        Sources {
            hwmon: "/sys/class/hwmon".into(),
            uptime: "/proc/uptime".into(),
            sunshine_addr: SUNSHINE_ADDR.into(),
        }
    }
}

pub async fn host_info(cfg: &Config) -> HostInfo {
    let r = Arc::new(RealRunner);
    host_info_with(cfg, &Sources::default(), r).await
}

pub async fn host_info_with(_cfg: &Config, src: &Sources, runner: Arc<dyn Runner>) -> HostInfo {
    let ts = tokio::task::spawn_blocking(move || runner.run("tailscale", &["status", "--json"]))
        .await
        .ok()
        .flatten();
    let (path, relay) = ts.as_deref().map_or((None, None), parse_tailscale);
    let (cpu, gpu) = read_temps(&src.hwmon);
    HostInfo {
        name: std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map_or_else(|_| "bawn".into(), |s| s.trim().to_string()),
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
                for i in 1..=32 {
                    let label = std::fs::read_to_string(d.join(format!("temp{i}_label")));
                    if label.is_ok_and(|l| l.starts_with("Package") || l.trim() == "Tctl") {
                        cpu = read(&format!("temp{i}_input"));
                        break;
                    }
                }
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
}

#[derive(Debug, PartialEq)]
pub enum RestartError {
    TooSoon,
    Failed,
}

impl Restarter {
    pub fn new(runner: Arc<dyn Runner>, clock: Arc<dyn Clock>) -> Self {
        Restarter {
            runner,
            clock,
            last: Mutex::new(None),
        }
    }

    pub async fn restart(&self) -> Result<(), RestartError> {
        let now = self.clock.now_s();
        let prev = {
            let mut last = self.last.lock().unwrap();
            if last.is_some_and(|l| now < l + RESTART_GAP_S) {
                return Err(RestartError::TooSoon);
            }
            last.replace(now) // claimed before running, so a double click cannot slip through
        };
        let r = self.runner.clone();
        let ok = tokio::task::spawn_blocking(move || {
            r.run(
                "systemctl",
                &[
                    "--user",
                    "restart",
                    "app-dev.lizardbyte.app.Sunshine.service",
                ],
            )
        })
        .await
        .ok()
        .flatten()
        .is_some();
        if ok {
            Ok(())
        } else {
            *self.last.lock().unwrap() = prev;
            Err(RestartError::Failed)
        }
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
        };
        let cfg = Config::from_env(d.path().into());
        let i = host_info_with(&cfg, &src, Fake::new(None)).await;
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
        assert_eq!(r.restart().await, Err(RestartError::TooSoon));
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
}
