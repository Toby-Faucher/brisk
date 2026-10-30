//! Pairing, serverinfo and applist against a local fake Sunshine (127.0.0.1, ephemeral ports).
//! The fake mirrors nvhttp.cpp's four pairing phases as pair_vectors.rs does; no real host is contacted.

use brisk_client::identity::{Identity, ServerCert, load_or_create_identity};
use brisk_client::recorder::Recorder;
use brisk_client::rtsp::StreamPorts;
use brisk_client::session::{Session, TakeOver};
use brisk_client::stop::Stop;
use brisk_proto::pair::*;
use brisk_proto::rtsp::{RtspCipher, parse_request};
use brisk_proto::sdp::{Codec, SS_ENC_AUDIO, SS_ENC_CONTROL_V2, StreamConfig};
use rsa::pkcs1v15::{Signature, SigningKey, VerifyingKey};
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
use rsa::signature::{SignatureEncoding, Signer, Verifier};
use rustls::client::danger::HandshakeSignatureValid;
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use x509_cert::Certificate;
use x509_cert::der::{DecodePem, Encode};

mod common;

/// AES-128-ECB of whole blocks, as Sunshine's server side encrypts (own copy, not brisk_proto's).
fn ecb(key: &[u8; 16], data: &[u8]) -> Vec<u8> {
    use aes::cipher::{BlockEncrypt, KeyInit};
    let c = aes::Aes128::new(key.into());
    let mut out = data.to_vec();
    for b in out.chunks_mut(16) {
        c.encrypt_block(b.into());
    }
    out
}

const EMPTY_400: &str = "<<empty-400>>";
/// A `/launch` that never answers; the app already shows as running.
const HOLD: &str = "<<hold>>";
const SERVER_SECRET: [u8; 16] = *b"FAKE-SRV-SECRET!";
const SERVER_CHALLENGE: [u8; 16] = *b"FAKE-SRV-CHALNGE";

/// Two server identities, generated once (RSA keygen is the slow part).
fn server_ident(i: usize) -> &'static Identity {
    static IDS: OnceLock<Vec<Identity>> = OnceLock::new();
    &IDS.get_or_init(|| {
        (0..2)
            .map(|_| load_or_create_identity(tempfile::tempdir().unwrap().path()).unwrap())
            .collect()
    })[i]
}

fn cert_sig(pem: &str) -> Vec<u8> {
    Certificate::from_pem(pem)
        .unwrap()
        .signature
        .raw_bytes()
        .to_vec()
}
fn pem_der(pem: &str) -> Vec<u8> {
    Certificate::from_pem(pem).unwrap().to_der().unwrap()
}

#[derive(Default)]
struct St {
    key: [u8; 16],
    /// rikey of every accepted `/launch`.
    rikeys: Vec<[u8; 16]>,
    client_pem: String,
    clienthash: Vec<u8>,
    authorized: Option<Vec<u8>>,
}

struct Fake {
    pin: String,
    ident: &'static Identity,
    tls_port: u16,
    omit_len: std::sync::atomic::AtomicBool,
    /// Pretend something else is running (`currentgame`/`state`); cleared by a `/cancel`.
    game: std::sync::atomic::AtomicU32,
    launches: AtomicUsize,
    /// RTSP connections accepted so far.
    rtsp_hits: AtomicUsize,
    /// Every `/cancel` request received, and how many of them to swallow without a reply.
    cancel_reqs: AtomicUsize,
    cancel_drop: AtomicUsize,
    st: Mutex<St>,
    rtsp_port: u16,
    /// "ok", "plain" (rtsp:// URL), "launch-busy", "launch-fail", "fail-ANNOUNCE", "tamper-DESCRIBE".
    mode: Mutex<&'static str>,
    rtsp: Mutex<RtspSt>,
}

#[derive(Default)]
struct RtspSt {
    cipher: Option<RtspCipher>,
    /// (method, target, Session header) per request, as the fake decoded it.
    seen: Vec<(String, String, Option<String>)>,
    announced: String,
    cancels: usize,
    /// SETUP server ports [audio, video, control]; Sunshine's defaults when unset. Stream tests
    /// point them at the fake media host so nothing reaches a real Sunshine on 127.0.0.1.
    ports: Option<[u16; 3]>,
}

fn xml(body: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200">{body}</root>"#)
}
const UNAUTHORIZED: &str = r#"<root status_code="401" status_message="The client is not authorized. Certificate verification failed."/>"#;

impl Fake {
    fn handle(&self, path: &str, q: &HashMap<String, String>, peer: Option<Vec<u8>>) -> String {
        let mut st = self.st.lock().unwrap();
        let authed = peer.is_some() && peer == st.authorized;
        let key = st.key;
        let game = self.game.load(Ordering::SeqCst);
        let busy = game != 0;
        match path {
            "/serverinfo" => xml(&format!(
                "<hostname>fake</hostname><appversion>7.1.431.-1</appversion><GfeVersion>3.23.0.74</GfeVersion><uniqueid>FAKE0123</uniqueid><HttpsPort>{}</HttpsPort><ServerCodecModeSupport>259</ServerCodecModeSupport><PairStatus>{}</PairStatus><currentgame>{}</currentgame><state>{}</state>",
                self.tls_port,
                authed as u8,
                game,
                if busy {
                    "SUNSHINE_SERVER_BUSY"
                } else {
                    "SUNSHINE_SERVER_FREE"
                }
            )),
            "/launch" if authed => {
                let mode = *self.mode.lock().unwrap();
                self.launches.fetch_add(1, Ordering::SeqCst);
                if mode == "launch-busy" {
                    return r#"<root status_code="400" status_message="An app is already running on this host"/>"#.into();
                }
                match mode {
                    "launch-empty-400" => {
                        // Worst case: our app id is "running", so a wrongful cancel_if_ours would send /cancel.
                        self.game
                            .store(q["appid"].parse().unwrap(), Ordering::SeqCst);
                        return EMPTY_400.into();
                    }
                    "launch-drop-ours" => self
                        .game
                        .store(q["appid"].parse().unwrap(), Ordering::SeqCst),
                    "launch-drop-other" => self.game.store(77, Ordering::SeqCst),
                    "launch-hold" => {
                        self.game
                            .store(q["appid"].parse().unwrap(), Ordering::SeqCst);
                        return HOLD.into();
                    }
                    _ => {}
                }
                if mode.starts_with("launch-drop") {
                    return String::new();
                }
                if mode == "launch-fail" {
                    return r#"<root status_code="503" status_message="fake launch failure"/>"#
                        .into();
                }
                let key: [u8; 16] = unhex(&q["rikey"]).unwrap().try_into().unwrap();
                st.rikeys.push(key);
                let enc = q["corever"] == "1" && mode != "plain";
                self.rtsp.lock().unwrap().cipher = enc.then(|| RtspCipher::host(key));
                let scheme = if enc { "rtspenc" } else { "rtsp" };
                xml(&format!(
                    "<sessionUrl0>{scheme}://127.0.0.1:{}</sessionUrl0><gamesession>1</gamesession>",
                    self.rtsp_port
                ))
            }
            "/cancel" if authed => {
                self.cancel_reqs.fetch_add(1, Ordering::SeqCst);
                if self.cancel_drop.load(Ordering::SeqCst) > 0 {
                    self.cancel_drop.fetch_sub(1, Ordering::SeqCst);
                    return String::new(); // serve_conn closes without replying
                }
                self.game.store(0, Ordering::SeqCst);
                self.rtsp.lock().unwrap().cancels += 1;
                xml("<cancel>1</cancel>")
            }
            "/applist" if authed => xml(
                "<App><AppTitle>Desktop</AppTitle><ID>1</ID></App><App><AppTitle>Steam Big Picture</AppTitle><ID>2</ID></App>",
            ),
            "/pair" if q.get("phrase").map(String::as_str) == Some("getservercert") => {
                let salt: [u8; 16] = unhex(&q["salt"]).unwrap().try_into().unwrap();
                st.key = derive_aes_key(salt, &self.pin).aes_key;
                st.client_pem = String::from_utf8(unhex(&q["clientcert"]).unwrap()).unwrap();
                xml(&format!(
                    "<paired>1</paired><plaincert>{}</plaincert>",
                    hex(self.ident.cert_pem.as_bytes())
                ))
            }
            "/pair" if q.get("phrase").map(String::as_str) == Some("pairchallenge") => {
                if authed {
                    xml("<paired>1</paired>")
                } else {
                    UNAUTHORIZED.into()
                }
            }
            "/pair" if q.contains_key("clientchallenge") => {
                let challenge =
                    aes_ecb_decrypt(&key, &unhex(&q["clientchallenge"]).unwrap()).unwrap();
                let h = Sha256::new()
                    .chain_update(&challenge)
                    .chain_update(cert_sig(&self.ident.cert_pem))
                    .chain_update(SERVER_SECRET)
                    .finalize();
                let resp = ecb(&key, &[h.as_slice(), &SERVER_CHALLENGE].concat());
                xml(&format!(
                    "<paired>1</paired><challengeresponse>{}</challengeresponse>",
                    hex(&resp)
                ))
            }
            "/pair" if q.contains_key("serverchallengeresp") => {
                st.clienthash =
                    aes_ecb_decrypt(&key, &unhex(&q["serverchallengeresp"]).unwrap()).unwrap();
                let sk = SigningKey::<Sha256>::new(
                    rsa::RsaPrivateKey::from_pkcs8_pem(&self.ident.key_pem).unwrap(),
                );
                let ps = [&SERVER_SECRET[..], &sk.sign(&SERVER_SECRET).to_vec()].concat();
                xml(&format!(
                    "<paired>1</paired><pairingsecret>{}</pairingsecret>",
                    hex(&ps)
                ))
            }
            "/pair" if q.contains_key("clientpairingsecret") => {
                let cps = unhex(&q["clientpairingsecret"]).unwrap();
                let (secret, sig) = cps.split_at(16);
                let h = Sha256::new()
                    .chain_update(SERVER_CHALLENGE)
                    .chain_update(cert_sig(&st.client_pem))
                    .chain_update(secret)
                    .finalize();
                let c = Certificate::from_pem(&st.client_pem).unwrap();
                let pk = rsa::RsaPublicKey::from_public_key_der(
                    &c.tbs_certificate.subject_public_key_info.to_der().unwrap(),
                )
                .unwrap();
                let ok = h.as_slice() == st.clienthash
                    && VerifyingKey::<Sha256>::new(pk)
                        .verify(secret, &Signature::try_from(sig).unwrap())
                        .is_ok();
                if ok {
                    st.authorized = Some(pem_der(&st.client_pem));
                }
                xml(&format!("<paired>{}</paired>", ok as u8))
            }
            _ => UNAUTHORIZED.into(),
        }
    }
}

fn unpercent(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

async fn serve_conn<S: AsyncRead + AsyncWrite + Unpin>(
    mut s: S,
    fake: &Fake,
    peer: Option<Vec<u8>>,
) {
    let mut buf = Vec::new();
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let mut chunk = [0u8; 4096];
        let n = s.read(&mut chunk).await.unwrap();
        assert!(n > 0, "client closed early");
        buf.extend_from_slice(&chunk[..n]);
    }
    let line = String::from_utf8_lossy(&buf)
        .lines()
        .next()
        .unwrap()
        .to_string();
    let target = line.split(' ').nth(1).unwrap();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let q = query.split('&').filter(|kv| !kv.is_empty()).map(|kv| {
        let (k, v) = kv.split_once('=').unwrap();
        (unpercent(k), unpercent(v))
    });
    let body = fake.handle(path, &q.collect(), peer.clone());
    if body.is_empty() {
        return;
    }
    let len = if peer.is_some() && fake.omit_len.load(std::sync::atomic::Ordering::SeqCst) {
        String::new()
    } else {
        format!("Content-Length: {}\r\n", body.len())
    };
    if body == HOLD {
        std::future::pending::<()>().await;
    }
    if body == EMPTY_400 {
        s.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        return;
    }
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\n{len}Connection: close\r\n\r\n{body}"
    );
    s.write_all(resp.as_bytes()).await.unwrap();
    s.flush().await.unwrap();
    // Dropped without close_notify on TLS, to exercise the client's Content-Length check.
}

/// Keys `cmd_announce` (rtsp.cpp) reads with `args.at` and no default: missing ones answer 400.
const SUNSHINE_REQUIRED: [&str; 10] = [
    "x-nv-audio.surround.numChannels",
    "x-nv-audio.surround.channelMask",
    "x-nv-audio.surround.AudioQuality",
    "x-nv-video[0].packetSize",
    "x-nv-video[0].clientViewportHt",
    "x-nv-video[0].clientViewportWd",
    "x-nv-video[0].maxFPS",
    "x-nv-vqos[0].bw.maximumBitrateKbps",
    "x-nv-video[0].videoEncoderSlicesPerFrame",
    "x-nv-video[0].maxNumReferenceFrames",
];

/// One RTSP request per connection, as rtsp.cpp: encrypted = 24-byte header then payload, bounded by
/// the 2048-byte buffer; plain = header to CRLFCRLF plus `Content-length`. Reply, then close.
async fn serve_rtsp(mut s: TcpStream, f: &Fake) {
    f.rtsp_hits.fetch_add(1, Ordering::SeqCst);
    let hold = *f.mode.lock().unwrap() == "rtsp-hold";
    if hold {
        std::future::pending::<()>().await; // never answers, so the handshake stalls
    }
    let enc = f.rtsp.lock().unwrap().cipher.is_some();
    let plain = if enc {
        let mut h = [0u8; 24];
        s.read_exact(&mut h).await.unwrap();
        let tl = u32::from_be_bytes(h[..4].try_into().unwrap());
        assert!(tl & 0x8000_0000 != 0, "Rejecting unencrypted RTSP message");
        let len = (tl & 0x7fff_ffff) as usize;
        assert!(24 + len < 2048, "Exceeded maximum rtsp packet size");
        let mut frame = h.to_vec();
        frame.resize(24 + len, 0);
        s.read_exact(&mut frame[24..]).await.unwrap();
        let mut st = f.rtsp.lock().unwrap();
        st.cipher
            .as_mut()
            .unwrap()
            .open(&frame)
            .expect("client frame")
    } else {
        let mut buf = vec![];
        loop {
            let mut c = [0u8; 2048];
            let n = s.read(&mut c).await.unwrap();
            assert!(n > 0, "client closed early");
            buf.extend_from_slice(&c[..n]);
            if let Ok(r) = parse_request(&buf) {
                let want: usize = r.header("Content-length").map_or(0, |v| v.parse().unwrap());
                if r.body.len() >= want {
                    break;
                }
            }
        }
        buf
    };
    let req = parse_request(&plain).unwrap();
    let mode = *f.mode.lock().unwrap();
    let mut extra = String::new();
    let mut body = String::new();
    let status = match req.method.as_str() {
        _ if mode == format!("fail-{}", req.method) => "400 BAD REQUEST",
        "OPTIONS" | "PLAY" => "200 OK",
        "DESCRIBE" => {
            body = "a=x-ss-general.featureFlags:0\na=x-ss-general.encryptionSupported:5\na=x-ss-general.encryptionRequested:1\na=fmtp:97 surround-params=21101\n".into();
            "200 OK"
        }
        "SETUP" => {
            let ty = req
                .target
                .split('=')
                .nth(1)
                .unwrap_or("")
                .split('/')
                .next()
                .unwrap();
            let p = f
                .rtsp
                .lock()
                .unwrap()
                .ports
                .unwrap_or([48000, 47998, 47999]);
            let port = match ty {
                "audio" => p[0],
                "video" => p[1],
                "control" => p[2],
                _ => 0,
            };
            if port == 0 {
                "404 NOT FOUND"
            } else {
                let id = if ty == "control" {
                    "X-SS-Connect-Data: 305419896".to_string()
                } else {
                    "X-SS-Ping-Payload: 0123456789ABCDEF".to_string()
                };
                extra = format!(
                    "Session: DEADBEEFCAFE;timeout = 90\r\nTransport: server_port={port}\r\n{id}\r\n"
                );
                "200 OK"
            }
        }
        "ANNOUNCE" => {
            let sdp = String::from_utf8(req.body.clone()).unwrap();
            let keys: Vec<&str> = sdp
                .split(['\r', '\n'])
                .filter_map(|l| l.strip_prefix("a="))
                .map(|l| l.split(':').next().unwrap())
                .collect();
            f.rtsp.lock().unwrap().announced = sdp.clone();
            if SUNSHINE_REQUIRED.iter().all(|k| keys.contains(k)) {
                "200 OK"
            } else {
                "400 BAD REQUEST"
            }
        }
        _ => "404 NOT FOUND",
    };
    let resp = format!(
        "RTSP/1.0 {status}\r\nCSeq: {}\r\n{extra}\r\n{body}",
        req.header("CSeq").unwrap()
    );
    let mut wire = {
        let mut st = f.rtsp.lock().unwrap();
        st.seen.push((
            req.method.clone(),
            req.target.clone(),
            req.header("Session").map(str::to_string),
        ));
        match &mut st.cipher {
            Some(c) => c.seal(resp.as_bytes()).unwrap(),
            None => resp.into_bytes(),
        }
    };
    if mode == format!("tamper-{}", req.method) {
        *wire.last_mut().unwrap() ^= 1;
    }
    s.write_all(&wire).await.unwrap();
    s.shutdown().await.unwrap();
}

#[derive(Debug)]
struct AnyClient(WebPkiSupportedAlgorithms);
impl ClientCertVerifier for AnyClient {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

/// Starts the fake (HTTP + HTTPS) and returns a Session pointing at its HTTP port.
async fn start(pin: &str, which: usize) -> (Session, Arc<Fake>) {
    let ident = server_ident(which);
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let https = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = http.local_addr().unwrap().port();
    let rtsp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fake = Arc::new(Fake {
        rtsp_port: rtsp.local_addr().unwrap().port(),
        mode: Mutex::new("ok"),
        rtsp: Mutex::default(),
        pin: pin.into(),
        ident,
        tls_port: https.local_addr().unwrap().port(),
        omit_len: Default::default(),
        game: Default::default(),
        launches: Default::default(),
        rtsp_hits: Default::default(),
        cancel_reqs: Default::default(),
        cancel_drop: Default::default(),
        st: Mutex::default(),
    });
    let provider = Arc::new(rustls_rustcrypto::provider());
    let key = rsa::pkcs8::SecretDocument::from_pkcs8_pem(&ident.key_pem).unwrap();
    let cfg = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(Arc::new(AnyClient(
            provider.signature_verification_algorithms,
        )))
        .with_single_cert(
            vec![pem_der(&ident.cert_pem).into()],
            PrivateKeyDer::Pkcs8(key.as_bytes().to_vec().into()),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
    let f = fake.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = http.accept().await.unwrap();
            serve_conn(s, &f, None).await;
        }
    });
    let f = fake.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = https.accept().await.unwrap();
            let Ok(tls) = acceptor.accept(s).await else {
                continue;
            };
            let peer = tls.get_ref().1.peer_certificates().map(|c| c[0].to_vec());
            // Own task: a held launch must not block the cancel that follows it.
            let f = f.clone();
            tokio::spawn(async move { serve_conn(tls, &f, peer).await });
        }
    });
    let f = fake.clone();
    tokio::spawn(async move {
        loop {
            let (s, _) = rtsp.accept().await.unwrap();
            serve_rtsp(s, &f).await;
        }
    });
    let mut s = Session::new("127.0.0.1");
    s.http_port = http_port;
    (s, fake)
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn pair_then_applist_over_pinned_https() {
    let t = tempfile::tempdir().unwrap();
    let ident = load_or_create_identity(t.path()).unwrap();
    let (mut s, fake) = start("4821", 0).await;
    let server = s.pair(t.path(), &ident, "4821").await.unwrap();
    assert_eq!(server.pem, fake.ident.cert_pem);
    assert_eq!(ServerCert::load(t.path()).unwrap(), server);
    assert_eq!(
        names(t.path()),
        ["client.key", "client.pem", "server.pem", "uniqueid"]
    );

    let mut s2 = Session::new("127.0.0.1");
    s2.http_port = s.http_port;
    let info = s2.serverinfo(None).await.unwrap();
    assert!(!info.paired);
    assert_eq!(s2.https_port, fake.tls_port);
    assert!(s2.serverinfo(Some((&ident, &server))).await.unwrap().paired);
    let apps = s2.applist(&ident, &server).await.unwrap();
    assert_eq!(
        apps.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(),
        ["Desktop", "Steam Big Picture"]
    );
}

#[tokio::test]
async fn https_response_without_content_length_is_an_error() {
    let t = tempfile::tempdir().unwrap();
    let ident = load_or_create_identity(t.path()).unwrap();
    let (mut s, fake) = start("4821", 0).await;
    let server = s.pair(t.path(), &ident, "4821").await.unwrap();
    s.serverinfo(None).await.unwrap();
    fake.omit_len
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let err = s.applist(&ident, &server).await.unwrap_err();
    assert!(err.to_string().contains("content-length"), "{err}");
}

/// The pinned verifier must still check the handshake signature: a server that presents the
/// pinned cert but signs with another key (so the cert bytes match) has to be refused.
#[tokio::test]
async fn pinned_verifier_rejects_bad_handshake_signature() {
    let t = tempfile::tempdir().unwrap();
    let ident = load_or_create_identity(t.path()).unwrap();
    let (mut s, fake) = start("4821", 0).await;
    let pinned = ServerCert {
        pem: fake.ident.cert_pem.clone(),
    };
    s.serverinfo(None).await.unwrap();
    s.serverinfo(Some((&ident, &pinned))).await.unwrap(); // control: honest server passes

    let provider = Arc::new(rustls_rustcrypto::provider());
    let other_key = rsa::pkcs8::SecretDocument::from_pkcs8_pem(&server_ident(1).key_pem).unwrap();
    let cfg = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(Arc::new(AnyClient(
            provider.signature_verification_algorithms,
        )))
        .with_single_cert(
            vec![pem_der(&fake.ident.cert_pem).into()],
            PrivateKeyDer::Pkcs8(other_key.as_bytes().to_vec().into()),
        );
    let Ok(cfg) = cfg else {
        return; // rustls refused the mismatched key/cert pair outright, nothing to test
    };
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    s.https_port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (c, _) = l.accept().await.unwrap();
        let _ = acceptor.accept(c).await;
    });
    assert!(s.serverinfo(Some((&ident, &pinned))).await.is_err());
}

#[tokio::test]
async fn wrong_pin_fails_and_writes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let ident = load_or_create_identity(t.path()).unwrap();
    let before = names(t.path());
    let (mut s, fake) = start("1111", 0).await;
    let err = s.pair(t.path(), &ident, "2222").await.unwrap_err();
    assert!(err.to_string().contains("wrong PIN"), "{err}");
    assert_eq!(names(t.path()), before, "no server.pem, no temp files");
    assert!(fake.st.lock().unwrap().authorized.is_none());
}

#[tokio::test]
async fn pinned_cert_rejects_a_different_server() {
    let t = tempfile::tempdir().unwrap();
    let ident = load_or_create_identity(t.path()).unwrap();
    let (mut a, _) = start("4821", 0).await;
    let pinned_a = a.pair(t.path(), &ident, "4821").await.unwrap();
    // Server B presents its own cert; even an otherwise valid TLS handshake must fail.
    let (mut b, fb) = start("4821", 1).await;
    b.https_port = fb.tls_port;
    let err = b.serverinfo(Some((&ident, &pinned_a))).await.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("certificate"),
        "{err}"
    );
    assert!(b.applist(&ident, &pinned_a).await.is_err());
    // Sanity: B is reachable with its own pin.
    let pinned_b = ServerCert {
        pem: fb.ident.cert_pem.clone(),
    };
    assert!(
        !b.serverinfo(Some((&ident, &pinned_b)))
            .await
            .unwrap()
            .paired
    );
}

/// Deterministic, recognisable rng output so the test knows every value it must not find.
struct Planted(Arc<Mutex<Vec<Vec<u8>>>>);
impl Rng for Planted {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut v = self.0.lock().unwrap();
        let n = v.len() as u8;
        buf.iter_mut()
            .enumerate()
            .for_each(|(i, b)| *b = 0xA0 ^ n.wrapping_mul(31) ^ (i as u8).wrapping_mul(7));
        v.push(buf.to_vec());
    }
}

#[tokio::test]
async fn recorder_output_has_no_planted_secret() {
    let t = tempfile::tempdir().unwrap();
    let rec_dir = t.path().join("rec");
    let ident = load_or_create_identity(&t.path().join("id")).unwrap();
    let pin = "4821";
    let (mut s, fake) = start(pin, 0).await;
    s.recorder = Some(Recorder::new(&rec_dir));
    let drawn = Arc::new(Mutex::new(Vec::new()));
    s.set_rng(Planted(drawn.clone()));
    let server = s.pair(&t.path().join("id"), &ident, pin).await.unwrap();
    s.applist(&ident, &server).await.unwrap();
    let mut rec = s.recorder.take().unwrap();
    rec.set_pin(pin);
    rec.finish().unwrap();

    let key_der = rsa::pkcs8::SecretDocument::from_pkcs8_pem(&ident.key_pem)
        .unwrap()
        .as_bytes()
        .to_vec();
    let mut planted: Vec<Vec<u8>> = drawn.lock().unwrap().clone(); // salt, client challenge, client secret
    assert_eq!(planted.len(), 3);
    planted.extend([
        SERVER_SECRET.to_vec(),
        SERVER_CHALLENGE.to_vec(),
        key_der,
        ident.key_pem.clone().into(),
        ident.cert_pem.clone().into(),
        fake.ident.cert_pem.clone().into(),
    ]);
    let mut needles: Vec<Vec<u8>> = vec![pin.into()];
    for p in &planted {
        needles.push(p.clone());
        needles.push(hex(p).into_bytes());
        needles.push(hex(p).to_lowercase().into_bytes());
    }
    let files = names(&rec_dir);
    assert!(
        files.iter().any(|f| f.ends_with("pair-req.bin"))
            && files.iter().any(|f| f.ends_with("applist-resp.meta.json")),
        "{files:?}"
    );
    for f in &files {
        let data = std::fs::read(rec_dir.join(f)).unwrap();
        assert!(!data.is_empty(), "{f}");
        for n in &needles {
            assert!(
                !data.windows(n.len()).any(|w| w == n.as_slice()),
                "{f} leaks {}",
                String::from_utf8_lossy(&n[..n.len().min(24)])
            );
        }
    }
    // The recording still carries the protocol: app titles survive scrubbing.
    let applist = files
        .iter()
        .find(|f| f.ends_with("applist-resp.bin"))
        .unwrap();
    assert!(
        String::from_utf8_lossy(&std::fs::read(rec_dir.join(applist)).unwrap())
            .contains("Steam Big Picture")
    );
}

fn stream_cfg() -> StreamConfig {
    StreamConfig {
        width: 1920,
        height: 1080,
        fps: 60,
        bitrate_kbps: 20000,
        codec: Codec::H264,
        packet_size: 1392,
        fec_percent: 20,
        tap: false,
        enc_flags: 0,
    }
}

/// Paired session against a fake in `mode`; returns what `launch_probe` needs.
async fn paired(
    mode: &'static str,
) -> (Session, Arc<Fake>, Identity, ServerCert, tempfile::TempDir) {
    let t = tempfile::tempdir().unwrap();
    let ident = load_or_create_identity(t.path()).unwrap();
    let (mut s, fake) = start("4821", 0).await;
    let server = s.pair(t.path(), &ident, "4821").await.unwrap();
    *fake.mode.lock().unwrap() = mode;
    (s, fake, ident, server, t)
}

const RTSP_SEQUENCE: [(&str, &str); 7] = [
    ("OPTIONS", "URL"),
    ("DESCRIBE", "URL"),
    ("SETUP", "streamid=audio/0/0"),
    ("SETUP", "streamid=video/0/0"),
    ("SETUP", "streamid=control/13/0"),
    ("ANNOUNCE", "streamid=control/13/0"),
    ("PLAY", "/"),
];

async fn launch_ok(mode: &'static str, scheme: &str) {
    let (mut s, fake, ident, server, _t) = paired(mode).await;
    let ports = s
        .launch_probe(
            &ident,
            &server,
            1,
            "7.1.431.-1",
            &stream_cfg(),
            TakeOver::Refuse,
        )
        .await
        .unwrap();
    assert_eq!(
        (ports.audio, ports.video, ports.control),
        (48000, 47998, 47999)
    );
    assert_eq!(ports.session_id, "DEADBEEFCAFE");
    assert_eq!(ports.ping_payload.as_deref(), Some("0123456789ABCDEF"));
    assert_eq!(ports.connect_data, Some(0x12345678));
    assert_eq!(ports.enc_flags, SS_ENC_CONTROL_V2); // supported 5, requested 1, wanted none
    let st = fake.rtsp.lock().unwrap();
    assert_eq!(st.cancels, 1);
    let url = format!("{scheme}://127.0.0.1:{}", fake.rtsp_port);
    let got: Vec<_> = st
        .seen
        .iter()
        .map(|(m, t, _)| (m.as_str(), t.as_str()))
        .collect();
    let want: Vec<_> = RTSP_SEQUENCE
        .iter()
        .map(|&(m, t)| (m, if t == "URL" { url.as_str() } else { t }))
        .collect();
    assert_eq!(got, want);
    // Session from the audio SETUP is echoed on every later request.
    assert!(
        st.seen[3..]
            .iter()
            .all(|(_, _, s)| s.as_deref() == Some("DEADBEEFCAFE"))
    );
    assert!(
        st.announced
            .contains("a=x-ss-general.encryptionEnabled:1 \r\n")
    );
}

/// Fake decrypts every request with the rikey from the launch query; all 7 requests got through.
#[tokio::test]
async fn launch_encrypted_rtsp_handshake_then_cancel() {
    launch_ok("ok", "rtspenc").await;
}

#[tokio::test]
async fn launch_plain_rtsp_handshake_then_cancel() {
    launch_ok("plain", "rtsp").await;
}

#[tokio::test]
async fn failures_still_cancel() {
    for (mode, needle) in [
        ("fail-ANNOUNCE", "ANNOUNCE"),
        ("tamper-DESCRIBE", "authentication"),
    ] {
        let (mut s, fake, ident, server, _t) = paired(mode).await;
        let err = s
            .launch_probe(
                &ident,
                &server,
                1,
                "7.1.431.-1",
                &stream_cfg(),
                TakeOver::Refuse,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains(needle), "{mode}: {err}");
        assert_eq!(fake.rtsp.lock().unwrap().cancels, 1, "{mode}");
    }
}

#[tokio::test]
async fn launch_recording_scrubs_rikey() {
    let (mut s, _fake, ident, server, t) = paired("ok").await;
    let dir = t.path().join("rec");
    s.recorder = Some(Recorder::new(&dir));
    let key = s
        .launch_probe(
            &ident,
            &server,
            1,
            "7.1.431.-1",
            &stream_cfg(),
            TakeOver::Refuse,
        )
        .await
        .unwrap()
        .key;
    s.recorder.take().unwrap().finish().unwrap();
    let files = names(&dir);
    assert!(
        files.iter().any(|f| f.ends_with("rtsp-describe-resp.bin")),
        "{files:?}"
    );
    for f in &files {
        let d = std::fs::read(dir.join(f)).unwrap();
        for n in [key.rikey.to_vec(), hex(&key.rikey).into_bytes()] {
            assert!(
                !d.windows(n.len()).any(|w| w == n.as_slice()),
                "{f} leaks rikey"
            );
        }
    }
}

/// The same `StreamConfig` launched twice gets two different keys, each the one sent to the
/// host (GCM nonce reuse otherwise: the RTSP/control IVs restart every launch).
#[tokio::test]
async fn relaunch_with_same_config_draws_a_fresh_rikey() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    let cfg = stream_cfg();
    let a = s
        .launch_probe(&ident, &server, 1, "7.1.431.-1", &cfg, TakeOver::Refuse)
        .await
        .unwrap()
        .key;
    let b = s
        .launch_probe(&ident, &server, 1, "7.1.431.-1", &cfg, TakeOver::Refuse)
        .await
        .unwrap()
        .key;
    assert_ne!(a.rikey, b.rikey);
    assert_eq!(fake.st.lock().unwrap().rikeys, [a.rikey, b.rikey]);
    assert!(
        a.rikeyid < 1 << 31 && b.rikeyid < 1 << 31,
        "rikeyid must stay positive"
    );
    assert_eq!(
        format!("{a:?}"),
        format!("LaunchKey {{ rikey: <redacted>, rikeyid: {} }}", a.rikeyid)
    );
}

/// Repeats one 16-byte pattern: the launch key becomes `RIKEY_FIXED` / `RIKEYID_FIXED`.
struct Fixed;
impl Rng for Fixed {
    fn fill(&mut self, buf: &mut [u8]) {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = RIKEY_FIXED[i % 16];
        }
    }
}
const RIKEY_FIXED: [u8; 16] = *b"RIKEY-0123456789";
/// LE u32 of "RIKE", already positive.
const RIKEYID_FIXED: u32 = 0x454b_4952;

fn fast(s: &mut Session) {
    s.cancel_wait = std::time::Duration::from_secs(2);
    s.cancel_backoff = std::time::Duration::from_millis(10);
}

async fn probe(
    s: &mut Session,
    i: &Identity,
    c: &ServerCert,
    take_over: TakeOver,
) -> brisk_client::Result<brisk_client::rtsp::StreamPorts> {
    s.launch_probe(i, c, 1, "7.1.431.-1", &stream_cfg(), take_over)
        .await
}

#[tokio::test]
async fn rejected_launch_never_cancels() {
    for (mode, needle, busy) in [
        ("launch-busy", "server busy", true),
        ("launch-empty-400", "server busy", true),
        ("launch-fail", "fake launch failure", false),
    ] {
        let (mut s, fake, ident, server, _t) = paired(mode).await;
        let e = probe(&mut s, &ident, &server, TakeOver::Refuse)
            .await
            .unwrap_err();
        // Typed: a busy host is `Busy`, another refusal is a `Session` error.
        assert_eq!(
            matches!(e, brisk_client::Error::Busy(_)),
            busy,
            "{mode}: {e:?}"
        );
        assert_eq!(
            matches!(e, brisk_client::Error::Session(_)),
            !busy,
            "{mode}: {e:?}"
        );
        let err = e.to_string();
        assert!(err.contains(needle), "{mode}: {err}");
        assert!(!err.contains("outcome unknown"), "{mode}: {err}");
        assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 0, "{mode}");
    }
}

#[tokio::test]
async fn busy_host_refused_without_launch_unless_take_over() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    fake.game.store(5, Ordering::SeqCst);
    let err = probe(&mut s, &ident, &server, TakeOver::Refuse)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("server busy") && err.contains("currentgame=5"),
        "{err}"
    );
    assert_eq!(fake.launches.load(Ordering::SeqCst), 0);
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 0);
    assert!(!s.launch_sent());
    probe(&mut s, &ident, &server, TakeOver::Replace)
        .await
        .unwrap();
    assert_eq!(fake.launches.load(Ordering::SeqCst), 1);
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 2); // take-over cancel, then our own
}

#[tokio::test]
async fn cancel_retries_after_a_lost_reply() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    fast(&mut s);
    fake.cancel_drop.store(1, Ordering::SeqCst);
    probe(&mut s, &ident, &server, TakeOver::Refuse)
        .await
        .unwrap();
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 2);
    assert_eq!(fake.rtsp.lock().unwrap().cancels, 1);
}

#[tokio::test]
async fn cancel_failing_three_times_is_a_loud_error() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    fast(&mut s);
    fake.cancel_drop.store(3, Ordering::SeqCst);
    let err = probe(&mut s, &ident, &server, TakeOver::Refuse)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("cancel FAILED after 3 attempts")
            && err.contains("may still be running")
            && err.contains("brisk-probe cancel --host 127.0.0.1"),
        "{err}"
    );
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 3);
}

/// What `brisk-probe cancel` does: retrying cancel, then serverinfo shows the host free.
#[tokio::test]
async fn cancel_subcommand_path_frees_host() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    fast(&mut s);
    fake.game.store(5, Ordering::SeqCst);
    fake.cancel_drop.store(1, Ordering::SeqCst);
    s.cancel_retry(&ident, &server).await.unwrap();
    let i = s.serverinfo(Some((&ident, &server))).await.unwrap();
    assert_eq!(
        (i.current_game, i.server_state.as_str()),
        (0, "SUNSHINE_SERVER_FREE")
    );
}

#[tokio::test]
async fn dropped_launch_cancels_only_if_host_runs_our_app() {
    for (mode, cancels, needle) in [
        ("launch-drop-ours", 1, "session cancelled"),
        ("launch-drop-other", 0, "launch outcome unknown"),
        ("launch-drop", 0, "nothing cancelled"),
    ] {
        let (mut s, fake, ident, server, _t) = paired(mode).await;
        fast(&mut s);
        let err = probe(&mut s, &ident, &server, TakeOver::Refuse)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(needle), "{mode}: {err}");
        assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), cancels, "{mode}");
    }
}

/// The control stream runs inside `launch_session`: after PLAY, before `/cancel`; its failure still cancels.
#[tokio::test]
async fn launch_session_runs_during_before_cancel() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    let f = fake.clone();
    let got = s
        .launch_session(
            &ident,
            &server,
            1,
            "7.1.431.-1",
            &stream_cfg(),
            TakeOver::Refuse,
            |p| async move {
                let st = f.rtsp.lock().unwrap();
                assert_eq!(st.cancels, 0, "not cancelled yet");
                assert_eq!(st.seen.last().unwrap().0, "PLAY");
                Ok(p.connect_data)
            },
        )
        .await
        .unwrap();
    assert_eq!(got, Some(0x12345678));
    assert_eq!(fake.rtsp.lock().unwrap().cancels, 1);

    let (mut s, fake, ident, server, _t) = paired("ok").await;
    let err = s
        .launch_session(
            &ident,
            &server,
            1,
            "7.1.431.-1",
            &stream_cfg(),
            TakeOver::Refuse,
            |_| async { Err::<(), _>(brisk_client::Error::Session("control failed".into())) },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("control failed"));
    assert_eq!(fake.rtsp.lock().unwrap().cancels, 1);

    // A panic inside the launched section still cancels, and comes back as an error.
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    let err = s
        .launch_session(
            &ident,
            &server,
            1,
            "7.1.431.-1",
            &stream_cfg(),
            TakeOver::Refuse,
            |_| async {
                tokio::task::yield_now().await;
                panic!("boom in during");
                #[allow(unreachable_code)]
                Ok(())
            },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("panicked"), "{err}");
    assert_eq!(fake.rtsp.lock().unwrap().cancels, 1);
}

async fn session_until<T, Fut>(
    s: &mut Session,
    stop: &mut Stop,
    ident: &Identity,
    server: &ServerCert,
    during: impl FnOnce(StreamPorts, Stop) -> Fut,
) -> brisk_client::Result<T>
where
    Fut: std::future::Future<Output = brisk_client::Result<T>>,
{
    let cfg = stream_cfg();
    let call = s.launch_session_until(
        stop,
        ident,
        server,
        1,
        "7.1.431.-1",
        &cfg,
        TakeOver::Refuse,
        during,
    );
    tokio::time::timeout(std::time::Duration::from_secs(20), call)
        .await
        .expect("returns without waiting for the host")
}

/// Polls until `cond` holds. The calls above are bounded by their own timeout.
async fn wait_for(cond: impl Fn() -> bool) {
    while !cond() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

fn never(_: StreamPorts, _: Stop) -> std::future::Ready<brisk_client::Result<()>> {
    unreachable!("the stop came before `during`")
}

#[tokio::test]
async fn without_a_stop_launch_session_until_is_launch_session() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    let (mut stop, _handle) = Stop::manual();
    let got = session_until(&mut s, &mut stop, &ident, &server, |p, _| async move {
        Ok(p.connect_data)
    })
    .await
    .unwrap();
    assert_eq!(got, Some(0x12345678));
    assert_eq!(fake.rtsp.lock().unwrap().cancels, 1);
}

#[tokio::test]
async fn stop_before_launch_sends_nothing() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    let (mut stop, handle) = Stop::manual();
    handle.request("SIGTERM");
    let err = session_until(&mut s, &mut stop, &ident, &server, never)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("before launch; nothing cancelled"),
        "{err}"
    );
    assert_eq!(fake.launches.load(Ordering::SeqCst), 0);
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 0);
}

/// `/launch` has no answer yet: the app is cancelled because the host runs it.
#[tokio::test]
async fn stop_while_launch_is_pending_cancels_our_app() {
    let (mut s, fake, ident, server, _t) = paired("launch-hold").await;
    let (mut stop, handle) = Stop::manual();
    let driver = async {
        wait_for(|| fake.launches.load(Ordering::SeqCst) == 1).await;
        handle.request("SIGTERM");
    };
    let (got, ()) = tokio::join!(
        session_until(&mut s, &mut stop, &ident, &server, never),
        driver
    );
    let err = got.unwrap_err().to_string();
    assert!(
        err.contains("interrupted (SIGTERM); session cancelled"),
        "{err}"
    );
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 1);
    assert_eq!(fake.game.load(Ordering::SeqCst), 0, "host is free again");
}

/// The RTSP handshake stalls after a confirmed launch: the session is cancelled.
#[tokio::test]
async fn stop_during_the_rtsp_handshake_cancels_the_session() {
    let (mut s, fake, ident, server, _t) = paired("rtsp-hold").await;
    let (mut stop, handle) = Stop::manual();
    let driver = async {
        wait_for(|| fake.rtsp_hits.load(Ordering::SeqCst) == 1).await;
        handle.request("SIGINT");
    };
    let (got, ()) = tokio::join!(
        session_until(&mut s, &mut stop, &ident, &server, never),
        driver
    );
    let err = got.unwrap_err().to_string();
    assert!(
        err.contains("interrupted (SIGINT); session cancelled"),
        "{err}"
    );
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 1);
}

/// Once `during` runs it owns the stop: it winds down, then the usual cancel runs.
#[tokio::test]
async fn stop_inside_during_lets_it_wind_down_then_cancels() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    let (mut stop, handle) = Stop::manual();
    let driver = async {
        wait_for(|| {
            let st = fake.rtsp.lock().unwrap();
            st.seen.iter().any(|r| r.0 == "PLAY")
        })
        .await;
        handle.request("SIGHUP");
    };
    let during = |_, mut stop: Stop| async move {
        let ended = stop.unless(std::future::pending::<()>()).await;
        assert!(ended.is_none());
        Err::<(), _>(brisk_client::Error::Session("wound down".into()))
    };
    let (got, ()) = tokio::join!(
        session_until(&mut s, &mut stop, &ident, &server, during),
        driver
    );
    assert_eq!(got.unwrap_err().to_string(), "wound down");
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 1);
}

/// The cancel keeps failing and would back off for an hour; a second stop does not wait for it.
#[tokio::test]
async fn second_stop_returns_without_waiting_for_the_cancel() {
    let (mut s, fake, ident, server, _t) = paired("rtsp-hold").await;
    s.cancel_wait = std::time::Duration::from_secs(2);
    s.cancel_backoff = std::time::Duration::from_secs(3600);
    fake.cancel_drop.store(3, Ordering::SeqCst);
    let (mut stop, handle) = Stop::manual();
    let driver = async {
        wait_for(|| fake.rtsp_hits.load(Ordering::SeqCst) == 1).await;
        handle.request("SIGTERM");
        wait_for(|| fake.cancel_reqs.load(Ordering::SeqCst) == 1).await;
        handle.request("SIGTERM");
    };
    let (got, ()) = tokio::join!(
        session_until(&mut s, &mut stop, &ident, &server, never),
        driver
    );
    let err = got.unwrap_err();
    assert!(matches!(err, brisk_client::Error::Abandoned(_)), "{err}");
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 1);
}

/// `brisk-probe stream` end to end against the fakes: launch, RTSP (ports pointing at the media
/// fake), control + A/V receive, then `/cancel`. Returns the stream result, `/cancel`s that
/// reached the fake, and the media fake's log.
async fn stream_run(
    media: common::fake_host::Media,
    secs: std::time::Duration,
    record: Option<&Path>,
) -> (
    brisk_client::Result<brisk_client::stream::Summary>,
    usize,
    Arc<Mutex<common::fake_host::Log>>,
) {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    s.set_rng(Fixed); // the media fake must know the launch key up front
    let cfg = StreamConfig {
        enc_flags: SS_ENC_AUDIO,
        ..stream_cfg()
    };
    let (rikey, rikeyid) = (RIKEY_FIXED, RIKEYID_FIXED);
    let host = common::fake_host::fake_with(common::fake_host::Cfg {
        key: rikey,
        connect_data: 0x12345678,
        ping_payload: "0123456789ABCDEF",
        drop: 0,
        media: Some(common::fake_host::Media {
            audio_key: Some((rikey, rikeyid)),
            ..media
        }),
        tap: None,
    });
    fake.rtsp.lock().unwrap().ports =
        Some([host.audio.port(), host.video.port(), host.addr.port()]);
    let f = fake.clone();
    let r = s
        .launch_session(
            &ident,
            &server,
            1,
            "7.1.431.-1",
            &cfg,
            TakeOver::Refuse,
            |p| async move {
                assert_eq!(f.rtsp.lock().unwrap().cancels, 0, "not cancelled yet");
                // DESCRIBE: supported 5, requested 1; we want audio.
                assert_eq!(p.enc_flags, SS_ENC_CONTROL_V2 | SS_ENC_AUDIO);
                assert_eq!((p.key.rikey, p.key.rikeyid), (rikey, rikeyid));
                brisk_client::stream::stream("127.0.0.1", &p, 1, secs, record, false).await
            },
        )
        .await;
    let cancels = fake.rtsp.lock().unwrap().cancels;
    (r, cancels, host.log)
}

fn secs(n: u64) -> std::time::Duration {
    std::time::Duration::from_secs(n)
}

#[tokio::test]
async fn stream_5s_passes_gate_then_cancels() {
    let (r, cancels, log) = stream_run(Default::default(), secs(5), None).await;
    let st = r.unwrap();
    assert_eq!(cancels, 1);
    assert!(st.passed() && st.frames >= 285, "{st}");
    assert_eq!(st.video_parse_errors + st.audio_parse_errors, 0, "{st}");
    // Opening IDR plus one per request, each answered in well under 2 s.
    assert!(st.idr_frames >= 3, "{st}");
    assert!(
        st.idr_response_ms.len() == 2 && st.idr_response_ms.iter().all(|r| r.unwrap() < 500.0),
        "{st}"
    );
    assert!(st.audio_ok >= 900 && st.audio_lost == 0, "{st}");
    assert_eq!((st.fec_recovered, st.video_loss.0), (0, 0), "{st}");
    let h = st.host_latency_ms.as_ref().unwrap();
    assert!(h[0] >= 3.0 && h[2] <= 4.9, "fake sends 3.0-4.9 ms: {st}");
    let l = log.lock().unwrap();
    assert_eq!(l.disconnects, 1, "graceful ENet disconnect before /cancel");
    assert!(st.frames <= l.frames as u64);
}

#[tokio::test]
async fn stream_3pct_video_loss_is_recovered_by_fec() {
    let media = common::fake_host::Media {
        loss_pct: 3,
        ..Default::default()
    };
    let (r, cancels, log) = stream_run(media, secs(3), None).await;
    let st = r.unwrap();
    assert_eq!(cancels, 1);
    let dropped = log.lock().unwrap().video_dropped as u64;
    assert!(dropped > 0);
    assert!(st.passed() && st.frames >= 171, "{st}");
    assert!(st.fec_recovered > 0 && st.frames_dropped == 0, "{st}");
    // Every dropped datagram shows as a missing sequence number (ones dropped near the end of
    // the run may not have been noticed yet).
    assert!(st.video_loss.0 > 0 && st.video_loss.0 <= dropped, "{st}");
}

#[tokio::test]
async fn stream_parse_error_fails_gate_but_still_cancels() {
    let media = common::fake_host::Media {
        corrupt_frame: Some(30),
        ..Default::default()
    };
    let (r, cancels, _) = stream_run(media, secs(2), None).await;
    let e = r.unwrap_err().to_string();
    assert!(
        e.contains("stream gate FAILED") && e.contains("parse_errors 1 > 0"),
        "{e}"
    );
    assert_eq!(cancels, 1);
}

#[tokio::test]
async fn stream_without_idr_answer_fails_gate_but_still_cancels() {
    let media = common::fake_host::Media {
        answer_idr: false,
        ..Default::default()
    };
    let (r, cancels, log) = stream_run(media, secs(2), None).await;
    let e = r.unwrap_err().to_string();
    assert!(
        e.contains("no IDR within 2s of request 1") && e.contains("request 2"),
        "{e}"
    );
    assert!(
        !e.contains("parse_errors") && !e.contains("frames_assembled"),
        "{e}"
    );
    assert_eq!(cancels, 1);
    assert_eq!(log.lock().unwrap().idr_frames, 1, "only the opening IDR");
}

#[tokio::test]
async fn stream_refuses_busy_host_without_launch() {
    let (mut s, fake, ident, server, _t) = paired("ok").await;
    fake.game.store(5, Ordering::SeqCst);
    let ran = std::sync::atomic::AtomicBool::new(false);
    let err = s
        .launch_session(
            &ident,
            &server,
            1,
            "7.1.431.-1",
            &stream_cfg(),
            TakeOver::Refuse,
            |p| {
                ran.store(true, Ordering::SeqCst);
                async move {
                    brisk_client::stream::stream("127.0.0.1", &p, 1, secs(1), None, false).await
                }
            },
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("server busy") && err.contains("currentgame=5"),
        "{err}"
    );
    assert!(!ran.load(Ordering::SeqCst));
    assert_eq!(fake.launches.load(Ordering::SeqCst), 0);
    assert_eq!(fake.cancel_reqs.load(Ordering::SeqCst), 0);
}

fn meta_field(json: &str, k: &str) -> u64 {
    let v = json.split(&format!("\"{k}\":")).nth(1).unwrap();
    v.split([',', '}']).next().unwrap().parse().unwrap()
}

/// `--record-samples`: the files the brisk-proto replay tests read, replayed the same way.
#[tokio::test]
async fn stream_records_replayable_samples() {
    use brisk_proto::audio::{PT_FEC, parse_audio_packet};
    use brisk_proto::rtp::parse_video_packet;
    let dir = tempfile::tempdir().unwrap();
    let (r, cancels, _) = stream_run(Default::default(), secs(1), Some(dir.path())).await;
    r.unwrap();
    assert_eq!(cancels, 1);
    let bins = |sub: &str| {
        let mut v: Vec<_> = std::fs::read_dir(dir.path().join(sub))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "bin"))
            .collect();
        v.sort();
        v
    };
    let video = bins("video");
    assert_eq!(video.len(), 120);
    let mut frame1 = 0;
    for f in &video {
        let raw = std::fs::read(f).unwrap();
        let p = parse_video_packet(&raw, 0).unwrap();
        let meta = std::fs::read_to_string(f.with_extension("meta.json")).unwrap();
        assert_eq!(meta_field(&meta, "frame_idx"), p.frame_idx as u64);
        assert_eq!(meta_field(&meta, "shard_idx"), p.shard_idx as u64);
        assert_eq!(meta_field(&meta, "data_shards"), p.data_shards as u64);
        assert_eq!(meta_field(&meta, "parity_shards"), p.parity_shards as u64);
        frame1 += u16::from(p.frame_idx == 1);
    }
    // The opening IDR (30008 bytes at 1376 per packet: 22 data + 5 parity) is in the samples whole.
    assert_eq!(frame1, 27);
    let a = dir.path().join("audio");
    let key = std::fs::read_to_string(a.join("rikey.hex")).unwrap();
    assert_eq!(key, "52494b45592d30313233343536373839 454b4952\n");
    assert!(
        std::fs::read_to_string(a.join("README.md"))
            .unwrap()
            .contains("ONE streaming session")
    );
    let audio = bins("audio");
    assert_eq!(audio.len(), 100);
    let mut data = 0;
    for f in audio {
        let raw = std::fs::read(f).unwrap();
        if raw[1] & 0x7f == PT_FEC {
            continue;
        }
        let o = parse_audio_packet(&raw, &RIKEY_FIXED, RIKEYID_FIXED).unwrap();
        assert_eq!(o.data.len(), 120);
        data += 1;
    }
    assert!(data >= 64);
}
