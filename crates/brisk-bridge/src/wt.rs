//! WebTransport over HTTP/3 on quinn, server side: the subset Chrome needs to open one session
//! and receive server-initiated unidirectional streams (draft-ietf-webtrans-http3; framing from
//! `web-transport-proto`).
//!
//! Per connection: we open our control stream with SETTINGS (WebTransport, extended CONNECT,
//! datagrams), accept the browser's CONNECT on its first bidirectional stream and answer 200 only
//! when the path is the session token. The browser's own uni streams (H3 control, QPACK) are kept
//! open and ignored; its WebTransport uni streams are short messages for the bridge.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use subtle::ConstantTimeEq;
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, Semaphore};
use web_transport_proto::{ConnectRequest, ConnectResponse, Settings, StreamUni, VarInt};

use crate::quic_crypto::{endpoint_config, server_config, ServerCrypto};

pub const ALPN: &[u8] = b"h3";
/// Largest browser-to-bridge message (a uni stream); longer ones are dropped.
pub const MAX_MSG: usize = 64;
/// Messages queued from the browser before new ones are dropped.
const INBOX: usize = 16;
/// Outgoing datagram (audio) bytes quinn may queue per connection, counting its 32 bytes of
/// overhead per datagram: about 11 packets of Sunshine's 512 kbit/s stereo (55 ms). A send into
/// a full buffer evicts the oldest queued datagrams (quinn's `send_datagram`).
pub const AUDIO_SEND_BUFFER: usize = 4096;

#[derive(Debug)]
pub enum Error {
    Connection(quinn::ConnectionError),
    Write(quinn::WriteError),
    /// The CONNECT request was missing, malformed or not for our token.
    Refused(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Connection(e) => write!(f, "webtransport: {e}"),
            Error::Write(e) => write!(f, "webtransport: {e}"),
            Error::Refused(s) => write!(f, "webtransport refused: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<quinn::ConnectionError> for Error {
    fn from(e: quinn::ConnectionError) -> Self {
        Error::Connection(e)
    }
}
impl From<quinn::WriteError> for Error {
    fn from(e: quinn::WriteError) -> Self {
        Error::Write(e)
    }
}

/// A QUIC endpoint for WebTransport on `addr` (refuses the unspecified address: bind a
/// Tailscale or loopback IP).
pub fn endpoint(
    addr: SocketAddr,
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> std::io::Result<quinn::Endpoint> {
    if addr.ip().is_unspecified() {
        return Err(std::io::Error::other(
            "refusing to listen on all interfaces: give a Tailscale or loopback IP",
        ));
    }
    let crypto = ServerCrypto::new(certs, key, &[ALPN]).map_err(std::io::Error::other)?;
    let mut t = quinn::TransportConfig::default();
    t.keep_alive_interval(Some(Duration::from_secs(1)));
    t.datagram_send_buffer_size(AUDIO_SEND_BUFFER);
    t.max_idle_timeout(Some(
        quinn::IdleTimeout::try_from(Duration::from_secs(10)).expect("10 s fits"),
    ));
    let socket = std::net::UdpSocket::bind(addr)?;
    quinn::Endpoint::new(
        endpoint_config(),
        Some(server_config(crypto, t)),
        socket,
        Arc::new(quinn::TokioRuntime),
    )
}

/// Unauthenticated handshakes allowed at once; more are refused.
pub const MAX_PENDING: usize = 8;
/// Open connections allowed (pending handshakes, the viewer, one being replaced); more are
/// refused.
pub const MAX_CONNECTIONS: usize = MAX_PENDING + 2;
/// Application close code for a refused connection (H3_GENERAL_PROTOCOL_ERROR).
const REFUSED: u32 = 0x101;

/// Admits new connections and runs their handshakes: at most [`MAX_PENDING`] at a time, each
/// closed unless it opens a session for our token within `wait`.
pub struct Acceptor {
    token: Arc<str>,
    wait: Duration,
    pending: Arc<Semaphore>,
}

impl Acceptor {
    pub fn new(token: &str, wait: Duration) -> Self {
        Self {
            token: token.into(),
            wait,
            pending: Arc::new(Semaphore::new(MAX_PENDING)),
        }
    }

    /// The handshake to run (spawn it), or `None` when the connection was refused because
    /// [`MAX_PENDING`] handshakes are running or [`MAX_CONNECTIONS`] are open.
    #[allow(clippy::type_complexity)]
    pub fn admit(
        &self,
        ep: &quinn::Endpoint,
        i: quinn::Incoming,
    ) -> Option<
        impl std::future::Future<Output = Result<(Session, mpsc::Receiver<Bytes>), Error>>
            + Send
            + 'static,
    > {
        let permit = match self.pending.clone().try_acquire_owned() {
            Ok(p) if ep.open_connections() < MAX_CONNECTIONS => p,
            _ => {
                i.refuse();
                return None;
            }
        };
        let (token, wait) = (self.token.clone(), self.wait);
        Some(async move {
            let _permit = permit;
            let deadline = tokio::time::Instant::now() + wait;
            // A QUIC handshake that times out is dropped, which closes it (no other handle).
            let conn = tokio::time::timeout_at(deadline, i)
                .await
                .map_err(|_| Error::Refused("no QUIC handshake in time".into()))??;
            let r = match tokio::time::timeout_at(deadline, handshake(&conn, &token)).await {
                Ok(r) => r,
                Err(_) => Err(Error::Refused(format!(
                    "no WebTransport session within {} ms",
                    wait.as_millis()
                ))),
            };
            if r.is_err() {
                conn.close(REFUSED.into(), b"refused");
            }
            r
        })
    }
}

/// One WebTransport session (one browser tab).
pub struct Session {
    conn: quinn::Connection,
    /// `0x54, session id`: written first on every uni stream we open.
    uni_header: Bytes,
    /// Quarter stream id (session id / 4, RFC 9297): first in every datagram.
    datagram_header: Vec<u8>,
    // Closing either of these ends the session or the HTTP/3 connection.
    _control: quinn::SendStream,
    _connect: (quinn::SendStream, quinn::RecvStream),
}

/// HTTP/3 + WebTransport handshake; only a CONNECT to `/<token>` succeeds. Also returns the
/// browser's messages (the payload of each of its uni streams, at most [`MAX_MSG`] bytes). The
/// caller closes the connection on error.
async fn handshake(
    conn: &quinn::Connection,
    token: &str,
) -> Result<(Session, mpsc::Receiver<Bytes>), Error> {
    let mut control = conn.open_uni().await?;
    let mut settings = Settings::default();
    settings.enable_webtransport(1);
    settings
        .write(&mut control)
        .await
        .map_err(|e| Error::Refused(format!("settings: {e}")))?;

    let (mut send, mut recv) = conn.accept_bi().await?;
    let req = ConnectRequest::read(&mut recv)
        .await
        .map_err(|e| Error::Refused(format!("CONNECT: {e}")))?;
    let want = format!("/{token}");
    if !bool::from(req.url.path().as_bytes().ct_eq(want.as_bytes())) {
        let _ = ConnectResponse::new(http::StatusCode::NOT_FOUND)
            .write(&mut send)
            .await;
        let _ = send.finish();
        return Err(Error::Refused("wrong session token".into()));
    }
    ConnectResponse::new(http::StatusCode::OK)
        .write(&mut send)
        .await
        .map_err(|e| Error::Refused(format!("CONNECT reply: {e}")))?;
    // Only now: an unauthenticated peer gets no stream-draining tasks.
    let (tx, inbox) = mpsc::channel(INBOX);
    tokio::spawn(browser_uni_streams(conn.clone(), tx));

    let session_id = VarInt::try_from(u64::from(quinn::VarInt::from(send.id())))
        .expect("a QUIC stream id is a varint");
    let mut h = Vec::new();
    StreamUni::WEBTRANSPORT.encode(&mut h);
    session_id.encode(&mut h);
    let s = Session {
        conn: conn.clone(),
        uni_header: h.into(),
        datagram_header: datagram_header(session_id.into_inner()),
        _control: control,
        _connect: (send, recv),
    };
    Ok((s, inbox))
}

/// HTTP datagram prefix for the session on CONNECT stream `stream_id`: the quarter stream id
/// (`stream_id / 4`, RFC 9297 section 2.1) as a varint.
pub fn datagram_header(stream_id: u64) -> Vec<u8> {
    let mut h = Vec::new();
    VarInt::try_from(stream_id / 4)
        .expect("a stream id / 4 fits a varint")
        .encode(&mut h);
    h
}

/// Takes the browser's uni streams. Each gets its own task: the HTTP/3 control and QPACK
/// streams may stay silent for a long time and must not block the WebTransport ones behind them.
/// quinn caps them at 100 concurrent streams.
async fn browser_uni_streams(conn: quinn::Connection, tx: mpsc::Sender<Bytes>) {
    while let Ok(s) = conn.accept_uni().await {
        tokio::spawn(browser_uni(s, tx.clone()));
    }
}

/// A WebTransport message to forward, or an HTTP/3 stream (control, QPACK), drained until it
/// ends: stopping it would be a connection error.
async fn browser_uni(mut s: quinn::RecvStream, tx: mpsc::Sender<Bytes>) -> Option<()> {
    let typ = VarInt::read(&mut s).await.ok()?;
    if StreamUni(typ) != StreamUni::WEBTRANSPORT {
        tokio::io::copy(&mut s, &mut tokio::io::sink()).await.ok()?;
        return Some(());
    }
    VarInt::read(&mut s).await.ok()?; // session id: a connection has one session
    let mut m = Vec::with_capacity(MAX_MSG);
    (&mut s)
        .take(MAX_MSG as u64 + 1)
        .read_to_end(&mut m)
        .await
        .ok()?;
    if m.len() <= MAX_MSG {
        let _ = tx.try_send(m.into()); // full inbox: drop
    }
    Some(())
}

impl Session {
    /// A new uni stream with send priority `priority` (higher goes first) and the WebTransport
    /// header already queued.
    pub async fn open_uni(&self, priority: i32) -> Result<quinn::SendStream, Error> {
        let mut s = self.conn.open_uni().await?;
        let _ = s.set_priority(priority);
        s.write_all(&self.uni_header).await?;
        Ok(s)
    }

    /// Queues one WebTransport datagram. `Ok(true)`: the send buffer was full, so quinn evicted the
    /// oldest queued datagram(s) to make room.
    pub fn send_datagram(&self, payload: &[u8]) -> Result<bool, quinn::SendDatagramError> {
        let mut d = Vec::with_capacity(self.datagram_header.len() + payload.len());
        d.extend_from_slice(&self.datagram_header);
        d.extend_from_slice(payload);
        // Same rule as quinn's make_space_for: payload + 32 bytes of overhead must fit.
        let evict = self.conn.datagram_send_buffer_space() < d.len();
        self.conn.send_datagram(d.into())?;
        Ok(evict)
    }

    /// Largest datagram payload the connection takes now (path MTU and the browser's limit).
    pub fn max_datagram_payload(&self) -> Option<usize> {
        self.conn
            .max_datagram_size()
            .map(|m| m.saturating_sub(self.datagram_header.len()))
    }

    /// Resolves when the browser closes the session or the connection dies.
    pub async fn closed(&self) -> quinn::ConnectionError {
        self.conn.closed().await
    }

    pub fn remote(&self) -> SocketAddr {
        self.conn.remote_address()
    }

    pub fn close(&self) {
        self.conn.close(0u32.into(), b"replaced");
    }
}

#[cfg(test)]
mod tests {
    use super::datagram_header;

    #[test]
    fn datagram_header_is_the_quarter_stream_id_varint() {
        assert_eq!(datagram_header(0), [0x00]);
        assert_eq!(datagram_header(4), [0x01]);
        assert_eq!(datagram_header(8), [0x02]);
        assert_eq!(
            datagram_header(252),
            [0x3f],
            "quarter id 63: the last 1-byte varint"
        );
        assert_eq!(datagram_header(256), [0x40, 0x40], "quarter id 64: 2 bytes");
    }
}
