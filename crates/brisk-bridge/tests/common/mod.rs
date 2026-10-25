//! Shared by the bridge's integration tests: a WebTransport server on loopback and a quinn client
//! that plays the browser (our pure-Rust crypto on both ends).
#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use brisk_bridge::quic_crypto::{endpoint_config, provider, ClientCrypto};
use brisk_bridge::wt::{self, Acceptor};
use bytes::Bytes;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use web_transport_proto::{ConnectRequest, ConnectResponse};

pub const TOKEN: &str = "0123456789abcdef";
pub const SECS2: Duration = Duration::from_secs(2);

pub fn server() -> (quinn::Endpoint, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let id = brisk_client::identity::load_or_create_identity(dir.path()).unwrap();
    let certs = CertificateDer::pem_slice_iter(id.cert_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(id.key_pem.as_bytes()).unwrap();
    (
        wt::endpoint("127.0.0.1:0".parse().unwrap(), certs, key).unwrap(),
        dir,
    )
}

/// The test plays the browser and trusts any certificate.
#[derive(Debug)]
struct AnyCert;

impl ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2".into()))
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            m,
            c,
            d,
            &provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// A client endpoint and a QUIC connection to `addr` (no HTTP/3 yet).
pub async fn connect(
    addr: SocketAddr,
) -> (
    quinn::Endpoint,
    Result<quinn::Connection, quinn::ConnectionError>,
) {
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCert))
        .with_no_client_auth();
    tls.alpn_protocols = vec![wt::ALPN.to_vec()];
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut ep = quinn::Endpoint::new(
        endpoint_config(),
        None,
        socket,
        Arc::new(quinn::TokioRuntime),
    )
    .unwrap();
    ep.set_default_client_config(quinn::ClientConfig::new(Arc::new(ClientCrypto(Arc::new(
        tls,
    )))));
    let c = ep.connect(addr, "localhost").unwrap().await;
    (ep, c)
}

pub async fn send_connect(
    c: &quinn::Connection,
    path: &str,
) -> (quinn::SendStream, quinn::RecvStream) {
    let (mut send, recv) = c.open_bi().await.unwrap();
    let url = url::Url::parse(&format!("https://localhost{path}")).unwrap();
    ConnectRequest::new(url).write(&mut send).await.unwrap();
    (send, recv)
}

/// Accepts one incoming connection with `acc` and runs its handshake.
pub async fn serve_one(
    ep: &quinn::Endpoint,
    acc: &Acceptor,
) -> Result<(wt::Session, tokio::sync::mpsc::Receiver<Bytes>), wt::Error> {
    let i = ep.accept().await.unwrap();
    acc.admit(ep, i).expect("admitted").await
}

/// A session plus the client's connection, with the server's HTTP/3 control stream (its first uni
/// stream) already taken, so the next `accept_uni` is a frame.
pub async fn session_pair(
    ep: &quinn::Endpoint,
) -> (wt::Session, quinn::Connection, quinn::Endpoint) {
    let acc = Acceptor::new(TOKEN, SECS2);
    let cli = async {
        let (cep, c) = connect(ep.local_addr().unwrap()).await;
        let c = c.unwrap();
        let (s, mut r) = send_connect(&c, &format!("/{TOKEN}")).await;
        ConnectResponse::read(&mut r).await.unwrap();
        let control = c.accept_uni().await.unwrap();
        std::mem::forget((s, r, control)); // keep these open for the session's life
        (c, cep)
    };
    let (srv, (c, cep)) = tokio::join!(serve_one(ep, &acc), cli);
    (srv.unwrap().0, c, cep)
}
