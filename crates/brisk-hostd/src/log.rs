//! hostd logs through `tracing`; this subscriber prints `level: message` lines to stderr (the journal under systemd).
//! `BRISK_LOG` picks the lowest level shown: `error`, `warn`, `info` (default), `debug` or `trace`.

use tracing::{Level, Metadata};

/// A `tracing` subscriber that writes lines to stderr at one level.
pub struct StderrLog(Level);

impl StderrLog {
    /// Installs the subscriber for the process. A second call does nothing.
    pub fn init(var: Option<&str>) {
        let _ = tracing::subscriber::set_global_default(Self(level(var)));
    }
}

/// Unknown or missing values mean `info`, so a typo never silences warnings.
fn level(var: Option<&str>) -> Level {
    var.and_then(|v| v.trim().parse().ok())
        .unwrap_or(Level::INFO)
}

impl tracing::Subscriber for StderrLog {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        *m.level() <= self.0
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
        let name = match *e.metadata().level() {
            Level::WARN => "warning",
            l => &l.as_str().to_lowercase(),
        };
        eprintln!("{name}: {}", m.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_reads_the_variable() {
        assert_eq!(level(None), Level::INFO);
        assert_eq!(level(Some("debug")), Level::DEBUG);
        assert_eq!(level(Some(" WARN ")), Level::WARN);
        assert_eq!(level(Some("loud")), Level::INFO);
    }
}
