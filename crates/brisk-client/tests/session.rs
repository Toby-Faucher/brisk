//! Pairing, serverinfo and applist against a local fake Sunshine (127.0.0.1, ephemeral ports).
//! The fake mirrors nvhttp.cpp's four pairing phases as pair_vectors.rs does; no real host is contacted.

use brisk_client::identity::{load_or_create_identity, Identity, ServerCert};
use brisk_client::recorder::Recorder;
use brisk_client::session::Session;
use brisk_proto::pair::*;
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
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use x509_cert::der::{DecodePem, Encode};
use x509_cert::Certificate;

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
    client_pem: String,
    clienthash: Vec<u8>,
    authorized: Option<Vec<u8>>,
}

struct Fake {
    pin: String,
    ident: &'static Identity,
    tls_port: u16,
    omit_len: std::sync::atomic::AtomicBool,
    st: Mutex<St>,
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
        match path {
            "/serverinfo" => xml(&format!(
                "<hostname>fake</hostname><appversion>7.1.431.-1</appversion><GfeVersion>3.23.0.74</GfeVersion><uniqueid>FAKE0123</uniqueid><HttpsPort>{}</HttpsPort><ServerCodecModeSupport>259</ServerCodecModeSupport><PairStatus>{}</PairStatus><currentgame>0</currentgame><state>SUNSHINE_SERVER_FREE</state>",
                self.tls_port, authed as u8
            )),
            "/applist" if authed => xml("<App><AppTitle>Desktop</AppTitle><ID>1</ID></App><App><AppTitle>Steam Big Picture</AppTitle><ID>2</ID></App>"),
            "/pair" if q.get("phrase").map(String::as_str) == Some("getservercert") => {
                let salt: [u8; 16] = unhex(&q["salt"]).unwrap().try_into().unwrap();
                st.key = derive_aes_key(salt, &self.pin).aes_key;
                st.client_pem = String::from_utf8(unhex(&q["clientcert"]).unwrap()).unwrap();
                xml(&format!("<paired>1</paired><plaincert>{}</plaincert>", hex(self.ident.cert_pem.as_bytes())))
            }
            "/pair" if q.get("phrase").map(String::as_str) == Some("pairchallenge") => {
                if authed { xml("<paired>1</paired>") } else { UNAUTHORIZED.into() }
            }
            "/pair" if q.contains_key("clientchallenge") => {
                let challenge = aes_ecb_decrypt(&key, &unhex(&q["clientchallenge"]).unwrap()).unwrap();
                let h = Sha256::new().chain_update(&challenge).chain_update(cert_sig(&self.ident.cert_pem)).chain_update(SERVER_SECRET).finalize();
                let resp = aes_ecb_encrypt(&key, &[h.as_slice(), &SERVER_CHALLENGE].concat());
                xml(&format!("<paired>1</paired><challengeresponse>{}</challengeresponse>", hex(&resp)))
            }
            "/pair" if q.contains_key("serverchallengeresp") => {
                st.clienthash = aes_ecb_decrypt(&key, &unhex(&q["serverchallengeresp"]).unwrap()).unwrap();
                let sk = SigningKey::<Sha256>::new(rsa::RsaPrivateKey::from_pkcs8_pem(&self.ident.key_pem).unwrap());
                let ps = [&SERVER_SECRET[..], &sk.sign(&SERVER_SECRET).to_vec()].concat();
                xml(&format!("<paired>1</paired><pairingsecret>{}</pairingsecret>", hex(&ps)))
            }
            "/pair" if q.contains_key("clientpairingsecret") => {
                let cps = unhex(&q["clientpairingsecret"]).unwrap();
                let (secret, sig) = cps.split_at(16);
                let h = Sha256::new().chain_update(SERVER_CHALLENGE).chain_update(cert_sig(&st.client_pem)).chain_update(secret).finalize();
                let c = Certificate::from_pem(&st.client_pem).unwrap();
                let pk = rsa::RsaPublicKey::from_public_key_der(&c.tbs_certificate.subject_public_key_info.to_der().unwrap()).unwrap();
                let ok = h.as_slice() == st.clienthash && VerifyingKey::<Sha256>::new(pk).verify(secret, &Signature::try_from(sig).unwrap()).is_ok();
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
    let len = if peer.is_some() && fake.omit_len.load(std::sync::atomic::Ordering::SeqCst) {
        String::new()
    } else {
        format!("Content-Length: {}\r\n", body.len())
    };
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\n{len}Connection: close\r\n\r\n{body}"
    );
    s.write_all(resp.as_bytes()).await.unwrap();
    s.flush().await.unwrap();
    // Dropped without close_notify on TLS, to exercise the client's Content-Length check.
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
    let fake = Arc::new(Fake {
        pin: pin.into(),
        ident,
        tls_port: https.local_addr().unwrap().port(),
        omit_len: Default::default(),
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
            serve_conn(tls, &f, peer).await;
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
    let server = s
        .pair(t.path(), &ident, "4821", Box::new(brisk_client::SystemRng))
        .await
        .unwrap();
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
    let server = s
        .pair(t.path(), &ident, "4821", Box::new(brisk_client::SystemRng))
        .await
        .unwrap();
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
    let err = s
        .pair(t.path(), &ident, "2222", Box::new(brisk_client::SystemRng))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("wrong PIN"), "{err}");
    assert_eq!(names(t.path()), before, "no server.pem, no temp files");
    assert!(fake.st.lock().unwrap().authorized.is_none());
}

#[tokio::test]
async fn pinned_cert_rejects_a_different_server() {
    let t = tempfile::tempdir().unwrap();
    let ident = load_or_create_identity(t.path()).unwrap();
    let (mut a, _) = start("4821", 0).await;
    let pinned_a = a
        .pair(t.path(), &ident, "4821", Box::new(brisk_client::SystemRng))
        .await
        .unwrap();
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
    let server = s
        .pair(
            &t.path().join("id"),
            &ident,
            pin,
            Box::new(Planted(drawn.clone())),
        )
        .await
        .unwrap();
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
