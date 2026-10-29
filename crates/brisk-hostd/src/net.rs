//! Outgoing HTTPS: one ureq agent setup, and error text that is safe to log.

use std::sync::Arc;
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// A ureq agent with rustls (graviola crypto) trusting `roots`, no proxy and no redirects.
/// `connect` and `total` are the time limits. With `status_as_error` false every status comes
/// back as a response.
pub(crate) fn agent(
    roots: RootCerts,
    connect: Option<Duration>,
    total: Duration,
    status_as_error: bool,
) -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(roots)
        .unversioned_rustls_crypto_provider(Arc::new(rustls_graviola::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(status_as_error)
        .timeout_connect(connect)
        .timeout_global(Some(total))
        .build()
        .into()
}

/// Error text safe to log: only the kind of failure. ureq's own messages can embed the URL, and
/// a URL can carry a key, so they are never passed on.
pub(crate) fn safe_err(e: &ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(c) => format!("HTTP {c}"),
        ureq::Error::Timeout(_) => "timed out".into(),
        // rustls handshake failures (bad certificate included) surface as InvalidData.
        ureq::Error::Io(e) if e.kind() == std::io::ErrorKind::InvalidData => "TLS error".into(),
        ureq::Error::Io(e) => format!("i/o error ({:?})", e.kind()),
        ureq::Error::HostNotFound => "host not found".into(),
        ureq::Error::BodyExceedsLimit(_) => "response too large".into(),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "TLS error".into(),
        _ => "request failed".into(),
    }
}
