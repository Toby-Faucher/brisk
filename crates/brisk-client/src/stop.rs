//! A stop request that several tasks can wait on: SIGINT, SIGTERM or SIGHUP in the binaries, a
//! [`StopHandle`] in tests. A request is remembered, so a signal that arrives before anyone waits
//! is still seen.

use std::future::Future;
use tokio::sync::watch;

/// How many requests came in, and the name of the first one.
#[derive(Clone, Copy, Default)]
struct Requests {
    count: u32,
    first: Option<&'static str>,
}

/// Waits for stop requests. Clones see the same requests.
#[derive(Clone)]
pub struct Stop {
    rx: watch::Receiver<Requests>,
}

/// Sends stop requests to the [`Stop`]s made with it.
pub struct StopHandle {
    tx: watch::Sender<Requests>,
}

impl StopHandle {
    /// Counts one request; `name` is kept for the first one (a signal name, say).
    pub fn request(&self, name: &'static str) {
        self.tx.send_modify(|r| {
            r.count += 1;
            r.first.get_or_insert(name);
        });
    }
}

impl Stop {
    /// A stop that fires when `handle` says so.
    pub fn manual() -> (Stop, StopHandle) {
        let (tx, rx) = watch::channel(Requests::default());
        (Stop { rx }, StopHandle { tx })
    }

    /// A stop fed by SIGINT, SIGTERM and SIGHUP, one request per signal. The handlers are
    /// installed before this returns, so a signal right after it cannot kill the process before
    /// the caller's cleanup. Needs a running tokio runtime.
    pub fn on_signals() -> std::io::Result<Stop> {
        use tokio::signal::unix::{SignalKind, signal};
        let (mut term, mut hup, mut int) = (
            signal(SignalKind::terminate())?,
            signal(SignalKind::hangup())?,
            signal(SignalKind::interrupt())?,
        );
        let (stop, handle) = Stop::manual();
        tokio::spawn(async move {
            loop {
                let name = tokio::select! {
                    _ = int.recv() => "SIGINT",
                    _ = term.recv() => "SIGTERM",
                    _ = hup.recv() => "SIGHUP",
                };
                handle.request(name);
            }
        });
        Ok(stop)
    }

    /// Name of the first request, if there has been one.
    pub fn first(&self) -> Option<&'static str> {
        self.rx.borrow().first
    }

    /// Resolves with the name of the first request. Stays pending if nobody can send one any more.
    pub async fn requested(&mut self) -> &'static str {
        self.nth(1).await
    }

    /// Resolves at the second request, which means "do not wait for cleanup".
    pub async fn repeated(&mut self) -> &'static str {
        self.nth(2).await
    }

    async fn nth(&mut self, n: u32) -> &'static str {
        match self.rx.wait_for(|r| r.count >= n).await {
            Ok(r) => r.first.expect("a counted request has a name"),
            Err(_) => std::future::pending().await,
        }
    }

    /// `f`'s output, or `None` if a stop request comes first.
    pub async fn unless<T>(&mut self, f: impl Future<Output = T>) -> Option<T> {
        tokio::select! {
            v = f => Some(v),
            _ = self.requested() => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn a_request_before_the_wait_is_remembered() {
        let (mut stop, handle) = Stop::manual();
        handle.request("SIGTERM");
        handle.request("SIGINT");
        assert_eq!(stop.requested().await, "SIGTERM");
        assert_eq!(stop.repeated().await, "SIGTERM");
        assert_eq!(stop.clone().first(), Some("SIGTERM"));
    }

    #[tokio::test]
    async fn unless_gives_the_result_or_none_on_stop() {
        let (mut stop, handle) = Stop::manual();
        assert_eq!(stop.unless(async { 7 }).await, Some(7));
        handle.request("SIGHUP");
        assert_eq!(stop.unless(std::future::pending::<()>()).await, None);
    }

    /// SIGTERM and SIGHUP stop the run like Ctrl-C (and no longer kill the process outright).
    #[tokio::test]
    async fn sigterm_and_sighup_are_caught() {
        for (flag, name) in [("-TERM", "SIGTERM"), ("-HUP", "SIGHUP")] {
            let mut stop = Stop::on_signals().expect("signal handlers");
            let pid = std::process::id().to_string();
            assert!(
                std::process::Command::new("kill")
                    .args([flag, &pid])
                    .status()
                    .expect("kill runs")
                    .success()
            );
            let got = tokio::time::timeout(Duration::from_secs(5), stop.requested()).await;
            assert_eq!(got, Ok(name));
        }
    }
}
