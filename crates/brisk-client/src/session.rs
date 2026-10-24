//! GameStream HTTP(S) session against Sunshine: minimal HTTP/1.1 GET over TCP, or over
//! rustls (pure-Rust RustCrypto provider) with the client cert and a pinned server cert.

use crate::identity::{Identity, ServerCert};
use crate::recorder::Recorder;
use crate::rtsp::{handshake, StreamPorts};
use brisk_proto::http::{
    cancel_query, check_status, launch_query, parse_applist, parse_field, parse_launch,
    parse_serverinfo, App, LaunchOpts, LaunchReply, ServerInfo, MAX_BODY,
};
use brisk_proto::pair::{unhex, PairError, PairingClient, Rng};
use brisk_proto::sdp::StreamConfig;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
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
    pub cancel_wait: Duration,
    pub cancel_backoff: Duration,
    /// Set once `launch_probe` is about to send `/launch`: the only time a session is ours to cancel.
    pub started: bool,
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
            started: false,
        }
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

    /// `GET /launch` (HTTPS). `cfg.rikey` is scrubbed from recordings: it keys the whole stream.
    /// `cfg.rikey` MUST be freshly random per launch (GCM nonce reuse otherwise); never reuse a `StreamConfig`
    /// across launches. `brisk-probe` draws it from `SystemRng` each run.
    pub async fn launch(
        &mut self,
        ident: &Identity,
        server: &ServerCert,
        app_id: u32,
        cfg: &StreamConfig,
    ) -> crate::Result<LaunchReply> {
        if let Some(rec) = &mut self.recorder {
            rec.add_secret(&cfg.rikey);
        }
        let q = launch_query(
            &ident.unique_id,
            app_id,
            cfg.width,
            cfg.height,
            cfg.fps,
            cfg.rikey,
            cfg.rikeyid,
            &LaunchOpts::default(),
        );
        let r = self
            .get(Some((ident, server)), "/launch", &q, LAUNCH_WAIT)
            .await?;
        Ok(parse_launch(&r)?)
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
        Err(format!(
            "cancel FAILED after {CANCEL_ATTEMPTS} attempts: session may still be running on host; run `brisk-probe cancel --host {}` ({last})",
            self.host
        )
        .into())
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
        let info = self.serverinfo(Some((ident, server))).await?;
        if info.current_game != 0 || info.server_state != "SUNSHINE_SERVER_FREE" {
            let busy = format!(
                "server busy: an app/session is already running (currentgame={}, state={})",
                info.current_game, info.server_state
            );
            if !take_over {
                return Err(
                    format!("{busy}; not cancelling it (pass --take-over to replace it)").into(),
                );
            }
            eprintln!("WARNING: --take-over: {busy}; cancelling it NOW");
            self.cancel_retry(ident, server).await?;
        }
        self.started = true;
        let l = match self.launch(ident, server, app_id, cfg).await {
            Ok(l) => l,
            Err(e) => {
                // A status reply means Sunshine refused: nothing of ours is running.
                let refused = e
                    .downcast_ref::<brisk_proto::http::HttpError>()
                    .is_some_and(|h| matches!(h, brisk_proto::http::HttpError::Status { .. }));
                if refused {
                    let busy = e.to_string().contains("already running");
                    return Err(if busy {
                        format!("server busy: an app/session is already running (currentgame={}); not cancelling it ({e})", info.current_game)
                    } else {
                        format!("launch rejected: {e}; nothing to cancel")
                    }
                    .into());
                }
                // Timeout or transport error: the app may have started, so clean up.
                return Err(match self.cancel_retry(ident, server).await {
                    Ok(()) => e,
                    Err(c) => format!("{e}; {c}").into(),
                });
            }
        };
        let r = handshake(
            &self.host,
            &l.session_url,
            app_version,
            cfg,
            self.recorder.as_mut(),
        )
        .await;
        let c = self.cancel_retry(ident, server).await;
        match (r, c) {
            (Ok(p), Ok(())) => Ok(p),
            (Ok(_), Err(c)) => Err(c),
            (Err(e), Ok(())) => Err(e),
            (Err(e), Err(c)) => Err(format!("{e}; {c}").into()),
        }
    }

    /// Phases 1-4 over HTTP, phase 5 (`pairchallenge`) over HTTPS pinned to the phase-1 cert.
    /// Writes `dir/server.pem` only after phase 5 succeeds. `rng` draws salt, challenge and secret.
    pub async fn pair(
        &mut self,
        dir: &Path,
        ident: &Identity,
        pin: &str,
        rng: Box<dyn Rng>,
    ) -> crate::Result<ServerCert> {
        if let Some(rec) = &mut self.recorder {
            identity_secrets(ident)
                .iter()
                .for_each(|s| rec.add_secret(s)); // client cert travels hex in phase 1
        }
        let drawn = Arc::new(Mutex::new(vec![]));
        let r = self
            .pair_inner(ident, pin, Box::new(Tap(rng, drawn.clone())))
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
            PairError::HashMismatch => {
                "pairing failed: wrong PIN (server secret does not match the PIN-derived hash)"
                    .into()
            }
            e => crate::Error::from(e),
        })?;

        let r = self.get(None, "/pair", &q, WAIT).await?;
        let q = pc
            .on_pairingsecret_resp(&parse_field(&r, "paired")?)
            .map_err(|_| "pairing failed: server rejected our pairing secret")?;

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
                    let name = ServerName::try_from(host.clone())?;
                    let s = tokio_rustls::TlsConnector::from(Arc::new(tls_config(id, srv)?))
                        .connect(name, tcp)
                        .await?;
                    exchange(s, &req).await
                }
            }
        })
        .await
        .map_err(|_| {
            format!(
                "{path}: no reply from {}:{port} within {}s",
                self.host,
                wait.as_secs()
            )
        })?
        .map_err(|e| format!("{path} ({}:{port}): {e}", self.host))?;
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
        body(&raw, tls.is_some()).map_err(|e| format!("{path}: {e}").into())
    }
}

/// Requires `paired == 1` (Sunshine's failure replies carry 0 plus a status message), then returns `name`.
fn paired_field(xml: &str, name: &'static str) -> crate::Result<String> {
    if parse_field(xml, "paired")? != "1" {
        return Err("pairing failed: server answered paired=0".into());
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

/// Copies every rng draw so the recorder can scrub it.
struct Tap(Box<dyn Rng>, Arc<Mutex<Vec<Vec<u8>>>>);
impl Rng for Tap {
    fn fill(&mut self, buf: &mut [u8]) {
        self.0.fill(buf);
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
            // Peer closed TLS without close_notify; `body` still enforces Content-Length.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        if buf.len() > limit {
            return Err("http response too large".into());
        }
    }
    Ok(buf)
}

/// Body of a raw HTTP/1.1 response; enforces Content-Length (required when `need_len`, i.e. over
/// TLS where a close without close_notify is tolerated), rejects chunked.
fn body(raw: &[u8], need_len: bool) -> crate::Result<String> {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("truncated http response (no header end)")?;
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| "non-utf8 http header")?;
    let mut lines = head.split("\r\n");
    let status: u16 = lines
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or("bad http status line")?;
    let mut body = &raw[end + 4..];
    let mut has_len = false;
    for (k, v) in lines.filter_map(|l| l.split_once(':')) {
        match k.trim().to_ascii_lowercase().as_str() {
            "transfer-encoding" => return Err("chunked http response not supported".into()),
            "content-length" => {
                has_len = true;
                let n: usize = v.trim().parse().map_err(|_| "bad content-length")?;
                if body.len() < n {
                    return Err("truncated http response body".into());
                }
                body = &body[..n];
            }
            _ => {}
        }
    }
    if need_len && !has_len {
        return Err("https response without content-length".into());
    }
    let body = String::from_utf8(body.to_vec()).map_err(|_| "non-utf8 http body")?;
    if status != 200 && body.trim().is_empty() {
        return Err(format!("http status {status}").into());
    }
    Ok(body) // non-200 with XML: the parser reports Sunshine's status_message
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls_rustcrypto::provider())
}

fn tls_config(id: &Identity, srv: &ServerCert) -> crate::Result<rustls::ClientConfig> {
    let p = provider();
    let pem_der = |pem: &str| -> crate::Result<CertificateDer<'static>> {
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
    fn body_checks_length_and_status() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello trailing";
        assert_eq!(body(ok, false).unwrap(), "hello");
        assert!(body(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nshort", false).is_err());
        assert!(body(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello",
            false
        )
        .is_err());
        assert!(body(b"HTTP/1.1 404 Not Found\r\n\r\n", false).is_err());
        assert!(body(b"HTTP/1.1 200 OK\r\nContent", false).is_err());
        assert!(body(b"HTTP/1.1 200 OK\r\n\r\nhi", true).is_err());
        assert_eq!(
            body(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi", true).unwrap(),
            "hi"
        );
        assert_eq!(enc("a b/+Z~"), "a%20b%2F%2BZ~");
    }
}
