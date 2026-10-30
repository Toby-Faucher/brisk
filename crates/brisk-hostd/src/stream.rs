//! The bridge supervisor: at most one `brisk-bridge` child, start/stop serialized, plus the
//! `steam://` commands. The bridge's wt URL carries its own `--listen` address (`BRISK_LISTEN`,
//! else the tailscale IP looked up at each start, see `resolve_listen`).

use crate::config::Config;
use crate::game::{Game, GameId, Source};
use crate::host::Runner;
use crate::sessions::{Report, SessionRec, Sessions, now_ms};
use crate::source::of;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{oneshot, watch};

/// A named set of stream quality settings.
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

/// What a client asks for when it starts a stream.
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
    /// Set on a fresh start from a game; the page omits both for a quality change. `id` is the new
    /// string id (or a bare number); `appid` is the old numeric field, still read.
    pub appid: Option<u32>,
    pub id: Option<RawId>,
}

/// A game id as a request carries it: text (`steam:620`) or a bare number.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RawId {
    Num(u32),
    Text(String),
}

impl RawId {
    /// The id as text.
    pub fn text(&self) -> String {
        match self {
            RawId::Num(n) => n.to_string(),
            RawId::Text(t) => t.clone(),
        }
    }
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

/// Whether a stream is running.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Starting,
    Live,
}

/// A snapshot of the current stream for the API.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct StreamInfo {
    pub state: State,
    pub preset: Option<Preset>,
    pub codec: Option<Codec>,
    pub appid: Option<u32>,
    #[serde(rename = "gameId")]
    pub game_id: Option<String>,
    pub wt: Option<String>,
    pub hash: Option<String>,
    /// The live session's id (its record's `started`), for `PUT /api/sessions/<started>/series`.
    pub started: Option<u64>,
    /// Seconds since that session started on hostd's monotonic clock: the page's series time base
    /// (the laptop's wall clock may be off).
    #[serde(rename = "elapsedS")]
    pub elapsed_s: Option<f64>,
}

const IDLE: StreamInfo = StreamInfo {
    state: State::Idle,
    preset: None,
    codec: None,
    appid: None,
    game_id: None,
    wt: None,
    hash: None,
    started: None,
    elapsed_s: None,
};

/// What the bridge's watcher task and the request path share. `epoch` bumps whenever a request
/// takes over, so a watcher of a replaced bridge leaves the state alone.
struct Shared {
    epoch: u64,
    info: StreamInfo,
    rec: Option<SessionRec>,
    /// `started` of the newest session and the Instant it was taken at.
    clock: Option<(u64, std::time::Instant)>,
}

impl Shared {
    fn view(&self) -> StreamInfo {
        let mut i = self.info.clone();
        if let (Some(id), Some((c, t0))) = (i.started, self.clock) {
            i.elapsed_s = (id == c).then(|| t0.elapsed().as_secs_f64());
        }
        i
    }
}

struct Running {
    pid: u32,
    done: watch::Receiver<bool>,
    /// The bridge's end-of-session report, complete once `done`.
    report: Arc<Mutex<Report>>,
}

/// Starts, watches and stops the bridge process that streams a game.
pub struct Supervisor {
    bin: PathBuf,
    proc_dir: PathBuf,
    data_dir: PathBuf,
    listen: Option<String>,
    app_id: u32,
    tls: PathBuf,
    /// Service mode: bridge lines with the session token are not re-printed (see `bridge_line`).
    pub quiet: bool,
    runner: Arc<dyn Runner>,
    pub sessions: Arc<Sessions>,
    shared: Arc<Mutex<Shared>>,
    /// Set by `shutdown`: no bridge may start after it.
    closing: std::sync::atomic::AtomicBool,
    /// Held for a whole start/stop, so requests run one after another.
    child: tokio::sync::Mutex<Option<Running>>,
    /// SIGTERM to exit: the bridge cancels its Sunshine session (Sunshine lingers ~10 s).
    pub stop_wait: Duration,
    pub page_wait: Duration,
    /// Gap between looks for a launched game's window.
    pub focus_poll: Duration,
}

impl Supervisor {
    /// A supervisor with no bridge running.
    pub fn new(cfg: &Config, runner: Arc<dyn Runner>) -> Self {
        Supervisor {
            bin: cfg.bridge_bin.clone(),
            proc_dir: cfg.proc_dir.clone(),
            data_dir: cfg.data_dir.clone(),
            listen: cfg.bridge_listen.clone(),
            app_id: cfg.app_id,
            tls: cfg.tls_dir.clone(),
            quiet: cfg.service,
            runner,
            sessions: Arc::new(Sessions::new(cfg.data_dir.join("sessions.jsonl"))),
            shared: Arc::new(Mutex::new(Shared {
                epoch: 0,
                info: IDLE,
                rec: None,
                clock: None,
            })),
            closing: Default::default(),
            child: tokio::sync::Mutex::new(None),
            stop_wait: Duration::from_secs(12),
            page_wait: Duration::from_secs(20),
            focus_poll: Duration::from_secs(1),
        }
    }

    /// The current stream state.
    pub fn info(&self) -> StreamInfo {
        self.shared.lock().expect("lock poisoned").view()
    }

    /// `info` without blocking: `None` while the state lock is held (or poisoned).
    pub(crate) fn try_info(&self) -> Option<StreamInfo> {
        self.shared.try_lock().ok().map(|s| s.view())
    }

    /// The `n` newest sessions.
    pub fn sessions(&self, n: usize) -> Vec<SessionRec> {
        self.sessions.latest(n)
    }

    /// Stops any running bridge, starts a new one and waits for its `page` line. `game` names
    /// the session record. Callers validate `req` (flags) and resolve the game first; `game` is
    /// set on a fresh start from a game and None for a quality change or the Desktop.
    pub async fn start(&self, req: StreamReq, game: Option<Game>) -> Result<StreamInfo, String> {
        let [w, h, fps, kbps] = req.flags()?;
        let mut slot = self.child.lock().await;
        if self.shutting_down() {
            return Err("hostd is shutting down".into());
        }
        // Before the old bridge is stopped: a missing tailscale or openssl leaves it running.
        let (r, conf, tls) = (self.runner.clone(), self.listen.clone(), self.tls.clone());
        let (listen, (cert, key)) = tokio::task::spawn_blocking(move || {
            Ok::<_, String>((
                resolve_listen(&*r, conf.as_deref())?,
                ensure_cert(&*r, &tls, std::time::SystemTime::now())?,
            ))
        })
        .await
        .map_err(|e| e.to_string())??;
        let (prev, prev_rec) = self.stop_locked(&mut slot).await;
        let quality_change = game.is_none() && prev.state == State::Live;
        let (appid, game_id) = match &game {
            Some(g) => (Some(g.appid), Some(g.id.to_string())),
            None if quality_change => (prev.appid, prev.game_id.clone()),
            None => (None, None),
        };
        let mut rec = prev_rec;
        if !quality_change && let Some(r) = rec.take() {
            self.sessions.close(r);
        }
        let epoch = {
            let mut s = self.shared.lock().expect("lock poisoned");
            s.info = StreamInfo {
                state: State::Starting,
                preset: Some(req.preset),
                codec: Some(req.codec),
                appid,
                game_id: game_id.clone(),
                ..IDLE
            };
            s.epoch
        };
        let n = |v: u32| v.to_string();
        let mut cmd = tokio::process::Command::new(&self.bin);
        crate::notify::scrub(cmd.as_std_mut());
        // SAFETY: the hook only makes one raw prctl syscall (rustix, no allocation or locks), which
        // is async-signal-safe after fork; the bridge dies with hostd even on SIGKILL.
        unsafe {
            cmd.pre_exec(|| {
                let _ = rustix::process::set_parent_process_death_signal(Some(
                    rustix::process::Signal::TERM,
                ));
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
                &listen,
            ])
            .arg("--cert")
            .arg(&cert)
            .arg("--key")
            .arg(&key)
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
        let mut launch = game.as_ref().map(|g| g.id.clone());
        let (proc, data) = (self.proc_dir.clone(), self.data_dir.clone());
        let (quiet, poll) = (self.quiet, self.focus_poll);
        let report = Arc::new(Mutex::new(Report::default()));
        let rep = report.clone();
        // Owns the child: forwards its stdout (keeps the pipe drained), launches the game after
        // the tap connects, and on exit flips to idle unless a request already took over.
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            let mut page_tx = Some(page_tx);
            let mut eof = false;
            let mut tok: Option<String> = None;
            // Ends on the child's exit, not on stdout EOF: a grandchild may hold the pipe open.
            loop {
                tokio::select! {
                    l = lines.next_line(), if !eof => match l {
                        Ok(Some(l)) => {
                            if let Some(p) = l.strip_prefix("page\t") {
                                if let Some(wh) = parse_page(p) {
                                    tok = wh.0.rsplit('/').next().filter(|t| t.len() >= 8).map(String::from);
                                    if let Some(tx) = page_tx.take() {
                                        let _ = tx.send(wh);
                                    }
                                }
                            } else if l.starts_with("tap\tconnected")
                                && let Some(id) = launch.take()
                            {
                                tokio::spawn(launch_and_focus(runner.clone(), proc.clone(), data.clone(), id, poll));
                            }
                            rep.lock().expect("lock poisoned").line(&l);
                            forward(bridge_line(l, quiet, tok.as_deref()));
                        }
                        _ => eof = true,
                    },
                    _ = child.wait() => break,
                }
            }
            drop(page_tx);
            // The exit report may still be in the pipe: read it before the session is handed over.
            let rest = async {
                while let Ok(Some(l)) = lines.next_line().await {
                    rep.lock().expect("lock poisoned").line(&l);
                    forward(bridge_line(l, quiet, tok.as_deref()));
                }
            };
            if !eof {
                let _ = tokio::time::timeout(Duration::from_secs(1), rest).await;
            }
            let rec = {
                let mut s = shared.lock().expect("lock poisoned");
                if s.epoch != epoch {
                    None
                } else {
                    s.info = IDLE;
                    s.rec.take()
                }
            };
            let _ = done_tx.send(true);
            if let Some(mut r) = rec {
                r.add_stats(std::mem::take(
                    &mut rep.lock().expect("lock poisoned").stats,
                ));
                sessions.close(r);
            }
        });
        *slot = Some(Running {
            pid,
            done: done_rx,
            report,
        });
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
        // The watcher may have seen the bridge exit after it sent the page line. It resets the state
        // under this lock, so a dead bridge shows as done or as a state that is no longer Starting.
        let gone = {
            let s = self.shared.lock().expect("lock poisoned");
            s.info.state != State::Starting || slot.as_ref().is_none_or(|r| *r.done.borrow())
        };
        if gone {
            self.stop_locked(&mut slot).await;
            self.fail(rec);
            return Err("the bridge stopped before it was ready".into());
        }
        let mut fresh = None;
        let rec = match rec {
            Some(mut r) => {
                (r.preset, r.codec, r.bitrate_kbps) = (req.preset, Some(req.codec), Some(kbps));
                r
            }
            None => {
                let r = SessionRec {
                    appid,
                    game_id: game_id.clone(),
                    name: game.map_or_else(|| "Desktop".into(), |g| g.name),
                    started: now_ms(),
                    ended: None,
                    host: crate::host::hostname(),
                    preset: req.preset,
                    codec: Some(req.codec),
                    bitrate_kbps: Some(kbps),
                    stats: None,
                };
                fresh = Some((r.started, std::time::Instant::now()));
                self.sessions.append(&r);
                r
            }
        };
        let mut s = self.shared.lock().expect("lock poisoned");
        s.info = StreamInfo {
            state: State::Live,
            preset: Some(req.preset),
            codec: Some(req.codec),
            appid,
            game_id,
            wt: Some(wt),
            hash: Some(hash),
            started: Some(rec.started),
            elapsed_s: None,
        };
        if fresh.is_some() {
            s.clock = fresh;
        }
        s.rec = Some(rec);
        Ok(s.view())
    }

    /// True once shutdown began.
    pub(crate) fn shutting_down(&self) -> bool {
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
        self.shared.lock().expect("lock poisoned").info = IDLE;
        if let Some(r) = rec {
            self.sessions.close(r);
        }
    }

    /// Takes over the state (watcher then leaves it alone), SIGTERMs the child and waits for it;
    /// SIGKILL after `stop_wait`. Returns what was running.
    async fn stop_locked(&self, slot: &mut Option<Running>) -> (StreamInfo, Option<SessionRec>) {
        let mut prev = {
            let mut s = self.shared.lock().expect("lock poisoned");
            s.epoch += 1;
            (std::mem::replace(&mut s.info, IDLE), s.rec.take())
        };
        if let Some(mut r) = slot.take() {
            // done is only set after the watcher reaped the child, so the pid is still ours here.
            if !*r.done.borrow() {
                signal(r.pid, rustix::process::Signal::TERM);
            }
            if tokio::time::timeout(self.stop_wait, r.done.wait_for(|d| *d))
                .await
                .is_err()
            {
                tracing::warn!(
                    "bridge {} ignored SIGTERM for {:?}, killing",
                    r.pid,
                    self.stop_wait
                );
                signal(r.pid, rustix::process::Signal::KILL);
                let _ = r.done.wait_for(|d| *d).await;
            }
            if let Some(rec) = &mut prev.1 {
                rec.add_stats(std::mem::take(
                    &mut r.report.lock().expect("lock poisoned").stats,
                ));
            }
        }
        prev
    }

    /// Install through the game's source; None when it has nothing to install. Blocking.
    pub fn install(&self, id: &GameId) -> Option<bool> {
        of(id).install(&*self.runner, id)
    }

    /// Starts the game through its source (an emulator already running it is left alone), stamps
    /// last-played if the source keeps one, and for a source that names a window class focuses
    /// that window in the background for up to a minute. Blocking; needs a tokio runtime context.
    pub fn launch(&self, id: &GameId) -> bool {
        let (src, env) = (of(id), graphical_env());
        if !src.launch(&*self.runner, &self.proc_dir, &self.data_dir, id, &env) {
            return false;
        }
        src.after_launch(&self.data_dir, id);
        if src.focus_class(&self.data_dir, id).is_some() {
            let (r, p, d, id, poll) = (
                self.runner.clone(),
                self.proc_dir.clone(),
                self.data_dir.clone(),
                id.clone(),
                self.focus_poll,
            );
            tokio::spawn(
                async move { focus(r, p, d, env, id, Duration::from_secs(60), poll).await },
            );
        }
        true
    }
}

/// The bridge's address: `BRISK_LISTEN`, else this machine's tailscale IPv4 on :4433, asked at
/// each start (bawn may boot before tailscale is up). Never all interfaces. Blocking.
pub fn resolve_listen(runner: &dyn Runner, configured: Option<&str>) -> Result<String, String> {
    let a = match configured {
        Some(a) => a.to_string(),
        None => {
            let ip = runner
                .run("tailscale", &["ip", "-4"])
                .and_then(|o| o.lines().next().map(|l| l.trim().to_string()))
                .filter(|l| !l.is_empty())
                .ok_or("tailscale gave no IPv4 address (is tailscale up? or set BRISK_LISTEN)")?;
            format!("{ip}:4433")
        }
    };
    match a.parse::<std::net::SocketAddrV4>() {
        Ok(s) if s.ip().is_unspecified() => {
            Err("refusing to listen on all interfaces (0.0.0.0)".into())
        }
        Ok(_) => Ok(a),
        Err(_) => Err(format!("bridge listen address '{a}' is not IPv4:PORT")),
    }
}

const CERT_DAYS: &str = "13"; // Chrome's serverCertificateHashes allows at most 14 days
const CERT_RENEW_S: u64 = 10 * 86400;

/// `bridge.pem` / `bridge.key.pem` in `dir`, made with openssl when missing or older than 10
/// days (the policy scripts/brisk-up.sh had): ECDSA P-256, 13 days, both 0600. The new pair is
/// written next to the old one and renamed over it, so a failed run leaves the old pair. Blocking.
pub fn ensure_cert(
    runner: &dyn Runner,
    dir: &std::path::Path,
    now: std::time::SystemTime,
) -> Result<(PathBuf, PathBuf), String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let (cert, key) = (dir.join("bridge.pem"), dir.join("bridge.key.pem"));
    let mtime = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    // Fresh = both present, the key no newer than its cert (else they are from different runs,
    // e.g. a renewal cut off between the two renames), and the cert's mtime not in the future.
    let fresh = match (mtime(&cert), mtime(&key)) {
        (Some(c), Some(k)) if k <= c => now
            .duration_since(c)
            .is_ok_and(|a| a.as_secs() <= CERT_RENEW_S),
        _ => false,
    };
    if fresh {
        return Ok((cert, key));
    }
    let fail = |e: String| {
        format!(
            "couldn't make the bridge certificate in {}: {e}",
            dir.display()
        )
    };
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|e| fail(e.to_string()))?;
    let (tc, tk) = (dir.join(".bridge.pem.new"), dir.join(".bridge.key.pem.new"));
    // Made 0600 first: openssl truncates and keeps the mode, so the key is never world-readable.
    for p in [&tc, &tk] {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(p)
            .map_err(|e| fail(e.to_string()))?;
    }
    let (tcs, tks) = (tc.to_string_lossy(), tk.to_string_lossy());
    let ran = runner.run(
        "openssl",
        &[
            "req",
            "-x509",
            "-newkey",
            "ec",
            "-pkeyopt",
            "ec_paramgen_curve:prime256v1",
            "-nodes",
            "-days",
            CERT_DAYS,
            "-subj",
            "/CN=brisk-bridge",
            "-keyout",
            &tks,
            "-out",
            &tcs,
        ],
    );
    let pem = |p: &PathBuf| std::fs::read_to_string(p).is_ok_and(|s| s.contains("-----BEGIN"));
    // Say which step failed; the runner gives no stderr, so this is the most it can tell.
    let step = if ran.is_none() {
        Some("openssl did not run or exited with an error (is it installed?)")
    } else if !pem(&tc) {
        Some("openssl wrote no PEM certificate")
    } else if !pem(&tk) {
        Some("openssl wrote no PEM key")
    } else {
        None
    };
    if let Some(step) = step {
        let _ = (std::fs::remove_file(&tc), std::fs::remove_file(&tk));
        tracing::warn!("bridge certificate: {step}");
        return Err(fail(step.into()));
    }
    // Key first: if we die in between, the old cert's age makes the next start renew again.
    std::fs::rename(&tk, &key)
        .and_then(|_| std::fs::rename(&tc, &cert))
        .map_err(|e| fail(e.to_string()))?;
    // One mtime for the pair, whatever order openssl wrote them in (see `fresh` above).
    if let Some(c) = mtime(&cert)
        && let Err(e) = std::fs::File::options()
            .write(true)
            .open(&key)
            .and_then(|f| f.set_modified(c))
    {
        // Harmless but noisy: the next start sees a mismatched pair and renews once more.
        tracing::warn!(
            "couldn't align {}'s mtime with the cert: {e}",
            key.display()
        );
    }
    tracing::info!(
        "new bridge certificate {} ({CERT_DAYS} days)",
        cert.display()
    );
    Ok((cert, key))
}

/// `pid` is our own unreaped child, so its number cannot have been reused.
fn signal(pid: u32, sig: rustix::process::Signal) {
    if let Some(p) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process(p, sig);
    }
}

/// `http://localhost:8443/?wt=<url>&hash=<hex>` -> (url, hex), the URL unchanged.
fn parse_page(p: &str) -> Option<(String, String)> {
    let q = p.split_once("?wt=")?.1;
    let (wt, hash) = q.split_once("&hash=")?;
    (!wt.is_empty() && !hash.is_empty()).then(|| (wt.to_string(), hash.to_string()))
}

/// Waits up to `wait` for the game's window, then focuses and fullscreens it: found by the
/// source's window class, else through the game's process tree. A miss is only logged.
pub(crate) async fn focus(
    runner: Arc<dyn Runner>,
    proc: PathBuf,
    data: PathBuf,
    env: Vec<(String, String)>,
    id: GameId,
    wait: Duration,
    poll: Duration,
) {
    if let Some(class) = of(&id).focus_class(&data, &id) {
        crate::running::focus_class(runner, env, class, id.to_string(), wait, poll).await;
    } else if let Some(n) = id.steam_appid() {
        crate::running::focus_game(runner, proc, env, n, wait, poll).await;
    }
}

/// A running game is only brought to the front: launching it again does nothing useful and
/// leaves its window small on the desktop. Then waits up to a minute for the window to fullscreen it.
async fn launch_and_focus(
    runner: Arc<dyn Runner>,
    proc: PathBuf,
    data: PathBuf,
    id: GameId,
    poll: Duration,
) {
    let (r, p, d, i) = (runner.clone(), proc.clone(), data.clone(), id.clone());
    let (ok, env) = tokio::task::spawn_blocking(move || {
        let src = of(&i);
        let env = graphical_env();
        let running = src.scope(&crate::running::procs(&p), &d, &i).is_some();
        // The emulator source skips a spawn for a running game itself; Steam is told here.
        let ok = if running && i.source() == Source::Steam {
            tracing::info!("game {i} is already running; streaming it without relaunch");
            true
        } else {
            src.launch(&*r, &p, &d, &i, &env)
        };
        if ok {
            src.after_launch(&d, &i);
        }
        (ok, env)
    })
    .await
    .unwrap_or_default();
    if ok {
        focus(runner, proc, data, env, id, Duration::from_secs(60), poll).await;
    }
}

/// `id` is a u32 validated by the caller, so the URL cannot carry anything else. Detached:
/// true once steam is running (it hands the URL to the running client and exits).
pub(crate) fn steam(runner: &dyn Runner, verb: &str, id: u32) -> bool {
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

/// What hostd re-prints of a bridge stdout line. `quiet` (service mode: stdout is the journal)
/// keeps the session token out: the `page` line and anything carrying `?wt=` or the token
/// itself (it grants stream and input for the session) become `page <redacted>`.
pub fn bridge_line(l: String, quiet: bool, tok: Option<&str>) -> String {
    let secret = l.starts_with("page\t") || l.contains("wt=") || tok.is_some_and(|t| l.contains(t));
    if quiet && secret {
        "page <redacted>".into()
    } else {
        l
    }
}

fn forward(l: String) {
    static TX: OnceLock<SyncSender<String>> = OnceLock::new();
    let _ = TX.get_or_init(|| forwarder(std::io::stdout())).try_send(l);
}

/// `WAYLAND_DISPLAY`/`XDG_RUNTIME_DIR`/`DISPLAY`/`SWAYSOCK` of the desktop session, read from one of our own
/// `sway` or `steam` processes. [U] On bawn sway's environ is not readable (non-dumpable), so the
/// running steam client is what answers; with neither found hostd's own env is used.
pub(crate) fn graphical_env() -> Vec<(String, String)> {
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
            .filter(|(k, _)| {
                ["WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "DISPLAY", "SWAYSOCK"].contains(k)
            })
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
