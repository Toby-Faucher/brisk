//! The hub: machines.json, GET /api/machines, the power route and the /api/m/{id}/* proxy
//! (docs/plans/machine-hub.md). Secrets are read from their files at each use, never stored and
//! never in a response, log line or error text.

use crate::http::AppState;
use crate::idle::{IdleWatch, Observed};
use crate::power::{Power, PowerCfg, ProxmoxCfg, VmState};
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
const PROBE_TTL: Duration = Duration::from_secs(3);
const STOPPING_FOR: Duration = Duration::from_secs(120);

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

/// machines.json text to validated machines; errors name the machine, never a secret.
pub fn parse(text: &str) -> Result<Vec<MachineCfg>, String> {
    let f: File = serde_json::from_str(text).map_err(|e| format!("not valid: {e}"))?;
    if f.machines.is_empty() {
        return Err("no machines".into());
    }
    let mut seen = std::collections::HashSet::new();
    for m in &f.machines {
        let id = &m.id;
        if id.is_empty()
            || id.len() > 32
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(format!("machine id {id:?} must be 1-32 of a-z, 0-9, -"));
        }
        if !seen.insert(id.as_str()) {
            return Err(format!("machine id {id:?} is used twice"));
        }
        url_ok(&m.url).map_err(|e| format!("machine {id}: {e}"))?;
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

pub struct Hub {
    machines: Vec<Arc<Entry>>,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
    /// When a shutdown was last requested, for the `stopping` state.
    stopping: Mutex<HashMap<String, Instant>>,
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
        }
    }

    fn get(&self, id: &str) -> Option<Arc<Entry>> {
        self.machines.iter().find(|e| e.cfg.id == id).cloned()
    }

    fn invalidate(&self, id: &str) {
        self.cache.lock().unwrap().remove(id);
    }

    fn is_stopping(&self, id: &str) -> bool {
        self.stopping
            .lock()
            .unwrap()
            .get(id)
            .is_some_and(|t| t.elapsed() < STOPPING_FOR)
    }

    fn view(e: &Entry, state: &str, host: Option<Value>) -> Value {
        let sunshine = host.as_ref().and_then(|h| h.get("sunshine")).cloned();
        json!({ "id": e.cfg.id, "name": e.cfg.name, "state": state,
                "canPower": e.power.is_some(), "sunshine": sunshine, "host": host })
    }

    /// Blocking: one state probe (Proxmox, then the machine's /api/host).
    fn probe(&self, e: &Entry) -> Value {
        let host = || get_json(e, "host");
        match &e.power {
            None => match host() {
                Some(h) => Self::view(e, "ready", Some(h)),
                None => Self::view(e, "unreachable", None),
            },
            Some(p) => match p.status() {
                Err(_) => Self::view(e, "unreachable", None),
                Ok(VmState::Stopped) => {
                    self.stopping.lock().unwrap().remove(&e.cfg.id);
                    Self::view(e, "off", None)
                }
                Ok(VmState::Running) => {
                    let h = host();
                    let ready = h.as_ref().and_then(|h| h["sunshine"].as_str()) == Some("ready");
                    let state = if self.is_stopping(&e.cfg.id) {
                        "stopping"
                    } else if ready {
                        "ready"
                    } else {
                        "starting"
                    };
                    Self::view(e, state, h)
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
    body: Vec<u8>,
}

/// Blocking call to `{url}/api/{pq}`. TLS as in steam.rs `https_get`; no proxy, no redirects, so
/// the machine's secret only ever goes to the configured URL.
fn call(
    e: &Entry,
    method: &str,
    pq: &str,
    heads: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> Result<Up, Fail> {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let secret = std::fs::read_to_string(&e.cfg.secret_file).map_err(|_| Fail::Local)?;
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls_graviola::default_provider()))
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(3)))
        .timeout_global(Some(timeout))
        .build()
        .into();
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
    let (ctype, seq, status) = (h("content-type"), h("x-brisk-seq"), r.status().as_u16());
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

fn err(code: StatusCode, msg: &str) -> Response {
    (code, axum::Json(json!({ "error": msg }))).into_response()
}

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
            (StatusCode::ACCEPTED, "starting".into())
        }
        (Verb::Shutdown, VmState::Stopped) => (StatusCode::OK, "off".into()),
        (Verb::Shutdown, VmState::Running) => {
            p.shutdown()?;
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

pub async fn power(
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
pub async fn proxy(State(s): State<Arc<AppState>>, req: Request) -> Response {
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
    let hv = |n| {
        parts
            .headers
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(String::from)
    };
    let (ct, accept) = (hv(header::CONTENT_TYPE), hv(header::ACCEPT));
    let Ok(body) = axum::body::to_bytes(body, CAP).await else {
        return err(StatusCode::PAYLOAD_TOO_LARGE, "request body is over 16 MiB");
    };
    let r = tokio::task::spawn_blocking(move || {
        let mut heads = vec![];
        if let Some(c) = &ct {
            heads.push(("content-type", c.as_str()));
        }
        if let Some(a) = &accept {
            heads.push(("accept", a.as_str()));
        }
        call(&e, &method, &pq, &heads, &body, Duration::from_secs(40)).map_err(|f| match f {
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
                    h2.stopping
                        .lock()
                        .unwrap()
                        .insert(e2.cfg.id.clone(), Instant::now());
                    h2.invalidate(&e2.cfg.id);
                    Ok::<(), String>(())
                })
                .await;
                match done {
                    Ok(Ok(())) => eprintln!("idle: shutting down {}", e.cfg.id),
                    _ => eprintln!("idle: shutdown of {} failed", e.cfg.id),
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
    }

    /// A fake hostd on an ephemeral port; returns its URL and the world it reports.
    async fn upstream() -> (String, Arc<Mutex<World>>) {
        use axum::routing::{any, get};
        let w = Arc::new(Mutex::new(World {
            sunshine: "ready",
            stream: "idle",
            running: false,
            update: None,
        }));
        let (w1, w2, w3) = (w.clone(), w.clone(), w.clone());
        let app = axum::Router::new()
            .route(
                "/api/host",
                get(move || async move {
                    json!({ "name": "up", "sunshine": w1.lock().unwrap().sunshine }).to_string()
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
                    (StatusCode::CONFLICT, [("content-type", "application/json")], r#"{"error":"nope"}"#)
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
    async fn dead_url() -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        format!("http://{}", l.local_addr().unwrap())
    }

    struct Fake {
        vm: Mutex<Result<VmState, String>>,
        status_calls: AtomicUsize,
        fail_action: Option<&'static str>,
        acts: Mutex<Vec<&'static str>>,
    }
    impl Fake {
        fn new(vm: VmState) -> Arc<Fake> {
            Arc::new(Fake {
                vm: Mutex::new(Ok(vm)),
                status_calls: AtomicUsize::new(0),
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
                        "node": "n", "vmid": 1, "token_id": "u@pve!t", "token_file": "/nonexistent" } });
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
        for ok in ["host", "games/1/info", "sessions/5/series", "a%20b"] {
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
        // An upstream 404 is an upstream 404.
        assert_eq!(
            send(&r.app, "GET", "/api/m/a/nothing", None, vec![])
                .await
                .0,
            StatusCode::NOT_FOUND
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
