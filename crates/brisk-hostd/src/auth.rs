//! Who may talk to hostd. Host header allowlist (DNS rebinding) and the tailnet identity check;
//! the secret itself is compared in http.rs's guard.

use crate::host::Runner;
use std::sync::{Arc, Mutex};
use std::time::Duration;
// tokio's clock so tests can pause it; it is the system clock otherwise
use tokio::time::Instant;

/// The Host headers hostd answers: loopback on 8443, `BRISK_ALLOWED_HOSTS`, and this machine's
/// tailnet name (`tailscale status --json` Self.DNSName, with :8443).
pub struct Hosts {
    fixed: Vec<String>,
    tailnet: Mutex<Vec<String>>,
    /// When `tailscale status` last ran; claimed before running, so a burst of requests with
    /// unknown names makes one call per `recheck`.
    checked: Mutex<Option<Instant>>,
    runner: Arc<dyn Runner>,
    pub recheck: Duration,
    pub timeout: Duration,
}

impl Hosts {
    /// The fixed local hosts plus `extra`; tailnet names are looked up later.
    pub fn new(extra: &[String], runner: Arc<dyn Runner>) -> Self {
        let mut fixed = vec!["localhost:8443".to_string(), "127.0.0.1:8443".to_string()];
        fixed.extend(extra.iter().map(|h| h.to_ascii_lowercase()));
        Hosts {
            fixed,
            tailnet: Mutex::new(vec![]),
            checked: Mutex::new(None),
            runner,
            recheck: Duration::from_secs(30),
            timeout: Duration::from_secs(3),
        }
    }

    /// Asks tailscale for this machine's name (blocking on a worker thread, at most `timeout`).
    pub async fn refresh(&self) {
        *self.checked.lock().unwrap() = Some(Instant::now());
        let r = self.runner.clone();
        let job = tokio::task::spawn_blocking(move || r.run("tailscale", &["status", "--json"]));
        let out = tokio::time::timeout(self.timeout, job)
            .await
            .ok()
            .and_then(Result::ok)
            .flatten();
        if let Some(names) = out.as_deref().and_then(tailnet_names) {
            *self.tailnet.lock().unwrap() = names;
        }
    }

    fn known(&self, h: &str) -> bool {
        self.fixed.iter().any(|f| f == h) || self.tailnet.lock().unwrap().iter().any(|f| f == h)
    }

    /// Exact match only. An unknown `*.ts.net` name (the tailnet came up after hostd, or was
    /// renamed) makes one fresh lookup per `recheck`; any other name is refused outright.
    pub async fn allowed(&self, host: &str) -> bool {
        let h = host.to_ascii_lowercase();
        if self.known(&h) {
            return true;
        }
        let name = h.rsplit_once(':').map_or(h.as_str(), |(n, _)| n);
        if !name.ends_with(".ts.net") {
            return false;
        }
        let due = {
            let mut c = self.checked.lock().unwrap();
            let due = c.is_none_or(|t| t.elapsed() >= self.recheck);
            if due {
                *c = Some(Instant::now());
            }
            due
        };
        if due {
            self.refresh().await;
        }
        self.known(&h)
    }
}

/// `Self.DNSName` without its trailing dot, as `name:8443` (the only port tailscale serve maps to us).
fn tailnet_names(json: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let n = v["Self"]["DNSName"].as_str()?.trim_end_matches('.');
    (!n.is_empty()).then(|| vec![format!("{}:8443", n.to_ascii_lowercase())])
}

/// A request that came through `tailscale serve` from an allowed login. tailscaled sets
/// `Tailscale-User-Login` and connects from loopback; it is only trusted from a loopback peer.
/// `csrf` is the page's `x-brisk-csrf` header: tailscale attaches the identity to any request from
/// the laptop, a cross-site one too, and a custom header cannot be sent cross-origin without a
/// CORS preflight, which hostd never grants.
pub(crate) fn tailnet_ok(users: &[String], loopback: bool, login: Option<&str>, csrf: bool) -> bool {
    let Some(login) = login.map(str::trim).filter(|l| !l.is_empty()) else {
        return false;
    };
    loopback && csrf && users.iter().any(|u| u.eq_ignore_ascii_case(login))
}
