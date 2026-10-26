//! systemd readiness and watchdog (`Type=notify`, `WatchdogSec=`). Everything here is a no-op
//! unless systemd set NOTIFY_SOCKET, so manual runs are unaffected.

use crate::http::AppState;
use sd_notify::NotifyState;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Removes systemd's notify/watchdog variables from a child's environment: the bridge, Steam and
/// helpers must never talk to (or be mistaken for) hostd's service manager. Plain `env_remove`,
/// not sd-notify's `*_and_unset_env` (an unsafe `remove_var` in a threaded process).
pub fn scrub(c: &mut std::process::Command) -> &mut std::process::Command {
    for k in ["NOTIFY_SOCKET", "WATCHDOG_USEC", "WATCHDOG_PID"] {
        c.env_remove(k);
    }
    c
}

/// Started by systemd with a notify socket.
pub fn enabled() -> bool {
    std::env::var_os("NOTIFY_SOCKET").is_some()
}

fn send(states: &[NotifyState]) {
    if enabled()
        && let Err(e) = sd_notify::notify(states)
    {
        eprintln!("warning: sd_notify: {e}");
    }
}

/// A quarter of WATCHDOG_USEC (systemd suggests half; a quarter tolerates three missed pings);
/// `None` without NOTIFY_SOCKET or a watchdog for this pid.
pub fn watchdog_every() -> Option<Duration> {
    enabled()
        .then(sd_notify::watchdog_enabled)
        .flatten()
        .map(|d| d / 4)
}

pub fn status_text(
    listen: SocketAddr,
    site: &crate::site::Site,
    stream: &crate::stream::StreamInfo,
) -> String {
    format!(
        "serving {listen}, static={}, stream {}",
        site.kind(),
        format!("{:?}", stream.state).to_lowercase()
    )
}

pub fn ready(status: &str) {
    send(&[NotifyState::Ready, NotifyState::Status(status)]);
}

pub fn stopping() {
    send(&[NotifyState::Stopping, NotifyState::Status("stopping")]);
}

/// True once `addr` answers an HTTP request (any status: 403 is fine), i.e. the router is
/// really serving, not just bound. Tries for about 10 s.
pub async fn serving(addr: SocketAddr) -> bool {
    for _ in 0..100 {
        let probe = async {
            let mut c = tokio::net::TcpStream::connect(addr).await.ok()?;
            c.write_all(b"GET /api/host HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .await
                .ok()?;
            let mut b = [0u8; 8];
            c.read_exact(&mut b).await.ok()?;
            b.starts_with(b"HTTP/1.").then_some(())
        };
        if tokio::time::timeout(Duration::from_secs(1), probe)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// hostd's liveness, checked before each ping: the runtime still runs a freshly spawned task,
/// and the library and stream state locks can be taken within a second. Returns the status text.
async fn alive(state: &AppState) -> Option<String> {
    tokio::time::timeout(Duration::from_secs(1), tokio::spawn(async {}))
        .await
        .ok()?
        .ok()?;
    for _ in 0..20 {
        if let Some(s) = locks_free(state) {
            return Some(s);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

fn locks_free(state: &AppState) -> Option<String> {
    let _games = state.games.try_lock().ok()?;
    Some(status_text(
        state.cfg.listen,
        &state.site,
        &state.stream.try_info()?,
    ))
}

/// Pings the watchdog every `every`, with the current status, but only while [`alive`] passes:
/// a wedged runtime or a stuck state lock stops the pings and systemd restarts hostd.
pub async fn watchdog(state: std::sync::Arc<AppState>, every: Duration) {
    let mut t = tokio::time::interval(every);
    t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        t.tick().await;
        match alive(&state).await {
            Some(s) => send(&[NotifyState::Watchdog, NotifyState::Status(&s)]),
            None => eprintln!("warning: hostd not responsive; watchdog ping skipped"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixDatagram;
    use std::sync::Arc;

    /// The notify calls read the real environment, so they run in a child copy of this test
    /// binary that gets NOTIFY_SOCKET (no set_var in a threaded test process).
    fn run_child(env: &[(&str, &str)]) -> std::process::ExitStatus {
        run_named("notify::tests::child", env)
    }

    fn run_named(test: &str, env: &[(&str, &str)]) -> std::process::ExitStatus {
        let mut c = std::process::Command::new(std::env::current_exe().unwrap());
        c.args([test, "--exact", "--ignored", "--nocapture"])
            .env_remove("NOTIFY_SOCKET")
            .env_remove("WATCHDOG_USEC")
            .env_remove("WATCHDOG_PID")
            .env("BRISK_NOTIFY_CHILD", "1");
        for (k, v) in env {
            c.env(k, v);
        }
        c.status().unwrap()
    }

    fn test_state(d: &std::path::Path) -> AppState {
        let mut cfg = crate::config::Config::from_env(d.join("dist"));
        cfg.data_dir = d.join("data");
        cfg.downloads_dir = d.join("dl");
        AppState::new(cfg)
    }

    fn recv_all(s: &UnixDatagram) -> Vec<String> {
        s.set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let mut out = vec![];
        let mut b = [0u8; 4096];
        while let Ok(n) = s.recv(&mut b) {
            out.push(String::from_utf8_lossy(&b[..n]).into_owned());
        }
        out
    }

    /// The child side: what main does, against a test state.
    #[test]
    #[ignore = "run by sends_ready_watchdog_stopping_in_order"]
    fn child() {
        if std::env::var_os("BRISK_NOTIFY_CHILD").is_none() {
            return;
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let d = tempfile::tempdir().unwrap();
            let st = Arc::new(test_state(d.path()));
            let every = watchdog_every();
            if !enabled() {
                assert_eq!(every, None, "watchdog without NOTIFY_SOCKET");
            }
            ready(&status_text(st.cfg.listen, &st.site, &st.stream.info()));
            if let Some(every) = every {
                assert_eq!(every, Duration::from_millis(100));
                let wd = tokio::spawn(watchdog(st.clone(), every));
                tokio::time::sleep(Duration::from_millis(350)).await;
                wd.abort();
            }
            stopping();
        });
    }

    #[test]
    fn sends_ready_watchdog_stopping_in_order() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notify.sock");
        let s = UnixDatagram::bind(&p).unwrap();
        let st = run_child(&[
            ("NOTIFY_SOCKET", p.to_str().unwrap()),
            ("WATCHDOG_USEC", "400000"),
        ]);
        assert!(st.success());
        let got = recv_all(&s);
        let status = "STATUS=serving 127.0.0.1:8443, static=disk, stream idle\n";
        assert_eq!(
            got.first().map(String::as_str),
            Some(&*format!("READY=1\n{status}"))
        );
        assert_eq!(
            got.last().map(String::as_str),
            Some("STOPPING=1\nSTATUS=stopping\n")
        );
        let pings = &got[1..got.len() - 1];
        // 400 ms / 4 = a tick every 100 ms: 0, 100, 200, 300 in a 350 ms window; fewer under load.
        assert!((1..=4).contains(&pings.len()), "{got:?}");
        assert!(
            pings.iter().all(|m| *m == format!("WATCHDOG=1\n{status}")),
            "{got:?}"
        );
    }

    /// Child side of `children_never_see_the_notify_variables`: hostd's real runner, with the
    /// variables set in this process (and once more passed explicitly to `spawn`).
    #[test]
    #[ignore = "run by children_never_see_the_notify_variables"]
    fn env_child() {
        use crate::host::{RealRunner, Runner};
        let Some(out) = std::env::var_os("BRISK_ENV_OUT") else {
            return;
        };
        let seen = RealRunner.run("env", &[]).unwrap();
        std::fs::write(std::path::Path::new(&out).join("run"), seen).unwrap();
        let script = format!("env > '{}/spawn'", out.to_str().unwrap());
        let extra = [("NOTIFY_SOCKET".to_string(), "/x".to_string())];
        assert!(RealRunner.spawn("sh", &["-c", &script], &extra));
    }

    #[test]
    fn children_never_see_the_notify_variables() {
        let d = tempfile::tempdir().unwrap();
        let st = run_named(
            "notify::tests::env_child",
            &[
                ("BRISK_ENV_OUT", d.path().to_str().unwrap()),
                ("NOTIFY_SOCKET", "/run/user/1000/systemd/notify"),
                ("WATCHDOG_USEC", "60000000"),
                ("WATCHDOG_PID", "1"),
            ],
        );
        assert!(st.success());
        for f in ["run", "spawn"] {
            let p = d.path().join(f);
            for _ in 0..100 {
                if std::fs::read_to_string(&p).is_ok_and(|s| s.contains("PATH=")) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let env = std::fs::read_to_string(&p).unwrap();
            assert!(
                env.contains("BRISK_ENV_OUT="),
                "{f}: child env not captured"
            );
            for k in ["NOTIFY_SOCKET=", "WATCHDOG_USEC=", "WATCHDOG_PID="] {
                assert!(!env.lines().any(|l| l.starts_with(k)), "{f} saw {k}");
            }
        }
    }

    #[test]
    fn sends_nothing_without_notify_socket() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notify.sock");
        let s = UnixDatagram::bind(&p).unwrap();
        // A watchdog value alone (no socket) must not start anything.
        assert!(run_child(&[("WATCHDOG_USEC", "400000")]).success());
        assert!(recv_all(&s).is_empty());
    }

    #[test]
    fn watchdog_is_only_for_our_pid() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notify.sock");
        let s = UnixDatagram::bind(&p).unwrap();
        let st = run_child(&[
            ("NOTIFY_SOCKET", p.to_str().unwrap()),
            ("WATCHDOG_USEC", "400000"),
            ("WATCHDOG_PID", "1"),
        ]);
        assert!(st.success());
        let got = recv_all(&s);
        assert_eq!(got.len(), 2, "READY and STOPPING only: {got:?}");
    }

    #[tokio::test]
    async fn serving_needs_an_http_answer() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut c, _) = l.accept().await.unwrap();
                let _ = c.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
            }
        });
        assert!(serving(a).await);
    }

    #[tokio::test]
    async fn liveness_fails_while_a_state_lock_is_stuck() {
        let d = tempfile::tempdir().unwrap();
        let st = test_state(d.path());
        assert_eq!(
            alive(&st).await.as_deref(),
            Some("serving 127.0.0.1:8443, static=disk, stream idle")
        );
        let held = st.games.lock().unwrap();
        assert_eq!(locks_free(&st), None);
        drop(held);
        assert!(locks_free(&st).is_some());
    }
}
