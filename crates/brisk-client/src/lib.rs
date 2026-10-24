//! Brisk client orchestration: identity, HTTP(S) session, RTSP handshake, control stream,
//! A/V stream receive, Sunshine tap receiver, fixture recorder. Logs go through `tracing`.

pub mod control;
pub mod identity;
pub mod recorder;
pub mod rtsp;
pub mod session;
pub mod stream;
pub mod tap;

use brisk_proto::{http::HttpError, input::InputError, pair::PairError, tap::TapError};

/// What went wrong, by kind; the strings are context for people, the variant is for callers.
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// Sunshine refused with an HTTP or XML status (a complete reply: nothing was started).
    Http(HttpError),
    Pair(PairError),
    Tls(rustls::Error),
    Tap(TapError),
    /// An input event the wire format cannot carry.
    Input(InputError),
    /// Malformed or unexpected bytes from the host (HTTP framing, RTSP, control, certificates).
    Protocol(String),
    /// No answer within the deadline.
    Timeout(String),
    /// The host already runs something we will not cancel.
    Busy(String),
    /// Launch, cancel or session lifecycle: rejected, outcome unknown, ended by the host, cancel failed.
    Session(String),
    /// Local state: not paired, no identity, bad local files or arguments.
    Setup(String),
    /// The other end went away (tap EOF, control session dropped).
    Closed(&'static str),
    /// `brisk-probe stream` gate failures.
    Gate(Vec<String>),
    /// `context: error`.
    Context(String, Box<Error>),
    /// An error, then what the cleanup after it did (`first; then`).
    Then(Box<Error>, Box<Error>),
}

impl Error {
    pub fn context(self, c: impl Into<String>) -> Self {
        Error::Context(c.into(), Box::new(self))
    }
    pub fn then(self, after: Error) -> Self {
        Error::Then(Box::new(self), Box::new(after))
    }
    /// The innermost error, through [`Error::Context`] and the first half of [`Error::Then`].
    pub fn root(&self) -> &Error {
        match self {
            Error::Context(_, e) | Error::Then(e, _) => e.root(),
            e => e,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => e.fmt(f),
            Error::Http(e) => e.fmt(f),
            Error::Pair(e) => e.fmt(f),
            Error::Tls(e) => e.fmt(f),
            Error::Tap(e) => e.fmt(f),
            Error::Input(e) => write!(f, "input: {e:?}"),
            Error::Protocol(s)
            | Error::Timeout(s)
            | Error::Busy(s)
            | Error::Session(s)
            | Error::Setup(s) => f.write_str(s),
            Error::Closed(s) => write!(f, "{s} closed"),
            Error::Gate(v) => write!(f, "stream gate FAILED: {}", v.join("; ")),
            Error::Context(c, e) => write!(f, "{c}: {e}"),
            Error::Then(a, b) => write!(f, "{a}; {b}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            Error::Http(e) => Some(e),
            Error::Pair(e) => Some(e),
            Error::Tls(e) => Some(e),
            Error::Tap(e) => Some(e),
            Error::Context(_, e) | Error::Then(e, _) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<HttpError> for Error {
    fn from(e: HttpError) -> Self {
        Error::Http(e)
    }
}
impl From<PairError> for Error {
    fn from(e: PairError) -> Self {
        Error::Pair(e)
    }
}
impl From<rustls::Error> for Error {
    fn from(e: rustls::Error) -> Self {
        Error::Tls(e)
    }
}
impl From<InputError> for Error {
    fn from(e: InputError) -> Self {
        Error::Input(e)
    }
}
impl From<TapError> for Error {
    fn from(e: TapError) -> Self {
        Error::Tap(e)
    }
}
/// Host bytes our protocol code rejected.
macro_rules! protocol_from {
    ($($t:ty),*) => {$(
        impl From<$t> for Error {
            fn from(e: $t) -> Self {
                Error::Protocol(e.to_string())
            }
        }
    )*};
}
protocol_from!(
    brisk_proto::control::ControlError,
    brisk_proto::enet::EnetError,
    brisk_proto::rtsp::RtspError
);

pub type Result<T> = std::result::Result<T, Error>;

/// CSPRNG (OS `getrandom`) for pairing salt, challenge and secret.
pub struct SystemRng;
impl brisk_proto::pair::Rng for SystemRng {
    fn fill(&mut self, buf: &mut [u8]) {
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, buf)
    }
}

/// The library logs through `tracing`; [`StderrLog`] prints warnings and errors to stderr, nothing else.
pub struct StderrLog;

impl tracing::Subscriber for StderrLog {
    fn enabled(&self, m: &tracing::Metadata<'_>) -> bool {
        *m.level() <= tracing::Level::WARN
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1) // spans are not used
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        struct Msg(String);
        impl tracing::field::Visit for Msg {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}");
                }
            }
        }
        let mut m = Msg(String::new());
        e.record(&mut m);
        eprintln!("{}: {}", e.metadata().level(), m.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}
