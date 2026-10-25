//! The WebTransport server against a quinn client on loopback (our pure-Rust crypto on both
//! ends): who gets a session, what happens to everyone else, and when a frame counts as acked.

mod common;

use std::time::Duration;

use brisk_bridge::session::send_frame_stream;
use brisk_bridge::wt::{self, Acceptor, MAX_PENDING};
use brisk_client::tap::TapFrame;
use bytes::Bytes;
use common::*;
use tokio::sync::oneshot;
use tokio::time::timeout;
use web_transport_proto::ConnectResponse;

/// Closed on purpose by the bridge (code 0x101), not just dropped (an implicit close is code 0).
fn closed_by_server(e: &Result<quinn::ConnectionError, tokio::time::error::Elapsed>) -> bool {
    matches!(e, Ok(quinn::ConnectionError::ApplicationClosed(c)) if c.error_code == 0x101u32.into())
}

#[tokio::test]
async fn right_token_gets_a_session_and_its_streams() {
    let (ep, _d) = server();
    let acc = Acceptor::new(TOKEN, SECS2);
    let cli = async {
        let (_ep, c) = connect(ep.local_addr().unwrap()).await;
        let c = c.unwrap();
        let (_s, mut r) = send_connect(&c, &format!("/{TOKEN}")).await;
        let resp = ConnectResponse::read(&mut r).await.unwrap();
        // A message to the bridge: WebTransport uni stream (0x54, session id 0), payload 0x01.
        let mut u = c.open_uni().await.unwrap();
        u.write_all(&[0x40, 0x54, 0x00, 0x01]).await.unwrap();
        u.finish().unwrap();
        (resp.status, c, _ep, _s, r)
    };
    let (srv, (status, _c, _cep, _s, _r)) = tokio::join!(serve_one(&ep, &acc), cli);
    let (_session, mut inbox) = srv.unwrap();
    assert_eq!(status, http::StatusCode::OK);
    let m = timeout(SECS2, inbox.recv()).await.unwrap().unwrap();
    assert_eq!(&m[..], &[1]);
}

#[tokio::test]
async fn wrong_token_closes_the_connection() {
    let (ep, _d) = server();
    let acc = Acceptor::new(TOKEN, SECS2);
    let cli = async {
        let (_ep, c) = connect(ep.local_addr().unwrap()).await;
        let c = c.unwrap();
        let _s = send_connect(&c, "/0123456789abcdee").await;
        timeout(SECS2, c.closed()).await
    };
    let (srv, closed) = tokio::join!(serve_one(&ep, &acc), cli);
    assert!(matches!(srv, Err(wt::Error::Refused(_))));
    assert!(closed_by_server(&closed), "{closed:?}");
}

#[tokio::test]
async fn malformed_connect_closes_the_connection() {
    let (ep, _d) = server();
    let acc = Acceptor::new(TOKEN, SECS2);
    let cli = async {
        let (_ep, c) = connect(ep.local_addr().unwrap()).await;
        let c = c.unwrap();
        let (mut s, _r) = c.open_bi().await.unwrap();
        s.write_all(b"\x01\x05junk!").await.unwrap(); // HEADERS frame with a bad QPACK block
        s.finish().unwrap();
        timeout(SECS2, c.closed()).await
    };
    let (srv, closed) = tokio::join!(serve_one(&ep, &acc), cli);
    assert!(matches!(srv, Err(wt::Error::Refused(_))), "{:?}", srv.err());
    assert!(closed_by_server(&closed), "{closed:?}");
}

#[tokio::test]
async fn handshake_timeout_closes_the_connection() {
    let (ep, _d) = server();
    let acc = Acceptor::new(TOKEN, Duration::from_millis(300));
    let cli = async {
        let (_ep, c) = connect(ep.local_addr().unwrap()).await;
        // QUIC is up but no CONNECT ever comes; keep-alives would hold this open forever.
        timeout(SECS2, c.unwrap().closed()).await
    };
    let (srv, closed) = tokio::join!(serve_one(&ep, &acc), cli);
    assert!(matches!(srv, Err(wt::Error::Refused(_))));
    assert!(closed_by_server(&closed), "{closed:?}");
}

#[tokio::test]
async fn refuses_more_than_max_pending_unauthenticated_connections() {
    let (ep, _d) = server();
    let addr = ep.local_addr().unwrap();
    let acc = Acceptor::new(TOKEN, Duration::from_secs(30));
    let srv = async {
        let mut refused = 0;
        while let Some(i) = ep.accept().await {
            match acc.admit(&ep, i) {
                Some(h) => drop(tokio::spawn(h)),
                None => refused += 1,
            }
            if refused > 0 {
                return refused;
            }
        }
        refused
    };
    let cli = async {
        let mut held = vec![];
        for _ in 0..MAX_PENDING {
            let (e, c) = connect(addr).await;
            held.push((e, c.expect("within the cap")));
        }
        let (_e, extra) = connect(addr).await;
        (held, extra)
    };
    let (refused, (_held, extra)) =
        timeout(Duration::from_secs(10), async { tokio::join!(srv, cli) })
            .await
            .expect("the connection past the cap is refused, not held");
    assert_eq!(refused, 1);
    assert!(
        matches!(extra, Err(quinn::ConnectionError::ConnectionClosed(_))),
        "{extra:?}"
    );
}

fn frame(len: usize) -> TapFrame {
    TapFrame {
        idx: 7,
        idr: true,
        capture_ns: 1,
        tap_ns: 2,
        dropped: 0,
        annexb: Bytes::from(vec![0u8; len]),
    }
}

#[tokio::test]
async fn a_frame_read_to_the_end_is_acked() {
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let (_keep, mut cancel) = oneshot::channel();
    let reader = async {
        let mut u = c.accept_uni().await.unwrap();
        u.read_to_end(1 << 20).await.unwrap().len()
    };
    let f = frame(1000);
    let (sent, n) = tokio::join!(send_frame_stream(&s, &f, &mut cancel), reader);
    assert_eq!(n, 3 + 21 + 1000); // 0x40 0x54 + session id 0, header, payload
    assert!(sent.acked && sent.bridge_ns.is_some());
}

#[tokio::test]
async fn stop_sending_is_not_an_ack() {
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let (_keep, mut cancel) = oneshot::channel();
    let reader = async {
        let mut u = c.accept_uni().await.unwrap();
        u.stop(5u32.into()).unwrap();
        std::future::pending::<()>().await
    };
    let f = frame(4 << 20);
    let sent = tokio::select! {
        s = send_frame_stream(&s, &f, &mut cancel) => s,
        _ = reader => unreachable!(),
    };
    assert!(!sent.acked, "the browser stopped the stream");
    assert!(sent.bridge_ns.is_some());
}

#[tokio::test]
async fn a_cancelled_frame_is_reset_and_not_acked() {
    let (ep, _d) = server();
    let (s, c, _cep) = session_pair(&ep).await;
    let (stop, mut cancel) = oneshot::channel::<()>();
    let reader = async {
        let mut u = c.accept_uni().await.unwrap();
        stop.send(()).unwrap();
        u.read_to_end(8 << 20).await
    };
    let f = frame(4 << 20);
    let (sent, read) = tokio::join!(send_frame_stream(&s, &f, &mut cancel), reader);
    assert!(!sent.acked);
    assert!(
        matches!(
            read,
            Err(quinn::ReadToEndError::Read(quinn::ReadError::Reset(_)))
        ),
        "{read:?}"
    );
}
