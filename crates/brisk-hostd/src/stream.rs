//! The bridge supervisor: at most one `brisk-bridge` child, start/stop serialized, plus the
//! `steam://` commands. The bridge's wt URL carries its own `--listen` address (the tailscale IP
//! from `BRISK_LISTEN`); with the 127.0.0.1 default a remote page cannot reach it.

use crate::config::Config;
use crate::host::Runner;
use crate::sessions::{now_ms, SessionRec, Sessions};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{oneshot, watch};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Fast,
    Balanced,
    Crisp,
    Custom,
}

/// The bridge's `--codec` value; H.264 unless the page asks for HEVC.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    #[default]
    H264,
    Hevc,
}

impl Codec {
    fn flag(self) -> &'static str {
        match self {
            Codec::H264 => "h264",
            Codec::Hevc => "hevc",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamReq {
    pub preset: Preset,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    #[serde(default)]
    pub codec: Codec,
    /// Set on a fresh start from a game; the page omits it for a quality change.
    pub appid: Option<u32>,
}

impl StreamReq {
    /// `[width, height, fps, bitrate_kbps]`, in the bridge's accepted ranges.
    pub fn flags(&self) -> Result<[u32; 4], String> {
        let kbps = match self.preset {
            Preset::Fast => 10000,
            Preset::Balanced => 20000,
            Preset::Crisp => 40000,
            Preset::Custom => {
                let f = [
                    self.width.unwrap_or(1920),
                    self.height.unwrap_or(1080),
                    self.fps.unwrap_or(60),
                    self.bitrate_kbps.unwrap_or(20000),
                ];
                // Same ranges as brisk-bridge's parse_args.
                let ok = (320..=3840).contains(&f[0])
                    && (320..=3840).contains(&f[1])
                    && (24..=144).contains(&f[2])
                    && (1000..=150000).contains(&f[3]);
                return ok.then_some(f).ok_or_else(|| {
                    "custom quality out of range (320-3840 px, 24-144 fps, 1000-150000 kbps)".into()
                });
            }
        };
        Ok([1920, 1080, 60, kbps])
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Starting,
    Live,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct StreamInfo {
    pub state: State,
    pub preset: Option<Preset>,
    pub codec: Option<Codec>,
    pub appid: Option<u32>,
    pub wt: Option<String>,
    pub hash: Option<String>,
}

const IDLE: StreamInfo = StreamInfo {
    state: State::Idle,
    preset: None,
    codec: None,
    appid: None,
    wt: None,
    hash: None,
};

/// What the bridge's watcher task and the request path share. `gen` bumps whenever a request
/// takes over, so a watcher of a replaced bridge leaves the state alone.
struct Shared {
    gen: u64,
    info: StreamInfo,
    rec: Option<SessionRec>,
}

struct Running {
    pid: u32,
    done: watch::Receiver<bool>,
}

pub struct Supervisor {
    bin: PathBuf,
    listen: String,
    app_id: u32,
    cert: PathBuf,
    key: PathBuf,
    runner: Arc<dyn Runner>,
    sessions: Arc<Sessions>,
    shared: Arc<Mutex<Shared>>,
    /// Set by `shutdown`: no bridge may start after it.
    closing: std::sync::atomic::AtomicBool,
    /// Held for a whole start/stop, so requests run one after another.
    child: tokio::sync::Mutex<Option<Running>>,
    /// SIGTERM to exit: the bridge cancels its Sunshine session (Sunshine lingers ~10 s).
    pub stop_wait: Duration,
    pub page_wait: Duration,
}

impl Supervisor {
    pub fn new(cfg: &Config, runner: Arc<dyn Runner>) -> Self {
        // Where scripts/brisk-up.sh creates them.
        let tls = std::env::var_os("HOME")
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
            .join(".config/brisk/tls");
        Supervisor {
            bin: cfg.bridge_bin.clone(),
            listen: cfg.bridge_listen.clone(),
            app_id: cfg.app_id,
            cert: tls.join("bridge.pem"),
            key: tls.join("bridge.key.pem"),
            runner,
            sessions: Arc::new(Sessions {
                path: cfg.data_dir.join("sessions.jsonl"),
            }),
            shared: Arc::new(Mutex::new(Shared {
                gen: 0,
                info: IDLE,
                rec: None,
            })),
            closing: Default::default(),
            child: tokio::sync::Mutex::new(None),
            stop_wait: Duration::from_secs(12),
            page_wait: Duration::from_secs(20),
        }
    }

    pub fn info(&self) -> StreamInfo {
        self.shared.lock().unwrap().info.clone()
    }

    pub fn sessions(&self, n: usize) -> Vec<SessionRec> {
        self.sessions.latest(n)
    }

    /// Stops any running bridge, starts a new one and waits for its `page` line. `game` names
    /// the session record. Callers validate `req` (flags, appid) first.
    pub async fn start(&self, req: StreamReq, game: Option<String>) -> Result<StreamInfo, String> {
        let [w, h, fps, kbps] = req.flags()?;
        let mut slot = self.child.lock().await;
        if self.shutting_down() {
            return Err("hostd is shutting down".into());
        }
        let (prev, prev_rec) = self.stop_locked(&mut slot).await;
        let quality_change = req.appid.is_none() && prev.state == State::Live;
        let appid = req.appid.or(prev.appid.filter(|_| quality_change));
        let mut rec = prev_rec;
        if !quality_change {
            if let Some(r) = rec.take() {
                self.sessions.close(r);
            }
        }
        let gen = {
            let mut s = self.shared.lock().unwrap();
            s.info = StreamInfo {
                state: State::Starting,
                preset: Some(req.preset),
                codec: Some(req.codec),
                appid,
                ..IDLE
            };
            s.gen
        };
        let n = |v: u32| v.to_string();
        let mut cmd = tokio::process::Command::new(&self.bin);
        // SAFETY: prctl is async-signal-safe; the bridge dies with hostd even on SIGKILL.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = cmd
            .args([
                "--host",
                "127.0.0.1",
                "--app",
                &n(self.app_id),
                "--listen",
                &self.listen,
            ])
            .arg("--cert")
            .arg(&self.cert)
            .arg("--key")
            .arg(&self.key)
            .args([
                "--codec",
                req.codec.flag(),
                "--page",
                "http://localhost:8443/",
                "--width",
                &n(w),
                "--height",
                &n(h),
            ])
            .args(["--fps", &n(fps), "--bitrate-kbps", &n(kbps)])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                self.fail(rec.take());
                format!("couldn't start the bridge ({}): {e}", self.bin.display())
            })?;
        let pid = child.id().unwrap_or(0);
        let out = child.stdout.take().expect("piped");
        let (page_tx, page_rx) = oneshot::channel();
        let (done_tx, done_rx) = watch::channel(false);
        let (shared, sessions, runner) = (
            self.shared.clone(),
            self.sessions.clone(),
            self.runner.clone(),
        );
        let mut launch = req.appid;
        // Owns the child: forwards its stdout (keeps the pipe drained), launches the game after
        // the tap connects, and on exit flips to idle unless a request already took over.
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            let mut page_tx = Some(page_tx);
            let mut eof = false;
            // Ends on the child's exit, not on stdout EOF: a grandchild may hold the pipe open.
            loop {
                tokio::select! {
                    l = lines.next_line(), if !eof => match l {
                        Ok(Some(l)) => {
                            if let Some(p) = l.strip_prefix("page\t") {
                                if let (Some(tx), Some(wh)) = (page_tx.take(), parse_page(p)) {
                                    let _ = tx.send(wh);
                                }
                            } else if l.starts_with("tap\tconnected") {
                                if let Some(id) = launch.take() {
                                    let r = runner.clone();
                                    tokio::task::spawn_blocking(move || steam(&*r, "rungameid", id));
                                }
                            }
                            forward(l);
                        }
                        _ => eof = true,
                    },
                    _ = child.wait() => break,
                }
            }
            drop(page_tx);
            let rec = {
                let mut s = shared.lock().unwrap();
                if s.gen != gen {
                    None
                } else {
                    s.info = IDLE;
                    s.rec.take()
                }
            };
            let _ = done_tx.send(true);
            if let Some(r) = rec {
                sessions.close(r);
            }
            // The exit report may still be in the pipe.
            let rest = async {
                while let Ok(Some(l)) = lines.next_line().await {
                    forward(l);
                }
            };
            let _ = tokio::time::timeout(Duration::from_secs(1), rest).await;
        });
        *slot = Some(Running { pid, done: done_rx });
        let (wt, hash) = match tokio::time::timeout(self.page_wait, page_rx).await {
            Ok(Ok(wh)) => wh,
            res => {
                self.stop_locked(&mut slot).await;
                self.fail(rec);
                return Err(match res {
                    Err(_) => "the bridge didn't get ready in time".into(),
                    Ok(_) => "the bridge stopped before it was ready (is port 4433 in use, or Sunshine down?)".into(),
                });
            }
        };
        let rec = rec.unwrap_or_else(|| {
            let r = SessionRec {
                appid,
                name: game.unwrap_or_else(|| "Desktop".into()),
                started: now_ms(),
                ended: None,
                host: hostname(),
                preset: req.preset,
            };
            self.sessions.append(&r);
            r
        });
        let mut s = self.shared.lock().unwrap();
        s.info = StreamInfo {
            state: State::Live,
            preset: Some(req.preset),
            codec: Some(req.codec),
            appid,
            wt: Some(wt),
            hash: Some(hash),
        };
        s.rec = Some(rec);
        Ok(s.info.clone())
    }

    pub fn shutting_down(&self) -> bool {
        self.closing.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Refuses further starts, then stops the bridge.
    pub async fn shutdown(&self) -> StreamInfo {
        self.closing
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.stop().await
    }

    /// Stops the bridge (if any) and closes its session record.
    pub async fn stop(&self) -> StreamInfo {
        let mut slot = self.child.lock().await;
        let (_, rec) = self.stop_locked(&mut slot).await;
        if let Some(r) = rec {
            self.sessions.close(r);
        }
        IDLE
    }

    fn fail(&self, rec: Option<SessionRec>) {
        self.shared.lock().unwrap().info = IDLE;
        if let Some(r) = rec {
            self.sessions.close(r);
        }
    }

    /// Takes over the state (watcher then leaves it alone), SIGTERMs the child and waits for it;
    /// SIGKILL after `stop_wait`. Returns what was running.
    async fn stop_locked(&self, slot: &mut Option<Running>) -> (StreamInfo, Option<SessionRec>) {
        let prev = {
            let mut s = self.shared.lock().unwrap();
            s.gen += 1;
            (std::mem::replace(&mut s.info, IDLE), s.rec.take())
        };
        if let Some(mut r) = slot.take() {
            // done is only set after the watcher reaped the child, so the pid is still ours here.
            if !*r.done.borrow() {
                signal(r.pid, "TERM");
            }
            if tokio::time::timeout(self.stop_wait, r.done.wait_for(|d| *d))
                .await
                .is_err()
            {
                eprintln!(
                    "warning: bridge {} ignored SIGTERM for {:?}, killing",
                    r.pid, self.stop_wait
                );
                signal(r.pid, "KILL");
                let _ = r.done.wait_for(|d| *d).await;
            }
        }
        prev
    }

    /// `steam steam://<verb>/<id>` in the graphical session. Blocking.
    pub fn steam(&self, verb: &str, id: u32) -> bool {
        steam(&*self.runner, verb, id)
    }
}

fn signal(pid: u32, sig: &str) {
    let _ = std::process::Command::new("kill")
        .args([format!("-{sig}"), pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status();
}

/// `http://localhost:8443/?wt=<url>&hash=<hex>` -> (url, hex), the URL unchanged.
fn parse_page(p: &str) -> Option<(String, String)> {
    let q = p.split_once("?wt=")?.1;
    let (wt, hash) = q.split_once("&hash=")?;
    (!wt.is_empty() && !hash.is_empty()).then(|| (wt.to_string(), hash.to_string()))
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map_or_else(|_| "bawn".into(), |s| s.trim().to_string())
}

/// `appid` is a u32 validated by the caller, so the URL cannot carry anything else. Detached:
/// true once steam is running (it hands the URL to the running client and exits).
fn steam(runner: &dyn Runner, verb: &str, id: u32) -> bool {
    runner.spawn(
        "steam",
        &[&format!("steam://{verb}/{id}")],
        &graphical_env(),
    )
}

/// Bridge stdout goes to ours through a bounded queue and a thread, so a closed or full stdout
/// never panics (println! does on EPIPE) or blocks the runtime; lines are dropped instead.
fn forwarder<W: Write + Send + 'static>(mut w: W) -> SyncSender<String> {
    let (tx, rx) = sync_channel::<String>(256);
    std::thread::spawn(move || {
        for l in rx {
            let _ = writeln!(w, "bridge\t{l}");
        }
    });
    tx
}

fn forward(l: String) {
    static TX: OnceLock<SyncSender<String>> = OnceLock::new();
    let _ = TX.get_or_init(|| forwarder(std::io::stdout())).try_send(l);
}

/// `WAYLAND_DISPLAY`/`XDG_RUNTIME_DIR`/`DISPLAY` of the desktop session, read from one of our own
/// `sway` or `steam` processes. [U] On bawn sway's environ is not readable (non-dumpable), so the
/// running steam client is what answers; with neither found hostd's own env is used.
fn graphical_env() -> Vec<(String, String)> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    for e in dir.flatten() {
        let p = e.path();
        let comm = std::fs::read_to_string(p.join("comm")).unwrap_or_default();
        if !matches!(comm.trim(), "sway" | "steam") {
            continue;
        }
        let Ok(env) = std::fs::read(p.join("environ")) else {
            continue;
        };
        let vars: Vec<(String, String)> = env
            .split(|b| *b == 0)
            .filter_map(|v| std::str::from_utf8(v).ok()?.split_once('='))
            .filter(|(k, _)| ["WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DISPLAY"].contains(k))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        if vars.iter().any(|(k, _)| k == "WAYLAND_DISPLAY") {
            return vars;
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_page_line() {
        assert_eq!(
            parse_page("http://localhost:8443/?wt=https://100.1.2.3:4433/ab&hash=0f"),
            Some(("https://100.1.2.3:4433/ab".into(), "0f".into()))
        );
        assert_eq!(parse_page("http://localhost:8443/"), None);
        assert_eq!(parse_page("http://x/?wt=&hash="), None);
    }

    /// A broken or stuck stdout must neither panic the forwarder nor block the sender.
    #[test]
    fn forwarder_survives_broken_and_stuck_stdout() {
        use std::sync::mpsc::TrySendError;
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        struct Stuck;
        impl Write for Stuck {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                std::thread::sleep(Duration::from_secs(3600));
                Ok(0)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let t = std::time::Instant::now();
        let (b, st) = (forwarder(Broken), forwarder(Stuck));
        for i in 0..1000 {
            let _ = b.try_send(i.to_string());
            let _ = st.try_send(i.to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(!matches!(
            b.try_send("x".into()),
            Err(TrySendError::Disconnected(_))
        ));
        assert!(t.elapsed() < Duration::from_secs(2));
    }
}
