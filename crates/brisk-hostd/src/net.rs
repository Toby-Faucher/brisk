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

/// Why an outgoing request failed: only the kind of failure. ureq's own messages can embed the
/// URL, and a URL can carry a key, so they are never kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpError {
    /// The server answered with this status.
    Status(u16),
    /// The body went over the caller's cap.
    TooLarge,
    /// Anything else, as safe text ("timed out", "TLS error").
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Status(c) => write!(f, "HTTP {c}"),
            Self::TooLarge => f.write_str("response too large"),
            Self::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for HttpError {}

impl From<HttpError> for String {
    fn from(e: HttpError) -> String {
        e.to_string()
    }
}

impl From<&ureq::Error> for HttpError {
    fn from(e: &ureq::Error) -> Self {
        let other = |s: &str| Self::Other(s.into());
        match e {
            ureq::Error::StatusCode(c) => Self::Status(*c),
            ureq::Error::Timeout(_) => other("timed out"),
            // rustls handshake failures (bad certificate included) surface as InvalidData.
            ureq::Error::Io(e) if e.kind() == std::io::ErrorKind::InvalidData => other("TLS error"),
            ureq::Error::Io(e) => Self::Other(format!("i/o error ({:?})", e.kind())),
            ureq::Error::HostNotFound => other("host not found"),
            ureq::Error::BodyExceedsLimit(_) => Self::TooLarge,
            ureq::Error::Tls(_) | ureq::Error::Rustls(_) => other("TLS error"),
            _ => other("request failed"),
        }
    }
}

/// Error text safe to log (see `HttpError`).
pub(crate) fn safe_err(e: &ureq::Error) -> String {
    HttpError::from(e).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text_is_unchanged() {
        assert_eq!(HttpError::Status(503).to_string(), "HTTP 503");
        assert_eq!(HttpError::TooLarge.to_string(), "response too large");
        assert_eq!(
            HttpError::Other("TLS error".into()).to_string(),
            "TLS error"
        );
        let e = ureq::Error::StatusCode(404);
        assert_eq!(HttpError::from(&e), HttpError::Status(404));
        assert_eq!(safe_err(&e), "HTTP 404");
    }
}
