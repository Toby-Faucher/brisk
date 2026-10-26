//! bawn's Wayland clipboard for `/api/clipboard`: a watcher thread keeps the current text or PNG,
//! a seq that bumps when it changes and its hash, so what the page sent is not echoed back as a
//! change. The real backend is wl-clipboard (`wl-paste`, `wl-copy`) in the sway session.

use axum::body::Bytes;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

/// Bigger clipboards are ignored both ways.
pub const MAX: usize = 16 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Text,
    Image,
}

impl Kind {
    pub fn mime(self) -> &'static str {
        match self {
            Kind::Text => "text/plain;charset=utf-8",
            Kind::Image => "image/png",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub kind: Kind,
    pub data: Bytes,
}

fn hash(c: &Clip) -> u64 {
    let mut h = DefaultHasher::new();
    c.kind.hash(&mut h);
    c.data.hash(&mut h);
    h.finish()
}

/// The clipboard of the desktop session. All methods block.
pub trait Backend: Send + Sync {
    /// Text or PNG on the clipboard now; None if empty, another type, or over `MAX`.
    fn read(&self) -> Option<Clip>;
    /// Takes the clipboard with `c`.
    fn write(&self, c: &Clip) -> bool;
    /// Returns when the clipboard may have changed.
    fn wait_change(&self);
}

struct Cur {
    seq: u64,
    hash: Option<u64>,
    clip: Option<Clip>,
}

pub struct Clipboard {
    backend: Arc<dyn Backend>,
    cur: Mutex<Cur>,
    watch: Once,
}

impl Clipboard {
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Clipboard {
            backend,
            cur: Mutex::new(Cur {
                seq: 0,
                hash: None,
                clip: None,
            }),
            watch: Once::new(),
        }
    }

    /// Starts the watcher thread once (main does this; tests with a fake backend too).
    pub fn start_watch(self: &Arc<Self>) {
        self.watch.call_once(|| {
            let me = self.clone();
            std::thread::spawn(move || {
                loop {
                    me.observe(me.backend.read());
                    me.backend.wait_change();
                }
            });
        });
    }

    fn observe(&self, c: Option<Clip>) {
        let h = c.as_ref().map(hash);
        let mut s = self.cur.lock().unwrap();
        if h != s.hash {
            s.seq += 1;
            s.hash = h;
            s.clip = c;
        }
    }

    /// The current seq, plus the content if it is newer than `since` (None: only the seq).
    pub fn since(&self, since: Option<u64>) -> (u64, Option<Clip>) {
        let s = self.cur.lock().unwrap();
        let newer = since.is_some_and(|n| n != s.seq);
        (s.seq, s.clip.clone().filter(|_| newer))
    }

    /// Sets the clipboard from the page. The hash is recorded first, so the watcher seeing our own
    /// write is no change and the page does not get it back. Blocking.
    /// On failure the previous hash and clip are restored.
    pub fn set(&self, c: Clip) -> bool {
        let prev = {
            let mut s = self.cur.lock().unwrap();
            let prev = (s.hash, s.clip.clone());
            s.hash = Some(hash(&c));
            s.clip = Some(c.clone());
            prev
        };
        let ok = self.backend.write(&c);
        if !ok {
            let mut s = self.cur.lock().unwrap();
            (s.hash, s.clip) = prev;
        }
        ok
    }
}

/// wl-clipboard in the graphical session. `wl-paste --watch` wakes the watcher; each read is a
/// fresh `wl-paste` under a 5 s `timeout` so a stuck source client cannot hang it.
#[derive(Default)]
pub struct WlBackend {
    watch: Mutex<Option<(Child, BufReader<ChildStdout>)>>,
    /// The `wl-copy --foreground` serving our last write; it exits once someone else copies.
    copy: Mutex<Option<Child>>,
}

/// `WAYLAND_DISPLAY`/`XDG_RUNTIME_DIR` as stream.rs finds them, else the first `wayland-*` socket
/// in the runtime dir (hostd started over ssh with no steam running has neither).
fn wayland_env() -> Vec<(String, String)> {
    let mut env = crate::stream::graphical_env();
    if env.iter().any(|(k, _)| k == "WAYLAND_DISPLAY")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        return env;
    }
    let run = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", rustix::process::getuid().as_raw()));
    let sock = std::fs::read_dir(&run).ok().and_then(|d| {
        d.flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("wayland-") && !n.ends_with(".lock"))
            .min()
    });
    if let Some(s) = sock {
        env.retain(|(k, _)| k != "XDG_RUNTIME_DIR");
        env.push(("XDG_RUNTIME_DIR".into(), run));
        env.push(("WAYLAND_DISPLAY".into(), s));
    }
    env
}

fn cmd(prog: &str, args: &[&str]) -> Command {
    let mut c = Command::new(prog);
    c.args(args)
        .envs(wayland_env())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

/// `wl-paste <args>` stdout, None on failure or over `MAX`.
fn paste(args: &[&str]) -> Option<Vec<u8>> {
    let mut a = vec!["5", "wl-paste"];
    a.extend_from_slice(args);
    let mut c = cmd("timeout", &a).stdout(Stdio::piped()).spawn().ok()?;
    let mut out = Vec::new();
    let _ = c.stdout.take()?.take(MAX as u64 + 1).read_to_end(&mut out);
    // The pipe is closed here: an oversized paste dies on SIGPIPE instead of blocking.
    let ok = c.wait().is_ok_and(|s| s.success());
    (ok && out.len() <= MAX).then_some(out)
}

fn reap(slot: &mut Option<Child>) {
    if let Some(c) = slot.as_mut()
        && !matches!(c.try_wait(), Ok(None))
    {
        *slot = None;
    }
}

impl Backend for WlBackend {
    fn read(&self) -> Option<Clip> {
        let types = String::from_utf8(paste(&["--list-types"])?).ok()?;
        let has = |f: &dyn Fn(&str) -> bool| types.lines().any(f);
        let kind = if has(&|t| t == "image/png") {
            Kind::Image
        } else if has(&|t| t.starts_with("text/plain") || t == "UTF8_STRING") {
            Kind::Text
        } else {
            return None;
        };
        let t = if kind == Kind::Image {
            "image/png"
        } else {
            "text"
        };
        let data = paste(&["--no-newline", "--type", t])?;
        Some(Clip {
            kind,
            data: data.into(),
        })
    }

    fn write(&self, c: &Clip) -> bool {
        let mut slot = self.copy.lock().unwrap();
        let Ok(mut new) = cmd("wl-copy", &["--foreground", "--type", c.kind.mime()])
            .stdin(Stdio::piped())
            .spawn()
        else {
            return false;
        };
        // wl-copy reads all of stdin before it takes the selection; dropping stdin ends it.
        let fed = new
            .stdin
            .take()
            .is_some_and(|mut i| i.write_all(&c.data).is_ok());
        // shortcut: a wl-copy that cannot take the selection (no session) exits at once; one still
        // running after 50 ms is taken as the owner. A slower failure is not seen.
        std::thread::sleep(Duration::from_millis(50));
        if !fed || !matches!(new.try_wait(), Ok(None)) {
            let _ = new.kill();
            let _ = new.wait();
            return false;
        }
        // The previous wl-copy exits once the new one owns the selection; killing it first would
        // empty the clipboard for a moment (a spurious change). Give it a second, then kill.
        if let Some(mut old) = slot.replace(new) {
            let t = Instant::now();
            while matches!(old.try_wait(), Ok(None)) && t.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = old.kill();
            let _ = old.wait();
        }
        true
    }

    fn wait_change(&self) {
        reap(&mut self.copy.lock().unwrap());
        let mut w = self.watch.lock().unwrap();
        if w.is_none() {
            use std::os::unix::process::CommandExt;
            let mut c = cmd("wl-paste", &["--watch", "echo"]);
            c.stdout(Stdio::piped());
            // SAFETY: the hook only makes one raw prctl syscall (rustix, no allocation or locks),
            // which is async-signal-safe after fork. The death signal follows the spawning
            // thread, which is the watcher thread: it lives as long as hostd.
            unsafe {
                c.pre_exec(|| {
                    let _ = rustix::process::set_parent_process_death_signal(Some(
                        rustix::process::Signal::TERM,
                    ));
                    Ok(())
                });
            }
            match c.spawn() {
                Ok(mut ch) => {
                    let out = ch.stdout.take().map(BufReader::new);
                    *w = out.map(|o| (ch, o));
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_secs(5));
                    return;
                }
            }
        }
        let mut line = String::new();
        let got = w
            .as_mut()
            .is_some_and(|(_, r)| r.read_line(&mut line).is_ok_and(|n| n > 0));
        if !got {
            // wl-paste --watch exited (sway restarted, no session): reap it, retry soon.
            if let Some((mut ch, _)) = w.take() {
                let _ = ch.kill();
                let _ = ch.wait();
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live check against the running sway session: `cargo test -p brisk-hostd -- --ignored live_`.
    /// Takes the desktop clipboard, then clears it.
    #[test]
    #[ignore]
    fn live_wayland_roundtrip() {
        let b = Arc::new(WlBackend::default());
        let c = Arc::new(Clipboard::new(b.clone()));
        c.start_watch();
        std::thread::sleep(Duration::from_millis(500));
        let (s0, _) = c.since(None);
        let clip = Clip {
            kind: Kind::Text,
            data: Bytes::from_static(b"brisk live check"),
        };
        assert!(c.set(clip.clone()));
        std::thread::sleep(Duration::from_millis(800));
        let out = paste(&["--no-newline", "--type", "text/plain;charset=utf-8"]);
        assert_eq!(out.as_deref(), Some(&b"brisk live check"[..]));
        assert_eq!(b.read(), Some(clip));
        assert_eq!(c.since(Some(s0)), (s0, None), "own write echoed");
        // A change made by someone else bumps the seq.
        let st = cmd("wl-copy", &["--clear"]).status().unwrap();
        assert!(st.success());
        std::thread::sleep(Duration::from_millis(800));
        assert!(c.since(None).0 > s0);
        assert_eq!(b.read(), None);
    }
}
