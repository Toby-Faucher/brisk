//! The hub: machines.json, GET /api/machines, the power route and the /api/m/{id}/* proxy
//! (docs/plans/machine-hub.md). Secrets are read from their files at each use, never stored and
//! never in a response, log line or error text.

use crate::http::{AppState, err};
use crate::idle::{IdleWatch, Observed};
use crate::power::{Power, PowerCfg, ProxmoxCfg, VmState, VmStats};
use axum::body::Body;
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CAP: usize = 16 << 20;
/// An upload chunk is at most 32 MiB; the rest is slack.
const UPLOAD_CAP: usize = 33 << 20;
const PROBE_TTL: Duration = Duration::from_secs(3);
const STOPPING_FOR: Duration = Duration::from_secs(120);
/// Proxmox takes a few seconds to report a started VM as running; until then the machine is `starting`, not `off`.
const STARTING_FOR: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineCfg {
    pub id: String,
    pub name: String,
    pub url: String,
    pub secret_file: PathBuf,
    #[serde(default)]
    pub power: Option<PowerCfg>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    machines: Vec<MachineCfg>,
}

fn url_ok(u: &str) -> Result<(), &'static str> {
    if u.ends_with('/') {
        return Err("url must not end with a slash");
    }
    let loopback = |r: &str| matches!(r.split(':').next(), Some("127.0.0.1" | "localhost"));
    let rest = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://").filter(|r| loopback(r)));
    // Authority only: no path, query, userinfo or whitespace to smuggle into the forwarded URL.
    match rest {
        Some(r)
            if !r.is_empty()
                && r.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b)) =>
        {
            Ok(())
        }
        _ => Err("url must be https://host[:port] (http only for loopback)"),
    }
}

/// Hub-to-machine ping: milliseconds (at least 1) for a plain TCP connect to the url's host:port, None on failure.
fn tcp_rtt_ms(url: &str) -> Option<u64> {
    use std::net::{TcpStream, ToSocketAddrs};
    let uri: ureq::http::Uri = url.parse().ok()?;
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("https") {
            443
        } else {
            80
        });
    let host = uri.host()?.trim_matches(['[', ']']);
    let addr = (host, port).to_socket_addrs().ok()?.next()?; // resolved before the clock starts
    let t = Instant::now();
    TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    Some((t.elapsed().as_secs_f64() * 1000.0).round().max(1.0) as u64)
}

/// `power.temp_url`: https, or http only to loopback, a private IPv4 or a tailnet IPv4; no userinfo.
fn temp_url_ok(u: &str) -> Result<(), &'static str> {
    use std::net::Ipv4Addr;
    const MSG: &str =
        "temp_url must be https (http only for loopback, private or tailnet addresses)";
    let uri: ureq::http::Uri = u.parse().map_err(|_| MSG)?;
    let (Some(auth), Some(host)) = (uri.authority(), uri.host()) else {
        return Err(MSG);
    };
    if auth.as_str().contains('@') {
        return Err("temp_url must not contain userinfo");
    }
    match uri.scheme_str() {
        Some("https") => Ok(()),
        Some("http") => {
            let ok = match host.parse::<Ipv4Addr>() {
                Ok(ip) => {
                    let o = ip.octets();
                    ip.is_loopback() || ip.is_private() || (o[0] == 100 && o[1] & 0xC0 == 0x40)
                }
                Err(_) => matches!(host, "localhost" | "[::1]"),
            };
            if ok { Ok(()) } else { Err(MSG) }
        }
        _ => Err(MSG),
    }
}

/// machines.json text to validated machines; errors name the machine, never a secret.
pub fn parse(text: &str) -> Result<Vec<MachineCfg>, String> {
    let f: File = serde_json::from_str(text).map_err(|e| format!("not valid: {e}"))?;
    if f.machines.is_empty() {
        return Err("no machines".into());
    }
    let mut seen = std::collections::HashSet::new();
    for m in &f.machines {
        let id = &m.id;
        if !crate::fsutil::slug_ok(id, 32) {
            return Err(format!("machine id {id:?} must be 1-32 of a-z, 0-9, -"));
        }
        if !seen.insert(id.as_str()) {
            return Err(format!("machine id {id:?} is used twice"));
        }
        url_ok(&m.url).map_err(|e| format!("machine {id}: {e}"))?;
        if let Some(t) = m.power.as_ref().and_then(|p| p.temp_url.as_deref()) {
            temp_url_ok(t).map_err(|e| format!("machine {id}: {e}"))?;
        }
        if m.power.as_ref().and_then(|p| p.idle_shutdown_min) == Some(0) {
            return Err(format!(
                "machine {id}: idle_shutdown_min must be at least 1"
            ));
        }
    }
    Ok(f.machines)
}

pub struct Entry {
    pub cfg: MachineCfg,
    power: Option<Arc<dyn Power>>,
}

/// The machines behind this hub and their state.
pub struct Hub {
    machines: Vec<Arc<Entry>>,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
    /// When a shutdown was last requested, for the `stopping` state.
    stopping: Mutex<HashMap<String, Instant>>,
    /// When a start was last requested, for the `starting` state while Proxmox still says stopped.
    starting: Mutex<HashMap<String, Instant>>,
}

/// `Ok(None)`: no file at the default path (single-host mode). A file that is there but bad, or
/// an explicitly named one that is missing, is an error: never a silent fallback.
pub fn load(path: &Path, explicit: bool) -> Result<Option<Hub>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !explicit => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let cfgs = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(Hub::new(cfgs, |_, p| crate::power::proxmox(p))))
}

impl Hub {
    /// `mk` builds the Power for a machine with a power block (tests pass a fake).
    pub fn new(cfgs: Vec<MachineCfg>, mk: impl Fn(&str, &ProxmoxCfg) -> Arc<dyn Power>) -> Hub {
        let machines = cfgs
            .into_iter()
            .map(|cfg| {
                let power = cfg.power.as_ref().map(|p| mk(&cfg.id, &p.proxmox));
                Arc::new(Entry { cfg, power })
            })
            .collect();
        Hub {
            machines,
            cache: Mutex::default(),
            stopping: Mutex::default(),
            starting: Mutex::default(),
        }
    }

    fn get(&self, id: &str) -> Option<Arc<Entry>> {
        self.machines.iter().find(|e| e.cfg.id == id).cloned()
    }

    fn invalidate(&self, id: &str) {
        self.cache.lock().unwrap().remove(id);
    }

    fn is_stopping(&self, id: &str) -> bool {
        Self::recent(&self.stopping, id, STOPPING_FOR)
    }

    fn is_starting(&self, id: &str) -> bool {
        Self::recent(&self.starting, id, STARTING_FOR)
    }

    fn recent(marks: &Mutex<HashMap<String, Instant>>, id: &str, within: Duration) -> bool {
        marks
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|t| t.elapsed() < within)
    }

    fn view(e: &Entry, state: &str, host: Option<Value>) -> Value {
        Self::view_with(e, state, host, None, None)
    }

    fn view_with(
        e: &Entry,
        state: &str,
        host: Option<Value>,
        rtt_ms: Option<u64>,
        vm: Option<VmStats>,
    ) -> Value {
        let sunshine = host.as_ref().and_then(|h| h.get("sunshine")).cloned();
        let vm = vm.map(|s| {
            json!({ "cpuPct": (s.cpu * 1000.0).round() / 10.0,
                    "memUsedBytes": s.mem_used, "memTotalBytes": s.mem_total })
        });
        json!({ "id": e.cfg.id, "name": e.cfg.name, "state": state, "url": e.cfg.url,
                "canPower": e.power.is_some(), "sunshine": sunshine, "host": host,
                "rttMs": rtt_ms, "vm": vm })
    }

    /// Blocking: one state probe (Proxmox, then the machine's /api/host).
    fn probe(&self, e: &Entry) -> Value {
        // The machine's /api/host plus a TCP-connect ping; the temperature fills a missing cpuTempC.
        let host = || {
            let rtt = tcp_rtt_ms(&e.cfg.url);
            let mut h = get_json(e, "host");
            if let Some(Value::Object(o)) = &mut h
                && o.get("cpuTempC").is_none_or(Value::is_null)
                && let Some(c) = e
                    .cfg
                    .power
                    .as_ref()
                    .and_then(|p| fetch_temp(p.temp_url.as_deref()?))
            {
                o.insert("cpuTempC".into(), json!(c));
            }
            (h, rtt)
        };
        match &e.power {
            None => match host() {
                (Some(h), rtt) => Self::view_with(e, "ready", Some(h), rtt, None),
                (None, _) => Self::view(e, "unreachable", None),
            },
            Some(p) => match p.status() {
                Err(_) => Self::view(e, "unreachable", None),
                Ok(VmState::Stopped) => {
                    self.stopping.lock().unwrap().remove(&e.cfg.id);
                    if self.is_starting(&e.cfg.id) {
                        Self::view(e, "starting", None)
                    } else {
                        Self::view(e, "off", None)
                    }
                }
                Ok(VmState::Running) => {
                    let (h, rtt) = host();
                    let ready = h.as_ref().and_then(|h| h["sunshine"].as_str()) == Some("ready");
                    let state = if self.is_stopping(&e.cfg.id) {
                        "stopping"
                    } else if ready {
                        self.starting.lock().unwrap().remove(&e.cfg.id);
                        "ready"
                    } else {
                        "starting"
                    };
                    Self::view_with(e, state, h, rtt, p.stats().ok())
                }
            },
        }
    }

    fn cached(&self, e: &Entry) -> Value {
        if let Some((t, v)) = self.cache.lock().unwrap().get(&e.cfg.id)
            && t.elapsed() < PROBE_TTL
        {
            return v.clone();
        }
        let v = self.probe(e);
        self.cache
            .lock()
            .unwrap()
            .insert(e.cfg.id.clone(), (Instant::now(), v.clone()));
        v
    }
}

/// Why a call to a machine produced no answer.
enum Fail {
    /// Secret file unreadable or not usable as a header: our side, say so.
    Local,
    Down,
}

struct Up {
    status: u16,
    ctype: Option<String>,
    seq: Option<String>,
    retry_after: Option<String>,
    body: Vec<u8>,
}

/// Blocking call to `{url}/api/{pq}`. TLS from `net::agent`: no proxy, no redirects, so
/// the machine's secret only ever goes to the configured URL.
fn call(
    e: &Entry,
    method: &str,
    pq: &str,
    heads: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> Result<Up, Fail> {
    let agent = crate::net::agent(
        ureq::tls::RootCerts::PlatformVerifier,
        Some(Duration::from_secs(3)),
        timeout,
        false,
    );
    let secret = std::fs::read_to_string(&e.cfg.secret_file).map_err(|_| Fail::Local)?;
    let mut rq = ureq::http::Request::builder()
        .method(method)
        .uri(format!("{}/api/{pq}", e.cfg.url))
        .header("x-brisk-secret", secret.trim())
        .header("x-brisk-csrf", "1");
    for (k, v) in heads {
        rq = rq.header(*k, *v);
    }
    let rq = rq.body(body).map_err(|_| Fail::Local)?;
    let mut r = agent.run(rq).map_err(|_| Fail::Down)?;
    let h = |n: &str| {
        r.headers()
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    };
    let (ctype, seq, retry_after, status) = (
        h("content-type"),
        h("x-brisk-seq"),
        h("retry-after"),
        r.status().as_u16(),
    );
    let body = r
        .body_mut()
        .with_config()
        .limit(CAP as u64)
        .read_to_vec()
        .map_err(|_| Fail::Down)?;
    Ok(Up {
        status,
        ctype,
        seq,
        retry_after,
        body,
    })
}

/// A 200 JSON answer from the machine, 3 s at most.
fn get_json(e: &Entry, path: &str) -> Option<Value> {
    let up = call(e, "GET", path, &[], &[], Duration::from_secs(3)).ok()?;
    if up.status != 200 {
        return None;
    }
    serde_json::from_slice(&up.body).ok()
}

/// `{"cpuTempC": n}` from the configured sensor URL, 2 s at most; any failure is `None`.
fn fetch_temp(url: &str) -> Option<f64> {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let agent = AGENT.get_or_init(|| {
        crate::net::agent(
            ureq::tls::RootCerts::PlatformVerifier,
            None,
            Duration::from_secs(2),
            true,
        )
    });
    let mut r = agent.get(url).call().ok()?;
    let body = r
        .body_mut()
        .with_config()
        .limit(1 << 16)
        .read_to_vec()
        .ok()?;
    let v: Value = serde_json::from_slice(&body).ok()?;
    v["cpuTempC"].as_f64().filter(|c| (0.0..=150.0).contains(c))
}

/// `GET /api/machines`: every machine with its state; 404 without a hub.
pub async fn list(State(s): State<Arc<AppState>>) -> Response {
    let Some(h) = s.hub.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // All machines at once: one slow or dead machine must not hold up the others.
    let jobs: Vec<_> = h
        .machines
        .iter()
        .map(|e| {
            let (h, e) = (h.clone(), e.clone());
            tokio::task::spawn_blocking(move || h.cached(&e))
        })
        .collect();
    let mut out = vec![];
    for (j, e) in jobs.into_iter().zip(&h.machines) {
        out.push(
            j.await
                .unwrap_or_else(|_| Hub::view(e, "unreachable", None)),
        );
    }
    axum::Json(json!({ "machines": out })).into_response()
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Verb {
    Start,
    Shutdown,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PowerBody {
    action: Verb,
}

/// Blocking: (status, state) for the power route.
fn do_power(h: &Hub, e: &Entry, p: &dyn Power, v: Verb) -> Result<(StatusCode, String), String> {
    let id = &e.cfg.id;
    let r = match (v, p.status()?) {
        (Verb::Start, VmState::Running) => {
            h.invalidate(id);
            let state = h.probe(e)["state"]
                .as_str()
                .unwrap_or("starting")
                .to_string();
            (StatusCode::OK, state)
        }
        (Verb::Start, VmState::Stopped) => {
            p.start()?;
            h.stopping.lock().unwrap().remove(id);
            h.starting
                .lock()
                .unwrap()
                .insert(id.clone(), Instant::now());
            (StatusCode::ACCEPTED, "starting".into())
        }
        (Verb::Shutdown, VmState::Stopped) => (StatusCode::OK, "off".into()),
        (Verb::Shutdown, VmState::Running) => {
            p.shutdown()?;
            h.starting.lock().unwrap().remove(id);
            h.stopping
                .lock()
                .unwrap()
                .insert(id.clone(), Instant::now());
            (StatusCode::ACCEPTED, "stopping".into())
        }
    };
    h.invalidate(id);
    Ok(r)
}

/// `POST /api/machines/{id}/power`: start or shut down a machine.
pub(crate) async fn power(
    State(s): State<Arc<AppState>>,
    UrlPath(id): UrlPath<String>,
    b: axum::body::Bytes,
) -> Response {
    let Some(h) = s.hub.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(e) = h.get(&id) else {
        return err(StatusCode::NOT_FOUND, "unknown machine");
    };
    let Ok(PowerBody { action }) = serde_json::from_slice(&b) else {
        return err(
            StatusCode::BAD_REQUEST,
            "bad request: action is start or shutdown",
        );
    };
    let Some(p) = e.power.clone() else {
        return err(StatusCode::CONFLICT, "this machine has no power control");
    };
    match tokio::task::spawn_blocking(move || do_power(&h, &e, &*p, action)).await {
        Ok(Ok((code, state))) => (code, axum::Json(json!({ "state": state }))).into_response(),
        Ok(Err(m)) => err(StatusCode::BAD_GATEWAY, &m),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "power task failed"),
    }
}

/// Every segment, once percent-decoded, is a plain name: no `.`/`..`, separators or control
/// characters. The raw (still encoded) path is what gets forwarded.
fn path_ok(p: &str) -> bool {
    p.split('/').all(|seg| {
        percent_encoding::percent_decode_str(seg)
            .decode_utf8()
            .is_ok_and(|d| {
                !matches!(&*d, "" | "." | "..")
                    && !d.contains(['/', '\\'])
                    && !d.chars().any(char::is_control)
            })
    })
}

/// ANY /api/m/{id}/{*path}: forwarded to the machine's own `/api/{path}`.
pub(crate) async fn proxy(State(s): State<Arc<AppState>>, req: Request) -> Response {
    let Some(h) = s.hub.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (parts, body) = req.into_parts();
    // The api router is nested, so the URI is already relative to /api.
    let uri = &parts.uri;
    let Some((id, path)) = uri
        .path()
        .strip_prefix("/m/")
        .and_then(|r| r.split_once('/'))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(e) = h.get(id) else {
        return err(StatusCode::NOT_FOUND, "unknown machine");
    };
    if !path_ok(path) {
        return err(StatusCode::BAD_REQUEST, "bad path");
    }
    let first = path.split('/').next().unwrap_or("");
    if percent_encoding::percent_decode_str(first).decode_utf8_lossy() == "files" {
        return err(
            StatusCode::NOT_IMPLEMENTED,
            "file drop is not available through the hub",
        );
    }
    let pq = match uri.query() {
        Some(q) => format!("{path}?{q}"),
        None => path.to_string(),
    };
    let method = parts.method.as_str().to_string();
    let hv = |n: &str| {
        parts
            .headers
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    };
    let (ct, accept, offset) = (hv("content-type"), hv("accept"), hv("x-offset"));
    let upload = first == "uploads";
    let cap = if upload { UPLOAD_CAP } else { CAP };
    let Ok(body) = axum::body::to_bytes(body, cap).await else {
        return err(
            StatusCode::PAYLOAD_TOO_LARGE,
            if upload {
                "request body is over 33 MiB"
            } else {
                "request body is over 16 MiB"
            },
        );
    };
    let timeout = Duration::from_secs(if upload { 300 } else { 40 });
    let r = tokio::task::spawn_blocking(move || {
        let mut heads = vec![];
        if let Some(c) = &ct {
            heads.push(("content-type", c.as_str()));
        }
        if let Some(a) = &accept {
            heads.push(("accept", a.as_str()));
        }
        if let Some(o) = &offset {
            heads.push(("x-offset", o.as_str()));
        }
        call(&e, &method, &pq, &heads, &body, timeout).map_err(|f| match f {
            Fail::Local => (
                StatusCode::BAD_GATEWAY,
                "hub cannot use this machine's secret file",
            ),
            // A VM that is off refuses connections like a dead one; ask Proxmox only now.
            Fail::Down
                if matches!(
                    e.power.as_ref().map(|p| p.status()),
                    Some(Ok(VmState::Stopped))
                ) =>
            {
                (StatusCode::SERVICE_UNAVAILABLE, "machine is off")
            }
            Fail::Down => (StatusCode::BAD_GATEWAY, "machine did not answer"),
        })
    })
    .await;
    match r {
        Ok(Ok(up)) => {
            let mut r = Response::new(Body::from(up.body));
            *r.status_mut() = StatusCode::from_u16(up.status).unwrap_or(StatusCode::BAD_GATEWAY);
            for (n, v) in [
                (header::CONTENT_TYPE.as_str(), up.ctype),
                ("x-brisk-seq", up.seq),
                (header::RETRY_AFTER.as_str(), up.retry_after),
            ] {
                if let Some(v) = v.and_then(|v| HeaderValue::from_str(&v).ok()) {
                    r.headers_mut().insert(n, v);
                }
            }
            r
        }
        Ok(Err((code, m))) => err(code, m),
        Err(_) => err(StatusCode::INTERNAL_SERVER_ERROR, "proxy task failed"),
    }
}

/// Blocking: what the idle clock needs, or None if the machine is not `ready` or an answer is
/// missing (which resets the clock).
fn observe(h: &Hub, e: &Entry) -> Option<Observed> {
    if h.probe(e)["state"] != "ready" {
        return None;
    }
    let streaming = get_json(e, "stream")?["state"].as_str()? != "idle";
    let games = get_json(e, "games")?;
    let games = games["games"].as_array()?;
    Some(Observed {
        streaming,
        game_running: games.iter().any(|g| g["running"] == true),
        updating: games.iter().any(|g| g["update"] == "running"),
    })
}

/// One task per machine with `idle_shutdown_min`: look every 30 s, ACPI shutdown when idle long enough.
pub fn spawn_idle(h: &Arc<Hub>) {
    for e in &h.machines {
        let Some(min) = e.cfg.power.as_ref().and_then(|p| p.idle_shutdown_min) else {
            continue;
        };
        let (h, e) = (h.clone(), e.clone());
        tokio::spawn(async move {
            let mut watch = IdleWatch::new(Duration::from_secs(min * 60));
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                let (h2, e2) = (h.clone(), e.clone());
                let seen = tokio::task::spawn_blocking(move || observe(&h2, &e2))
                    .await
                    .ok()
                    .flatten();
                if !watch.observe(Instant::now(), seen) {
                    continue;
                }
                let (h2, e2) = (h.clone(), e.clone());
                let done = tokio::task::spawn_blocking(move || {
                    let p = e2.power.as_ref().ok_or("no power")?;
                    p.shutdown()?;
                    h2.starting.lock().unwrap().remove(&e2.cfg.id);
                    h2.stopping
                        .lock()
                        .unwrap()
                        .insert(e2.cfg.id.clone(), Instant::now());
                    h2.invalidate(&e2.cfg.id);
                    Ok::<(), String>(())
                })
                .await;
                match done {
                    Ok(Ok(())) => eprintln!("info: idle: shutting down {}", e.cfg.id),
                    _ => eprintln!("warning: idle: shutdown of {} failed", e.cfg.id),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::http::router;
    use axum::http::Request as Rq;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    const HUB_SECRET: &str = "hub-secret-0123456789";
    const MACHINE_SECRET: &str = "machine-secret-abcdef";
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\xff\xfe";

    /// What the fake machine reports.
    #[derive(Clone)]
    struct World {
        sunshine: &'static str,
        stream: &'static str,
        running: bool,
        update: Option<&'static str>,
        temp: Option<f64>,
    }

    /// A fake hostd on an ephemeral port; returns its URL and the world it reports.
    async fn upstream() -> (String, Arc<Mutex<World>>) {
        use axum::routing::{any, get};
        let w = Arc::new(Mutex::new(World {
            sunshine: "ready",
            stream: "idle",
            running: false,
            update: None,
            temp: None,
        }));
        let (w1, w2, w3) = (w.clone(), w.clone(), w.clone());
        let app = axum::Router::new()
            .route(
                "/api/host",
                get(move || async move {
                    let w = w1.lock().unwrap();
                    json!({ "name": "up", "sunshine": w.sunshine, "cpuTempC": w.temp }).to_string()
                }),
            )
            .route(
                "/api/stream",
                get(move || async move { json!({ "state": w2.lock().unwrap().stream }).to_string() }),
            )
            .route(
                "/api/games",
                get(move || async move {
                    let w = w3.lock().unwrap();
                    json!({ "games": [{ "appid": 1, "running": w.running, "update": w.update }] })
                        .to_string()
                }),
            )
            .route(
                "/api/png",
                get(|| async {
                    ([("content-type", "image/png"), ("x-brisk-seq", "7")], PNG.to_vec())
                }),
            )
            .route(
                "/api/boom",
                any(|| async {
                    (StatusCode::CONFLICT, [("content-type", "application/json"), ("retry-after", "7")], r#"{"error":"nope"}"#)
                }),
            )
            .route(
                "/api/emulators/dolphin/controls/{kind}",
                any(|req: Rq<Body>| async move {
                    axum::Json(json!({ "method": req.method().to_string(), "pq": req.uri().to_string() }))
                }),
            )
            .route("/api/games/{id}/screenshots", any(|req: Rq<Body>| async move {
                    let (m, uri) = (req.method().to_string(), req.uri().to_string());
 let _ = axum::body::to_bytes(req.into_body(), 1 << 24).await;
 axum::Json(json!({ "method": m, "pq": uri }))
                }))
            .route("/api/games/{id}/screenshots/{name}", any(|req: Rq<Body>| async move {
                    let (m, uri) = (req.method().to_string(), req.uri().to_string());
 let _ = axum::body::to_bytes(req.into_body(), 1 << 24).await;
 axum::Json(json!({ "method": m, "pq": uri }))
                }))
            .route(
                "/api/uploads/{id}",
                any(|axum::extract::Path(id): axum::extract::Path<String>, req: Rq<Body>| async move {
                    let off = req.headers().get("x-offset").and_then(|v| v.to_str().ok()).map(String::from);
                    let n = axum::body::to_bytes(req.into_body(), 1 << 26).await.unwrap().len();
                    let code = match id.as_str() {
                        "full" => StatusCode::INSUFFICIENT_STORAGE,
                        "busy" => StatusCode::CONFLICT,
                        _ => StatusCode::OK,
                    };
                    (code, axum::Json(json!({ "body": n, "offset": off })))
                }),
            )
            .route(
                "/api/echo",
                any(|req: Rq<Body>| async move {
                    let h = req.headers().clone();
                    let (method, pq) = (req.method().to_string(), req.uri().to_string());
                    let body = axum::body::to_bytes(req.into_body(), 1 << 24).await.unwrap();
                    let hs = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).map(String::from);
                    axum::Json(json!({
                        "method": method, "pq": pq, "body": body.len(),
                        "secretOk": hs("x-brisk-secret").as_deref() == Some(MACHINE_SECRET),
                        "csrf": hs("x-brisk-csrf"), "ct": hs("content-type"),
                        "hubSecret": hs("x-brisk-secret").as_deref() == Some(HUB_SECRET),
                    }))
                }),
            );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        (url, w)
    }

    /// A URL nothing listens on.
    #[tokio::test]
    async fn tcp_rtt_is_a_connect_time() {
        let (url, _w) = upstream().await;
        let dead = dead_url().await;
        let (a, b, c) = tokio::task::spawn_blocking(move || {
            (tcp_rtt_ms(&url), tcp_rtt_ms(&dead), tcp_rtt_ms("not a url"))
        })
        .await
        .unwrap();
        assert!(a.is_some_and(|n| n >= 1) && b.is_none() && c.is_none());
    }

    async fn dead_url() -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        format!("http://{}", l.local_addr().unwrap())
    }

    struct Fake {
        vm: Mutex<Result<VmState, String>>,
        status_calls: AtomicUsize,
        stats: Option<VmStats>,
        fail_action: Option<&'static str>,
        acts: Mutex<Vec<&'static str>>,
    }
    impl Fake {
        fn new(vm: VmState) -> Arc<Fake> {
            Arc::new(Fake {
                vm: Mutex::new(Ok(vm)),
                status_calls: AtomicUsize::new(0),
                stats: Some(VmStats {
                    cpu: 0.256,
                    mem_used: 2048,
                    mem_total: 8192,
                }),
                fail_action: None,
                acts: Mutex::default(),
            })
        }
    }
    impl Power for Fake {
        fn status(&self) -> Result<VmState, String> {
            self.status_calls.fetch_add(1, Ordering::SeqCst);
            self.vm.lock().unwrap().clone()
        }
        fn stats(&self) -> Result<VmStats, String> {
            self.stats.ok_or_else(|| "no stats".into())
        }
        fn start(&self) -> Result<(), String> {
            self.act("start")
        }
        fn shutdown(&self) -> Result<(), String> {
            self.act("shutdown")
        }
    }
    impl Fake {
        fn act(&self, a: &'static str) -> Result<(), String> {
            if self.fail_action == Some(a) {
                return Err("proxmox said no".into());
            }
            self.acts.lock().unwrap().push(a);
            // A shutdown takes a while: the VM stays Running.
            if a == "start" {
                *self.vm.lock().unwrap() = Ok(VmState::Running);
            }
            Ok(())
        }
    }

    struct Rig {
        _d: tempfile::TempDir,
        app: axum::Router,
        hub: Arc<Hub>,
    }

    /// `machines`: (id, url, fake power). The hub's secret files hold MACHINE_SECRET.
    fn rig(machines: Vec<(&str, String, Option<Arc<Fake>>)>) -> Rig {
        rig_t(machines, None)
    }

    /// Like `rig`, with `power.temp_url` set on every machine that has power.
    fn rig_t(machines: Vec<(&str, String, Option<Arc<Fake>>)>, temp: Option<String>) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let sf = d.path().join("secret");
        std::fs::write(&sf, format!("{MACHINE_SECRET}\n")).unwrap();
        let mut powers: HashMap<String, Arc<Fake>> = HashMap::new();
        let ms: Vec<Value> = machines
            .iter()
            .map(|(id, url, p)| {
                let mut m = json!({ "id": id, "name": format!("{id}-name"), "url": url,
                                    "secret_file": sf });
                if let Some(p) = p {
                    powers.insert(id.to_string(), p.clone());
                    m["power"] = json!({ "proxmox": { "api": "https://pve.example.invalid:8006",
                        "node": "n", "vmid": 1, "token_id": "u@pve!t", "token_file": "/nonexistent" },
                        "temp_url": temp });
                }
                m
            })
            .collect();
        let cfgs = parse(&json!({ "machines": ms }).to_string()).unwrap();
        let hub = Arc::new(Hub::new(cfgs, |id, _| powers[id].clone()));
        let mut cfg = Config::from_env(d.path().join("dist"));
        cfg.secret = HUB_SECRET.into();
        cfg.data_dir = d.path().join("data");
        let mut st = AppState::new(cfg);
        st.hub = Some(hub.clone());
        Rig {
            app: router(Arc::new(st)),
            hub,
            _d: d,
        }
    }

    async fn send(
        app: &axum::Router,
        method: &str,
        uri: &str,
        ct: Option<&str>,
        body: Vec<u8>,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut r = Rq::builder()
            .method(method)
            .uri(uri)
            .header("host", "localhost:8443")
            .header("x-brisk-secret", HUB_SECRET);
        if let Some(c) = ct {
            r = r.header("content-type", c);
        }
        let r = app
            .clone()
            .oneshot(r.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let (st, h) = (r.status(), r.headers().clone());
        let b = axum::body::to_bytes(r.into_body(), 1 << 25)
            .await
            .unwrap()
            .to_vec();
        // Nothing the hub answers may carry a secret.
        assert!(!String::from_utf8_lossy(&b).contains(MACHINE_SECRET));
        (st, h, b)
    }

    async fn getj(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
        let (st, _, b) = send(app, "GET", uri, None, vec![]).await;
        (st, serde_json::from_slice(&b).unwrap_or_default())
    }

    async fn post_power(app: &axum::Router, id: &str, body: &str) -> (StatusCode, Value) {
        let (st, _, b) = send(
            app,
            "POST",
            &format!("/api/machines/{id}/power"),
            None,
            body.into(),
        )
        .await;
        (st, serde_json::from_slice(&b).unwrap_or_default())
    }

    fn state_of(v: &Value, i: usize) -> &str {
        v["machines"][i]["state"].as_str().unwrap()
    }

    fn one(extra: &str) -> String {
        format!(
            r#"{{"machines":[{{"id":"a","name":"A","url":"https://a.example.invalid","secret_file":"/x"{extra}}}]}}"#
        )
    }

    #[test]
    fn config_validation() {
        let bad = [
            ("{", "not valid"),
            (r#"{"machines":[]}"#, "no machines"),
            (r#"{"machines":[],"x":1}"#, "not valid"),
            (&one(r#","bogus":1"#), "not valid"),
            (&one("").replace(r#""a""#, r#""A""#), "machine id"),
            (&one("").replace(r#""a""#, r#""""#), "machine id"),
            (&one("").replace(r#""a""#, r#""a_b""#), "machine id"),
            (
                &one("").replace(r#""a""#, &format!("\"{}\"", "a".repeat(33))),
                "machine id",
            ),
            (
                &one("").replace("https://a.example.invalid", "http://a.example.invalid"),
                "url must be",
            ),
            (
                &one("").replace("https://a.example.invalid", "https://a.example.invalid/"),
                "slash",
            ),
            (
                &one("").replace("https://a.example.invalid", "https://a.example.invalid/x"),
                "url must be",
            ),
            (
                &one("").replace("https://a.example.invalid", "https://u@a.example.invalid"),
                "url must be",
            ),
            (
                &one("").replace("https://a.example.invalid", "https://a.example.invalid?x"),
                "url must be",
            ),
            (
                &one("").replace("https://a.example.invalid", "ftp://a.example.invalid"),
                "url must be",
            ),
            (
                &one("").replace(
                    "https://a.example.invalid",
                    "http://localhost.example.invalid",
                ),
                "url must be",
            ),
            (
                r#"{"machines":[{"id":"a","name":"A","url":"https://h.invalid","secret_file":"/x"},{"id":"a","name":"B","url":"https://h.invalid","secret_file":"/x"}]}"#,
                "twice",
            ),
            (
                &one(
                    r#","power":{"proxmox":{"api":"https://p.invalid","node":"n","vmid":1,"token_id":"t","token_file":"/t"},"idle_shutdown_min":0}"#,
                ),
                "idle_shutdown_min",
            ),
        ];
        let temp = |u: &str| {
            one(&format!(
                r#","power":{{"proxmox":{{"api":"https://p.invalid","node":"n","vmid":1,"token_id":"t","token_file":"/t"}},"temp_url":"{u}"}}"#
            ))
        };
        for (u, want) in [
            ("http://h.invalid/t", "temp_url must be"),
            ("http://8.8.8.8/t", "temp_url must be"),
            ("http://100.128.0.1/t", "temp_url must be"),
            ("http://172.32.0.1/t", "temp_url must be"),
            ("ftp://10.0.0.1/t", "temp_url must be"),
            ("/t", "temp_url must be"),
            ("https://u:p@h.invalid/t", "userinfo"),
            ("http://u@10.0.0.1/t", "userinfo"),
        ] {
            let e = parse(&temp(u)).expect_err(u);
            assert!(e.contains(want) && e.starts_with("machine a:"), "{u}: {e}");
        }
        for u in [
            "https://h.invalid/t",
            "http://127.0.0.1:9/t",
            "http://localhost:9/t",
            "http://10.1.2.3/t",
            "http://172.16.0.1/t",
            "http://192.168.1.1:8080/t",
            "http://100.64.0.1/t",
            "http://100.127.255.254/t",
        ] {
            let m = parse(&temp(u)).expect(u);
            assert_eq!(m[0].power.as_ref().unwrap().temp_url.as_deref(), Some(u));
        }
        for (text, want) in bad {
            let e = parse(text).expect_err(text);
            assert!(e.contains(want), "{text}: {e}");
        }
        for url in [
            "https://h.invalid:8443",
            "http://127.0.0.1:9",
            "http://localhost:9",
        ] {
            assert_eq!(
                parse(&one("").replace("https://a.example.invalid", url))
                    .unwrap()
                    .len(),
                1,
                "{url}"
            );
        }
        let ok = one(
            r#","power":{"proxmox":{"api":"https://p.invalid","node":"n","vmid":1,"token_id":"t","token_file":"/t"},"idle_shutdown_min":15}"#,
        );
        assert_eq!(
            parse(&ok).unwrap()[0]
                .power
                .as_ref()
                .unwrap()
                .idle_shutdown_min,
            Some(15)
        );
    }

    #[test]
    fn load_missing_and_bad_files() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("m.json");
        assert!(load(&f, false).unwrap().is_none());
        assert!(load(&f, true).is_err());
        std::fs::write(&f, "{").unwrap();
        let e = load(&f, false).err().unwrap();
        assert!(e.contains("m.json") && e.contains("not valid"), "{e}");
    }

    #[test]
    fn path_rules() {
        for ok in [
            "host",
            "games/1/info",
            "sessions/5/series",
            "a%20b",
            "games/steam:620/info",
            "games/steam%3A620/info",
            "games/emu:dolphin:mario-kart-wii/local",
        ] {
            assert!(path_ok(ok), "{ok}");
        }
        for bad in [
            "",
            "..",
            "a/../b",
            "./a",
            "a//b",
            "a/",
            "%2e%2e/x",
            "%2E%2e",
            "a/%2e%2E/b",
            "a%2fb",
            "a%5cb",
            "a%00b",
            "a%0ab",
            "%ff",
        ] {
            assert!(!path_ok(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn proxy_forwards_end_to_end() {
        let (url, _) = upstream().await;
        let r = rig(vec![("a", url, None)]);
        // Secret, csrf, method, query, content-type and body all arrive; the hub's own secret does not.
        let (st, _, b) = send(
            &r.app,
            "POST",
            "/api/m/a/echo?x=1&y=a%20b",
            Some("text/plain"),
            b"hello".to_vec(),
        )
        .await;
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(st, StatusCode::OK);
        assert_eq!(v["method"], "POST");
        assert_eq!(v["pq"], "/api/echo?x=1&y=a%20b");
        assert_eq!(
            (v["body"].as_u64(), v["ct"].as_str()),
            (Some(5), Some("text/plain"))
        );
        assert_eq!(
            (
                v["secretOk"].as_bool(),
                v["csrf"].as_str(),
                v["hubSecret"].as_bool()
            ),
            (Some(true), Some("1"), Some(false))
        );
        // Binary bodies and the seq header pass through unchanged.
        let (st, h, b) = send(&r.app, "GET", "/api/m/a/png", None, vec![]).await;
        assert_eq!((st, b.as_slice()), (StatusCode::OK, PNG));
        assert_eq!(h["content-type"], "image/png");
        assert_eq!(h["x-brisk-seq"], "7");
        // Binary request bodies too (clipboard image).
        let big = vec![0xffu8; 1 << 20];
        let (_, _, b) = send(&r.app, "POST", "/api/m/a/echo", Some("image/png"), big).await;
        assert_eq!(
            serde_json::from_slice::<Value>(&b).unwrap()["body"],
            1 << 20
        );
        // Upstream errors keep status and body.
        let (st, h, b) = send(&r.app, "DELETE", "/api/m/a/boom", None, vec![]).await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert_eq!(h["content-type"], "application/json");
        assert_eq!(b, br#"{"error":"nope"}"#);
        assert_eq!(h["retry-after"], "7");
        // An upstream 404 is an upstream 404.
        assert_eq!(
            send(&r.app, "GET", "/api/m/a/nothing", None, vec![])
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }

    /// The hub has no path allowlist (only `files` is refused), so the emulator mapping routes pass.
    #[tokio::test]
    async fn proxy_passes_emulator_controls() {
        let (url, _) = upstream().await;
        let r = rig(vec![("a", url, None)]);
        for m in ["GET", "PUT"] {
            let (st, _, b) = send(
                &r.app,
                m,
                "/api/m/a/emulators/dolphin/controls/gamecube",
                None,
                vec![],
            )
            .await;
            let v: Value = serde_json::from_slice(&b).unwrap();
            assert_eq!(st, StatusCode::OK, "{m}");
            assert_eq!(v["pq"], "/api/emulators/dolphin/controls/gamecube");
        }
    }

    /// Stream screenshots go through the plain proxy (the mock upstream takes 2 MiB, hostd caps shots at 8 MiB, under the proxy 16 MiB).
    #[tokio::test]
    async fn proxy_passes_stream_screenshots() {
        let (url, _) = upstream().await;
        let r = rig(vec![("a", url, None)]);
        for (m, tail, body) in [
            ("POST", "screenshots", vec![0u8; 1 << 20]),
            ("GET", "screenshots", vec![]),
            ("DELETE", "screenshots/1730000000000.png", vec![]),
        ] {
            let (st, _, b) = send(
                &r.app,
                m,
                &format!("/api/m/a/games/emu%3Adolphin%3Ax/{tail}"),
                None,
                body,
            )
            .await;
            let v: Value = serde_json::from_slice(&b).unwrap();
            assert_eq!(st, StatusCode::OK, "{m}");
            assert_eq!(v["pq"], format!("/api/games/emu%3Adolphin%3Ax/{tail}"));
        }
    }

    #[tokio::test]
    async fn proxy_uploads_take_a_chunk_and_pass_answers_through() {
        let (url, _) = upstream().await;
        let r = rig(vec![("a", url, None)]);
        let put = |id: &str, n: usize, off: &str| {
            let (app, uri, off) = (
                r.app.clone(),
                format!("/api/m/a/uploads/{id}"),
                off.to_string(),
            );
            async move {
                let q = Rq::builder()
                    .method("PUT")
                    .uri(uri)
                    .header("host", "localhost:8443")
                    .header("x-brisk-secret", HUB_SECRET)
                    .header("x-offset", off)
                    .body(Body::from(vec![7u8; n]))
                    .unwrap();
                let res = app.oneshot(q).await.unwrap();
                let st = res.status();
                let b = axum::body::to_bytes(res.into_body(), 1 << 20)
                    .await
                    .unwrap();
                (st, serde_json::from_slice::<Value>(&b).unwrap_or_default())
            }
        };
        // 20 MiB passes, with its x-offset
        let (st, v) = put("abc", 20 << 20, "12345").await;
        assert_eq!(
            (st, v["body"].as_u64(), v["offset"].as_str()),
            (StatusCode::OK, Some(20 << 20), Some("12345"))
        );
        // a full 32 MiB chunk passes; 34 MiB does not
        assert_eq!(put("abc", 32 << 20, "0").await.0, StatusCode::OK);
        assert_eq!(
            put("abc", 34 << 20, "0").await.0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        // 409 and 507 come back as they are
        assert_eq!(put("busy", 10, "0").await.0, StatusCode::CONFLICT);
        assert_eq!(
            put("full", 10, "0").await.0,
            StatusCode::INSUFFICIENT_STORAGE
        );
        // other paths keep the 16 MiB cap
        let (st, _, _) = send(&r.app, "POST", "/api/m/a/echo", None, vec![0; 20 << 20]).await;
        assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
        let (st, _, _) = send(&r.app, "PUT", "/api/m/a/uploads/abc/../x", None, vec![]).await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert_eq!(
            getj(&r.app, "/api/m/a/files/x").await.0,
            StatusCode::NOT_IMPLEMENTED
        );
    }

    #[tokio::test]
    async fn proxy_refusals() {
        let (url, _) = upstream().await;
        let r = rig(vec![("a", url, None)]);
        let (st, v) = getj(&r.app, "/api/m/a/files/x").await;
        assert_eq!(st, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(v["error"], "file drop is not available through the hub");
        assert_eq!(
            send(&r.app, "PUT", "/api/m/a/files", None, vec![]).await.0,
            StatusCode::NOT_IMPLEMENTED
        );
        assert_eq!(
            send(&r.app, "PUT", "/api/m/a/%66iles/x", None, vec![])
                .await
                .0,
            StatusCode::NOT_IMPLEMENTED
        );
        for p in [
            "../host",
            "%2e%2e/host",
            "x/%2E%2E/host",
            "x/..",
            "a%2f..%2fb",
            "x//y",
        ] {
            let (st, v) = getj(&r.app, &format!("/api/m/a/{p}")).await;
            assert_eq!(st, StatusCode::BAD_REQUEST, "{p}");
            assert_eq!(v["error"], "bad path");
        }
        let (st, v) = getj(&r.app, "/api/m/zzz/host").await;
        assert_eq!(
            (st, v["error"].as_str()),
            (StatusCode::NOT_FOUND, Some("unknown machine"))
        );
    }

    #[tokio::test]
    async fn guard_covers_hub_routes() {
        let (url, _) = upstream().await;
        let r = rig(vec![("a", url, None)]);
        for (host, secret) in [
            ("localhost:8443", None),
            ("evil.example.invalid", Some(HUB_SECRET)),
        ] {
            for uri in ["/api/machines", "/api/m/a/host"] {
                let mut q = Rq::builder().uri(uri).header("host", host);
                if let Some(s) = secret {
                    q = q.header("x-brisk-secret", s);
                }
                let res = r
                    .app
                    .clone()
                    .oneshot(q.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(res.status(), StatusCode::FORBIDDEN, "{host} {uri}");
            }
        }
        let q = Rq::builder()
            .method("POST")
            .uri("/api/machines/a/power")
            .header("host", "localhost:8443");
        let res = r
            .app
            .clone()
            .oneshot(q.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn single_host_mode_has_no_hub_routes() {
        let d = tempfile::tempdir().unwrap();
        let mut cfg = Config::from_env(d.path().join("dist"));
        cfg.secret = HUB_SECRET.into();
        cfg.data_dir = d.path().join("data");
        let app = router(Arc::new(AppState::new(cfg)));
        assert_eq!(getj(&app, "/api/machines").await.0, StatusCode::NOT_FOUND);
        assert_eq!(getj(&app, "/api/m/a/host").await.0, StatusCode::NOT_FOUND);
        assert_eq!(
            post_power(&app, "a", r#"{"action":"start"}"#).await.0,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn proxy_down_off_and_bad_secret_file() {
        let up = Fake::new(VmState::Stopped);
        let run = Fake::new(VmState::Running);
        let (a, b, c) = (dead_url().await, dead_url().await, dead_url().await);
        let r = rig(vec![
            ("plain", a, None),
            ("off", b, Some(up)),
            ("run", c, Some(run)),
        ]);
        let (st, v) = getj(&r.app, "/api/m/plain/host").await;
        assert_eq!(
            (st, v["error"].as_str()),
            (StatusCode::BAD_GATEWAY, Some("machine did not answer"))
        );
        let (st, v) = getj(&r.app, "/api/m/off/host").await;
        assert_eq!(
            (st, v["error"].as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, Some("machine is off"))
        );
        assert_eq!(
            getj(&r.app, "/api/m/run/host").await.0,
            StatusCode::BAD_GATEWAY
        );
        let sf = r.hub.get("plain").unwrap().cfg.secret_file.clone();
        std::fs::remove_file(&sf).unwrap();
        let (st, v) = getj(&r.app, "/api/m/plain/host").await;
        assert_eq!(st, StatusCode::BAD_GATEWAY);
        assert!(!v["error"].as_str().unwrap().contains(sf.to_str().unwrap()));
    }

    #[tokio::test]
    async fn machines_list_states() {
        let (url, world) = upstream().await;
        let off = Fake::new(VmState::Stopped);
        let starting = Fake::new(VmState::Running);
        let ready = Fake::new(VmState::Running);
        let gone = Fake {
            vm: Mutex::new(Err("pve down".into())),
            ..Arc::into_inner(Fake::new(VmState::Running)).unwrap()
        };
        let r = rig(vec![
            ("plain", url.clone(), None),
            ("deadplain", dead_url().await, None),
            ("off", url.clone(), Some(off)),
            ("starting", dead_url().await, Some(starting)),
            ("ready", url.clone(), Some(ready)),
            ("gone", url, Some(Arc::new(gone))),
        ]);
        let (st, v) = getj(&r.app, "/api/machines").await;
        assert_eq!(st, StatusCode::OK);
        let states: Vec<&str> = (0..6).map(|i| state_of(&v, i)).collect();
        assert_eq!(
            states,
            [
                "ready",
                "unreachable",
                "off",
                "starting",
                "ready",
                "unreachable"
            ]
        );
        let m = &v["machines"];
        assert_eq!(
            (m[0]["id"].as_str(), m[0]["name"].as_str()),
            (Some("plain"), Some("plain-name"))
        );
        assert_eq!(
            (m[0]["canPower"].as_bool(), m[2]["canPower"].as_bool()),
            (Some(false), Some(true))
        );
        assert_eq!(
            (m[0]["sunshine"].as_str(), m[0]["host"]["name"].as_str()),
            (Some("ready"), Some("up"))
        );
        assert!(m[1]["host"].is_null() && m[1]["sunshine"].is_null() && m[2]["host"].is_null());
        assert_eq!(
            m[0]["url"].as_str(),
            Some(r.hub.machines[0].cfg.url.as_str())
        );
        // rttMs: a number when the TCP connect worked, null otherwise; vm only for a running VM.
        let rtt = |i: usize| m[i]["rttMs"].as_u64();
        assert!(rtt(0).is_some() && rtt(4).is_some());
        assert!([1, 2, 3, 5].iter().all(|&i| m[i]["rttMs"].is_null()));
        assert_eq!(
            m[4]["vm"],
            json!({ "cpuPct": 25.6, "memUsedBytes": 2048, "memTotalBytes": 8192 })
        );
        assert!(m[0]["vm"].is_null() && m[2]["vm"].is_null() && m[5]["vm"].is_null());
        assert!(m[3]["vm"].is_object() && m[3]["rttMs"].is_null());
        // Running but Sunshine not ready is still starting.
        drop(world);
        let (url2, w2) = upstream().await;
        w2.lock().unwrap().sunshine = "down";
        let r2 = rig(vec![("s", url2, Some(Fake::new(VmState::Running)))]);
        let (_, v) = getj(&r2.app, "/api/machines").await;
        assert_eq!(
            (state_of(&v, 0), v["machines"][0]["sunshine"].as_str()),
            ("starting", Some("down"))
        );
    }

    #[tokio::test]
    async fn vm_is_null_when_stats_fail() {
        let (url, _) = upstream().await;
        let p = Arc::new(Fake {
            stats: None,
            ..Arc::into_inner(Fake::new(VmState::Running)).unwrap()
        });
        let r = rig(vec![("a", url, Some(p))]);
        let (_, v) = getj(&r.app, "/api/machines").await;
        assert_eq!(state_of(&v, 0), "ready");
        assert!(v["machines"][0]["vm"].is_null());
    }

    /// A temp sensor on loopback answering `body` at /t.
    async fn temp_server(body: &'static str) -> String {
        let app = axum::Router::new().route("/t", axum::routing::get(move || async move { body }));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/t", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        url
    }

    #[tokio::test]
    async fn temp_url_fills_only_a_missing_cpu_temp() {
        let temp_of = |v: &Value| v["machines"][0]["host"]["cpuTempC"].clone();
        for (body, want) in [
            (r#"{"cpuTempC": 61.5}"#, json!(61.5)),
            (r#"{"cpuTempC": 151}"#, Value::Null),
            (r#"{"cpuTempC": -1}"#, Value::Null),
            (r#"{"cpuTempC": "hot"}"#, Value::Null),
            ("nope", Value::Null),
        ] {
            let (url, _) = upstream().await;
            let r = rig_t(
                vec![("a", url, Some(Fake::new(VmState::Running)))],
                Some(temp_server(body).await),
            );
            assert_eq!(
                temp_of(&getj(&r.app, "/api/machines").await.1),
                want,
                "{body}"
            );
        }
        // The machine's own reading wins.
        let (url, w) = upstream().await;
        w.lock().unwrap().temp = Some(40.0);
        let r = rig_t(
            vec![("a", url, Some(Fake::new(VmState::Running)))],
            Some(temp_server(r#"{"cpuTempC": 61.5}"#).await),
        );
        assert_eq!(temp_of(&getj(&r.app, "/api/machines").await.1), json!(40.0));
        // Unreachable sensor: left null; a host that did not answer gets nothing.
        let (url, _) = upstream().await;
        let r = rig_t(
            vec![
                ("a", url, Some(Fake::new(VmState::Running))),
                ("b", dead_url().await, Some(Fake::new(VmState::Running))),
            ],
            Some(dead_url().await),
        );
        let (_, v) = getj(&r.app, "/api/machines").await;
        assert!(temp_of(&v).is_null() && v["machines"][1]["host"].is_null());
    }

    #[tokio::test]
    async fn machines_list_is_cached() {
        let (url, _) = upstream().await;
        let p = Fake::new(VmState::Running);
        let r = rig(vec![("a", url, Some(p.clone()))]);
        getj(&r.app, "/api/machines").await;
        getj(&r.app, "/api/machines").await;
        assert_eq!(p.status_calls.load(Ordering::SeqCst), 1);
        r.hub.invalidate("a");
        getj(&r.app, "/api/machines").await;
        assert_eq!(p.status_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn power_route() {
        let (url, _) = upstream().await;
        let off = Fake::new(VmState::Stopped);
        let run = Fake::new(VmState::Running);
        let bad = Arc::new(Fake {
            fail_action: Some("start"),
            ..Arc::into_inner(Fake::new(VmState::Stopped)).unwrap()
        });
        let r = rig(vec![
            ("plain", url.clone(), None),
            ("off", url.clone(), Some(off.clone())),
            ("run", url.clone(), Some(run.clone())),
            ("bad", url, Some(bad)),
        ]);
        let start = r#"{"action":"start"}"#;
        let down = r#"{"action":"shutdown"}"#;
        assert_eq!(
            post_power(&r.app, "zzz", start).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            post_power(&r.app, "off", r#"{"action":"reboot"}"#).await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_power(&r.app, "off", "x").await.0,
            StatusCode::BAD_REQUEST
        );
        let (st, v) = post_power(&r.app, "plain", start).await;
        assert_eq!(st, StatusCode::CONFLICT);
        assert!(v["error"].is_string());
        let (st, v) = post_power(&r.app, "bad", start).await;
        assert_eq!(
            (st, v["error"].as_str()),
            (StatusCode::BAD_GATEWAY, Some("proxmox said no"))
        );
        // shutdown of a stopped VM and start of a running one are 200 no-ops
        let (st, v) = post_power(&r.app, "off", down).await;
        assert_eq!((st, v["state"].as_str()), (StatusCode::OK, Some("off")));
        assert!(off.acts.lock().unwrap().is_empty());
        let (st, v) = post_power(&r.app, "run", start).await;
        assert_eq!((st, v["state"].as_str()), (StatusCode::OK, Some("ready")));
        assert!(run.acts.lock().unwrap().is_empty());
        let (st, v) = post_power(&r.app, "off", start).await;
        assert_eq!(
            (st, v["state"].as_str()),
            (StatusCode::ACCEPTED, Some("starting"))
        );
        assert_eq!(*off.acts.lock().unwrap(), ["start"]);
        // Proxmox still says stopped for a moment: the list says starting, then off once the wait is over
        *off.vm.lock().unwrap() = Ok(VmState::Stopped);
        r.hub.invalidate("off");
        let (_, v) = getj(&r.app, "/api/machines").await;
        assert_eq!(state_of(&v, 1), "starting");
        r.hub.starting.lock().unwrap().insert(
            "off".into(),
            Instant::now() - STARTING_FOR - Duration::from_secs(1),
        );
        r.hub.invalidate("off");
        let (_, v) = getj(&r.app, "/api/machines").await;
        assert_eq!(state_of(&v, 1), "off");
        // shutdown of a running VM: 202, then the list says stopping
        let (st, v) = post_power(&r.app, "run", down).await;
        assert_eq!(
            (st, v["state"].as_str()),
            (StatusCode::ACCEPTED, Some("stopping"))
        );
        assert_eq!(*run.acts.lock().unwrap(), ["shutdown"]);
        let (_, v) = getj(&r.app, "/api/machines").await;
        assert_eq!(state_of(&v, 2), "stopping");
        // past 120 s it is just running again
        r.hub.stopping.lock().unwrap().insert(
            "run".into(),
            Instant::now() - STOPPING_FOR - Duration::from_secs(1),
        );
        r.hub.invalidate("run");
        let (_, v) = getj(&r.app, "/api/machines").await;
        assert_eq!(state_of(&v, 2), "ready");
    }

    #[tokio::test]
    async fn observe_reads_the_machine() {
        let (url, w) = upstream().await;
        let r = rig(vec![
            ("a", url, Some(Fake::new(VmState::Running))),
            ("d", dead_url().await, Some(Fake::new(VmState::Running))),
        ]);
        let (h, e, d) = (
            r.hub.clone(),
            r.hub.get("a").unwrap(),
            r.hub.get("d").unwrap(),
        );
        // Blocking calls off the runtime thread, which also serves the fake machine.
        let see = |e: &Arc<Entry>| {
            let (h, e) = (h.clone(), e.clone());
            async move {
                tokio::task::spawn_blocking(move || observe(&h, &e))
                    .await
                    .unwrap()
            }
        };
        let idle = Observed {
            streaming: false,
            game_running: false,
            updating: false,
        };
        assert_eq!(see(&e).await, Some(idle));
        *w.lock().unwrap() = World {
            sunshine: "ready",
            stream: "live",
            running: true,
            update: Some("running"),
            temp: None,
        };
        assert_eq!(
            see(&e).await,
            Some(Observed {
                streaming: true,
                game_running: true,
                updating: true
            })
        );
        w.lock().unwrap().sunshine = "down";
        assert_eq!(see(&e).await, None);
        assert_eq!(see(&d).await, None);
    }
}
