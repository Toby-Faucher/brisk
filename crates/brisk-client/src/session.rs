//! GameStream HTTP(S) session against Sunshine: minimal HTTP/1.1 GET over TCP, or over
//! rustls (pure-Rust RustCrypto provider) with the client cert and a pinned server cert.

use crate::identity::{Identity, ServerCert};
use crate::recorder::Recorder;
use crate::rtsp::{LaunchKey, StreamPorts, handshake};
use crate::stop::Stop;
use brisk_proto::http::{
    App, LaunchOpts, LaunchReply, MAX_BODY, ServerInfo, cancel_query, check_status, launch_query,
    parse_applist, parse_field, parse_launch, parse_serverinfo,
};
use brisk_proto::pair::{PairError, PairingClient, Rng, unhex};
use brisk_proto::sdp::StreamConfig;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use x509_cert::der::{DecodePem, Encode};

pub const HTTP_PORT: u16 = 47989;
pub const HTTPS_PORT: u16 = 47984;
/// Sunshine holds `getservercert` open until the PIN is entered, and expires the session after 5 min.
const PIN_WAIT: Duration = Duration::from_secs(320);
const WAIT: Duration = Duration::from_secs(10);
/// `/launch` returns only after Sunshine has started the app.
const LAUNCH_WAIT: Duration = Duration::from_secs(120);

pub struct Session {
    pub host: String,
    pub http_port: u16,
    /// Updated from every plain-HTTP `serverinfo`.
    pub https_port: u16,
    pub recorder: Option<Recorder>,
    /// Per-attempt timeout and base backoff for `cancel_retry` (fields so tests can shrink them).
    #[doc(hidden)]
    pub cancel_wait: Duration,
    #[doc(hidden)]
    pub cancel_backoff: Duration,
    launch_sent: bool,
    launch_confirmed: bool,
    /// Draws every launch key.
    rng: Arc<Mutex<dyn Rng + Send>>,
}

const CANCEL_ATTEMPTS: u32 = 3;

type Tls<'a> = Option<(&'a Identity, &'a ServerCert)>;

impl Session {
    pub fn new(host: &str) -> Self {
        Session {
            host: host.to_owned(),
            http_port: HTTP_PORT,
            https_port: HTTPS_PORT,
            recorder: None,
            cancel_wait: Duration::from_secs(15),
            cancel_backoff: Duration::from_secs(1),
            launch_sent: false,
            launch_confirmed: false,
            rng: Arc::new(Mutex::new(crate::SystemRng)),
        }
    }

    /// `launch_session` is about to send (or has sent) `/launch`: a session may be ours.
    pub fn launch_sent(&self) -> bool {
        self.launch_sent
    }

    /// `/launch` came back 200: a session is certainly ours.
    pub fn launch_confirmed(&self) -> bool {
        self.launch_confirmed
    }

    /// Replaces the CSPRNG (tests only: a predictable rng makes predictable keys and pairing draws).
    #[doc(hidden)]
    pub fn set_rng(&mut self, rng: impl Rng + Send + 'static) {
        self.rng = Arc::new(Mutex::new(rng));
    }

    /// Plain HTTP when `paired` is None; HTTPS with client cert and pinned server cert otherwise
    /// (only then does Sunshine report `PairStatus` 1).
    pub async fn serverinfo(&mut self, paired: Tls<'_>) -> crate::Result<ServerInfo> {
        let q: Vec<_> = paired
            .iter()
            .map(|(id, _)| ("uniqueid".to_string(), id.unique_id.clone()))
            .collect();
        let info = parse_serverinfo(&self.get(paired, "/serverinfo", &q, WAIT).await?)?;
        if paired.is_none() {
            self.https_port = info.https_port;
        }
        Ok(info)
    }

    pub async fn applist(
        &mut self,
        ident: &Identity,
        server: &ServerCert,
    ) -> crate::Result<Vec<App>> {
        let q = [("uniqueid".to_string(), ident.unique_id.clone())];
        Ok(parse_applist(
            &self
                .get(Some((ident, server)), "/applist", &q, WAIT)
                .await?,
        )?)
    }

    /// `GET /launch` (HTTPS) with a launch key drawn here, fresh every call: the control/RTSP
    /// GCM IVs restart per session, so a reused key would repeat nonces. The key is scrubbed from
    /// recordings (it keys the whole stream).
    pub async fn launch(
        &mut self,
        ident: &Identity,
        server: &ServerCert,
        app_id: u32,
        cfg: &StreamConfig,
    ) -> crate::Result<(LaunchReply, LaunchKey)> {
        let (mut rikey, mut id) = ([0u8; 16], [0u8; 4]);
        {
            let mut rng = self.rng.lock().unwrap();
            rng.fill(&mut rikey);
            rng.fill(&mut id);
        }
        // Sunshine parses rikeyid as a signed int; stay positive.
        let key = LaunchKey {
            rikey,
            rikeyid: u32::from_le_bytes(id) & 0x7fff_ffff,
        };
        if let Some(rec) = &mut self.recorder {
            rec.add_secret(&key.rikey);
        }
        let q = launch_query(
            &ident.unique_id,
            app_id,
            cfg.width,
            cfg.height,
            cfg.fps,
            key.rikey,
            key.rikeyid,
            &LaunchOpts::default(),
        );
        let r = self
            .get(Some((ident, server)), "/launch", &q, LAUNCH_WAIT)
            .await?;
        Ok((parse_launch(&r)?, key))
    }

    /// `GET /cancel` (HTTPS): Sunshine stops the stream and the app.
    pub async fn cancel(&mut self, ident: &Identity, server: &ServerCert) -> crate::Result<()> {
        let q = cancel_query(&ident.unique_id);
        let w = self.cancel_wait;
        check_status(&self.get(Some((ident, server)), "/cancel", &q, w).await?)?;
        Ok(())
    }

    /// `cancel` with up to 3 attempts and a growing backoff (a single 10 s timeout was seen live).
    pub async fn cancel_retry(
        &mut self,
        ident: &Identity,
        server: &ServerCert,
    ) -> crate::Result<()> {
        let mut last = String::new();
        for n in 1..=CANCEL_ATTEMPTS {
            match self.cancel(ident, server).await {
                Ok(()) => return Ok(()),
                Err(e) => last = e.to_string(),
            }
            if n < CANCEL_ATTEMPTS {
                tokio::time::sleep(self.cancel_backoff * n).await;
            }
        }
        Err(crate::Error::Session(format!(
            "cancel FAILED after {CANCEL_ATTEMPTS} attempts: session may still be running on host; run `brisk-probe cancel --host {}` ({last})",
            self.host
        )))
    }

    /// After an ambiguous `/launch`: cancel only if the host now runs the app we asked for.
    pub async fn cancel_if_ours(
        &mut self,
        ident: &Identity,
        server: &ServerCert,
        app_id: u32,
    ) -> crate::Result<()> {
        let unknown = format!(
            "launch outcome unknown; run `brisk-probe cancel --host {}`",
            self.host
        );
        match self.serverinfo(Some((ident, server))).await {
            Ok(i) if i.current_game == app_id => self.cancel_retry(ident, server).await,
            Ok(i) if i.current_game == 0 => Err(crate::Error::Session(
                "launch did not start anything; nothing cancelled".into(),
            )),
            Ok(i) => Err(crate::Error::Session(format!(
                "{unknown} (host runs app {}, not ours; not cancelling it)",
                i.current_game
            ))),
            Err(e) => Err(crate::Error::Session(format!(
                "{unknown} (serverinfo failed: {e})"
            ))),
        }
    }

    /// Refuse (or, with `take_over`, cancel) when the host already runs something, launch,
    /// RTSP handshake, then cancel. Only a session this call started is ever cancelled: a rejected
    /// launch (Sunshine answered with a status) and a busy pre-check never reach `/cancel`.
    pub async fn launch_probe(
        &mut self,
        ident: &Identity,
        server: &ServerCert,
        app_id: u32,
        app_version: &str,
        cfg: &StreamConfig,
        take_over: bool,
    ) -> crate::Result<StreamPorts> {
        self.launch_session(
            ident,
            server,
            app_id,
            app_version,
            cfg,
            take_over,
            |p| async { Ok(p) },
        )
        .await
    }

    /// [`Self::launch_probe`] that runs `during` on the stream ports after PLAY and before the
    /// cancel (e.g. the control stream). The cancel rules are the same.
    #[expect(clippy::too_many_arguments)]
    pub async fn launch_session<T, F, Fut>(
        &mut self,
        ident: &Identity,
        server: &ServerCert,
        app_id: u32,
        app_version: &str,
        cfg: &StreamConfig,
        take_over: bool,
        during: F,
    ) -> crate::Result<T>
    where
        F: FnOnce(StreamPorts) -> Fut,
        Fut: std::future::Future<Output = crate::Result<T>>,
    {
        let info = self.serverinfo(Some((ident, server))).await?;
        if info.current_game != 0 || info.server_state != "SUNSHINE_SERVER_FREE" {
            let busy = format!(
                "server busy: an app/session is already running (currentgame={}, state={})",
                info.current_game, info.server_state
            );
            if !take_over {
                return Err(crate::Error::Busy(format!(
                    "{busy}; not cancelling it (pass --take-over to replace it)"
                )));
            }
            tracing::warn!("--take-over: {busy}; cancelling it NOW");
            self.cancel_retry(ident, server).await?;
        }
        self.launch_sent = true;
        let (l, key) = match self.launch(ident, server, app_id, cfg).await {
            Ok(l) => l,
            Err(e) => {
                use brisk_proto::http::HttpError::Status;
                // A complete non-200 reply (HTTP or XML status) means Sunshine refused: never cancel.
                match e.root() {
                    crate::Error::Http(Status { code: 400, .. }) => {
                        let game = match self.serverinfo(Some((ident, server))).await {
                            Ok(i) => i.current_game.to_string(),
                            Err(_) => "unknown".into(),
                        };
                        return Err(crate::Error::Busy(format!(
                            "server busy: an app/session is already running (currentgame={game}); not cancelling it ({e})"
                        )));
                    }
                    crate::Error::Http(Status { .. }) => {
                        return Err(crate::Error::Session(format!(
                            "launch rejected: {e}; nothing to cancel"
                        )));
                    }
                    _ => {}
                }
                // Timeout, reset or unparseable 200: the app may have started.
                return Err(match self.cancel_if_ours(ident, server, app_id).await {
                    Ok(()) => e.then(crate::Error::Session("session cancelled".into())),
                    Err(c) => e.then(c),
                });
            }
        };
        self.launch_confirmed = true;
        let r = handshake(
            &self.host,
            &l.session_url,
            app_version,
            cfg,
            &key,
            self.recorder.as_mut(),
        )
        .await;
        let r = match r {
            // A panic in `during` must not skip the cancel below.
            Ok(p) => CatchUnwind(Box::pin(during(p))).await.unwrap_or_else(|_| {
                Err(crate::Error::Session("panicked during the session".into()))
            }),
            Err(e) => Err(e),
        };
        let c = self.cancel_retry(ident, server).await;
        match (r, c) {
            (Ok(p), Ok(())) => Ok(p),
            (Ok(_), Err(c)) => Err(c),
            (Err(e), Ok(())) => Err(e),
            (Err(e), Err(c)) => Err(e.then(c)),
        }
    }

    /// [`Self::launch_session`] that stops early on `stop`. Until `during` starts (serverinfo, a
    /// take-over cancel, `/launch`, the RTSP handshake) a request drops the work in progress and
    /// cancels whatever this call may have started: nothing before `/launch`, the app only if the
    /// host runs it after an unanswered `/launch`, the session after a confirmed one. The error
    /// says which. A second request during that cancel returns [`crate::Error::Abandoned`] at
    /// once.
    ///
    /// Once `during` runs, it owns the stop (it gets a clone) and must return soon after a request,
    /// so it can release what it holds. The final cancel still runs, and a second request still
    /// cuts it short. Without a request this is exactly `launch_session`.
    #[expect(clippy::too_many_arguments)]
    pub async fn launch_session_until<T, F, Fut>(
        &mut self,
        stop: &mut Stop,
        ident: &Identity,
        server: &ServerCert,
        app_id: u32,
        app_version: &str,
        cfg: &StreamConfig,
        take_over: bool,
        during: F,
    ) -> crate::Result<T>
    where
        F: FnOnce(StreamPorts, Stop) -> Fut,
        Fut: std::future::Future<Output = crate::Result<T>>,
    {
        let host = self.host.clone();
        let abandoned = || {
            crate::Error::Abandoned(format!(
                "second stop request: not waiting for the cancel; run `brisk-probe cancel --host {host}` if the session still runs"
            ))
        };
        let (started, during_stop, mut second) =
            (std::cell::Cell::new(false), stop.clone(), stop.clone());
        let sig = {
            let run =
                self.launch_session(ident, server, app_id, app_version, cfg, take_over, |p| {
                    started.set(true);
                    during(p, during_stop)
                });
            let mut run = std::pin::pin!(run);
            let sig = tokio::select! {
                r = &mut run => return r,
                sig = stop.requested() => sig,
            };
            if started.get() {
                return tokio::select! {
                    r = &mut run => r,
                    _ = second.repeated() => Err(abandoned()),
                };
            }
            sig
        };
        tracing::warn!("{sig}: stopping");
        if !self.launch_sent() {
            return Err(crate::Error::Session(if take_over {
                "interrupted before launch; take-over cancel may have been processed".into()
            } else {
                "interrupted before launch; nothing cancelled".into()
            }));
        }
        let confirmed = self.launch_confirmed();
        let cancel = async {
            match confirmed {
                true => self.cancel_retry(ident, server).await,
                false => self.cancel_if_ours(ident, server, app_id).await,
            }
        };
        tokio::select! {
            c = cancel => {
                c?;
                Err(crate::Error::Session(format!("interrupted ({sig}); session cancelled")))
            }
            _ = second.repeated() => Err(abandoned()),
        }
    }

    /// Phases 1-4 over HTTP, phase 5 (`pairchallenge`) over HTTPS pinned to the phase-1 cert.
    /// Writes `dir/server.pem` only after phase 5 succeeds. Salt, challenge and secret come from
    /// the session's CSPRNG.
    pub async fn pair(
        &mut self,
        dir: &Path,
        ident: &Identity,
        pin: &str,
    ) -> crate::Result<ServerCert> {
        if let Some(rec) = &mut self.recorder {
            identity_secrets(ident)
                .iter()
                .for_each(|s| rec.add_secret(s)); // client cert travels hex in phase 1
        }
        let drawn = Arc::new(Mutex::new(vec![]));
        let r = self
            .pair_inner(ident, pin, Box::new(Tap(self.rng.clone(), drawn.clone())))
            .await;
        if let Some(rec) = &mut self.recorder {
            drawn.lock().unwrap().iter().for_each(|s| rec.add_secret(s));
        }
        let server = r?;
        server.save(dir)?;
        Ok(server)
    }

    async fn pair_inner(
        &mut self,
        ident: &Identity,
        pin: &str,
        mut rng: Box<dyn Rng>,
    ) -> crate::Result<ServerCert> {
        self.serverinfo(None).await?; // learn HttpsPort for phase 5
        let mut salt = [0u8; 16];
        rng.fill(&mut salt);
        let mut pc = PairingClient::new(&ident.cert_pem, &ident.key_pem, &ident.unique_id, rng)?;

        let r = self
            .get(None, "/pair", &pc.getservercert(salt, pin), PIN_WAIT)
            .await?;
        let plain = paired_field(&r, "plaincert")?;
        let server = ServerCert {
            pem: String::from_utf8(unhex(&plain)?).map_err(|_| PairError::BadCert)?,
        };
        if let Some(rec) = &mut self.recorder {
            rec.add_secret(server.pem.as_bytes());
        }
        let q = pc.on_servercert(&plain)?;

        let r = self.get(None, "/pair", &q, WAIT).await?;
        let q = pc.on_clientchallenge_resp(&paired_field(&r, "challengeresponse")?)?;

        let r = self.get(None, "/pair", &q, WAIT).await?;
        let ps = paired_field(&r, "pairingsecret")?;
        if let (Some(rec), Ok(b)) = (&mut self.recorder, unhex(&ps)) {
            rec.add_secret(&b[..b.len().min(16)]); // server secret travels in clear hex
        }
        let q = pc.on_serverchallenge_resp(&ps).map_err(|e| match e {
            PairError::HashMismatch => crate::Error::Session(
                "pairing failed: wrong PIN (server secret does not match the PIN-derived hash)"
                    .into(),
            ),
            e => crate::Error::from(e),
        })?;

        let r = self.get(None, "/pair", &q, WAIT).await?;
        let q = pc
            .on_pairingsecret_resp(&parse_field(&r, "paired")?)
            .map_err(|_| {
                crate::Error::Session("pairing failed: server rejected our pairing secret".into())
            })?;

        let r = self.get(Some((ident, &server)), "/pair", &q, WAIT).await?;
        paired_field(&r, "paired")?;
        Ok(server)
    }

    /// One GET with `Connection: close`; returns the body. Records request and response bytes.
    async fn get(
        &mut self,
        tls: Tls<'_>,
        path: &str,
        query: &[(String, String)],
        wait: Duration,
    ) -> crate::Result<String> {
        let port = if tls.is_some() {
            self.https_port
        } else {
            self.http_port
        };
        let q: Vec<_> = query
            .iter()
            .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
            .collect();
        let req = format!(
            "GET {path}?{} HTTP/1.1\r\nHost: {}:{port}\r\nConnection: close\r\n\r\n",
            q.join("&"),
            self.host
        );
        let host = self.host.clone();
        let raw = tokio::time::timeout(wait, async {
            let tcp = TcpStream::connect((host.as_str(), port)).await?;
            match tls {
                None => exchange(tcp, &req).await,
                Some((id, srv)) => {
                    let name = ServerName::try_from(host.clone())
                        .map_err(|e| crate::Error::Setup(format!("host name: {e}")))?;
                    let cfg = tls_config(id, srv)
                        .map_err(|e| crate::Error::Setup(format!("tls config: {e}")))?;
                    let s = tokio_rustls::TlsConnector::from(Arc::new(cfg))
                        .connect(name, tcp)
                        .await?;
                    exchange(s, &req).await
                }
            }
        })
        .await
        .map_err(|_| {
            crate::Error::Timeout(format!(
                "{path}: no reply from {}:{port} within {}s",
                self.host,
                wait.as_secs()
            ))
        })?
        .map_err(|e| e.context(format!("{path} ({}:{port})", self.host)))?;
        let label = path.trim_start_matches('/');
        if let Some(rec) = &mut self.recorder {
            if let Some((id, srv)) = tls {
                for s in identity_secrets(id)
                    .into_iter()
                    .chain([srv.pem.clone().into_bytes()])
                {
                    rec.add_secret(&s);
                }
            }
            rec.push(&format!("{label}-req"), tls.is_some(), req.as_bytes());
            rec.push(&format!("{label}-resp"), tls.is_some(), &raw);
        }
        // Keep a refusal (`HttpError::Status`) intact: callers decide on it, e.g. never cancel.
        brisk_proto::http::response_body(&raw, tls.is_some()).map_err(|e| match e {
            e @ brisk_proto::http::HttpError::Status { .. } => e.into(),
            e => crate::Error::from(e).context(path),
        })
    }
}

/// Requires `paired == 1` (Sunshine's failure replies carry 0 plus a status message), then returns `name`.
fn paired_field(xml: &str, name: &'static str) -> crate::Result<String> {
    if parse_field(xml, "paired")? != "1" {
        return Err(crate::Error::Session(
            "pairing failed: server answered paired=0".into(),
        ));
    }
    Ok(parse_field(xml, name)?)
}

/// Key PEM, key DER and client cert PEM: what the recorder must never write.
fn identity_secrets(id: &Identity) -> Vec<Vec<u8>> {
    let mut v = vec![
        id.key_pem.clone().into_bytes(),
        id.cert_pem.clone().into_bytes(),
    ];
    if let Ok(d) =
        <rsa::pkcs8::SecretDocument as rsa::pkcs8::DecodePrivateKey>::from_pkcs8_pem(&id.key_pem)
    {
        v.push(d.as_bytes().to_vec());
    }
    v
}

/// A future whose panic is returned as `Err` instead of unwinding.
struct CatchUnwind<F>(std::pin::Pin<Box<F>>);
impl<F: std::future::Future> std::future::Future for CatchUnwind<F> {
    type Output = std::thread::Result<F::Output>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let f = &mut self.0;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.as_mut().poll(cx))) {
            Ok(p) => p.map(Ok),
            Err(e) => std::task::Poll::Ready(Err(e)),
        }
    }
}

/// Draws from the session rng and copies every draw so the recorder can scrub it.
struct Tap(Arc<Mutex<dyn Rng + Send>>, Arc<Mutex<Vec<Vec<u8>>>>);
impl Rng for Tap {
    fn fill(&mut self, buf: &mut [u8]) {
        self.0.lock().unwrap().fill(buf);
        self.1.lock().unwrap().push(buf.to_vec());
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut s: S,
    req: &str,
) -> crate::Result<Vec<u8>> {
    s.write_all(req.as_bytes()).await?;
    s.flush().await?;
    let limit = MAX_BODY + 16 * 1024;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match s.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            // Peer closed TLS without close_notify; `response_body` still enforces Content-Length.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        if buf.len() > limit {
            return Err(crate::Error::Protocol("http response too large".into()));
        }
    }
    Ok(buf)
}

type BoxResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn tls_config(id: &Identity, srv: &ServerCert) -> BoxResult<rustls::ClientConfig> {
    let p = Arc::new(rustls_rustcrypto::provider());
    let pem_der = |pem: &str| -> BoxResult<CertificateDer<'static>> {
        Ok(x509_cert::Certificate::from_pem(pem)?.to_der()?.into())
    };
    let key =
        <rsa::pkcs8::SecretDocument as rsa::pkcs8::DecodePrivateKey>::from_pkcs8_pem(&id.key_pem)?;
    let verifier = PinnedCert {
        der: pem_der(&srv.pem)?.to_vec(),
        algs: p.signature_verification_algorithms,
    };
    Ok(rustls::ClientConfig::builder_with_provider(p)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_client_auth_cert(
            vec![pem_der(&id.cert_pem)?],
            PrivateKeyDer::Pkcs8(key.as_bytes().to_vec().into()),
        )?)
}

/// Accepts exactly one server cert (byte-equal DER): no CA chain, no hostname, no expiry check,
/// as GameStream server certs are self-signed. Handshake signatures are still verified.
#[derive(Debug)]
struct PinnedCert {
    der: Vec<u8>,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedCert {
    fn verify_server_cert(
        &self,
        ee: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if ee.as_ref() == self.der.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ))
        }
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.algs)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.algs)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enc_escapes_query_values() {
        assert_eq!(enc("a b/+Z~"), "a%20b%2F%2BZ~");
    }
}
