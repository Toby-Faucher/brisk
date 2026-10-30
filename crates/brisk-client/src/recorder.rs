//! Fixture recorder: buffers raw HTTP exchanges, scrubs every secret, then writes
//! `<dir>/<name>.bin` + `<name>.meta.json`. Nothing reaches disk before scrubbing.

use brisk_proto::scrub::{scrub, scrub_with_encodings};
use std::path::{Path, PathBuf};

/// Secrets shorter than this are not scrubbed byte-wise: they would blank coincidental matches.
pub const MIN_SECRET: usize = 8;

/// Whether recorded bytes travelled encrypted on the wire (they are stored as plaintext).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// Sent as is.
    Plain,
    /// Sent inside TLS or the encrypted RTSP framing.
    Encrypted,
}

/// Collects raw exchanges in memory and writes them out scrubbed.
///
/// Lifecycle: [`Recorder::new`], register secrets ([`Recorder::add_secret`], [`Recorder::set_pin`]),
/// [`Recorder::push`] each exchange, then [`Recorder::finish`]. Nothing hits disk until `finish`,
/// which consumes the recorder; dropping it without `finish` discards everything.
pub struct Recorder {
    dir: PathBuf,
    entries: Vec<(String, bool, Vec<u8>)>,
    secrets: Vec<Vec<u8>>,
    pin: Option<String>,
}

impl Recorder {
    /// An empty recorder that will write into `dir` (created by `finish`, not here).
    pub fn new(dir: &Path) -> Self {
        Recorder {
            dir: dir.to_owned(),
            entries: vec![],
            secrets: vec![],
            pin: None,
        }
    }

    /// Scrubbed raw and as upper/lower hex. Ignored if shorter than [`MIN_SECRET`].
    ///
    /// Limitation: a secret made only of `0x00` bytes is not effectively scrubbed, because the
    /// scrubber's filler is `0x00` (see `brisk_proto::scrub::scrub`); its hex text is still blanked.
    pub fn add_secret(&mut self, s: &[u8]) {
        if s.len() >= MIN_SECRET && !self.secrets.iter().any(|x| x == s) {
            self.secrets.push(s.to_vec());
        }
    }

    /// The PIN is 4 digits, too short for [`Recorder::add_secret`]; it is blanked as ASCII text in a separate pass.
    pub fn set_pin(&mut self, pin: &str) {
        self.pin = Some(pin.to_owned());
    }

    /// `label` such as "pair-req"; `wire` says whether the bytes travelled encrypted (recorded as plaintext).
    pub fn push(&mut self, label: &str, wire: Wire, bytes: &[u8]) {
        self.entries
            .push((label.to_owned(), wire == Wire::Encrypted, bytes.to_vec()));
    }

    fn clean(&self, b: &[u8]) -> Vec<u8> {
        let out = scrub_with_encodings(b, &self.secrets);
        // shortcut: the PIN pass also blanks any coincidental run of the same 4 digits (e.g. in a
        // Content-Length or hex blob); the PIN itself never travels on the wire, so this only guards greps.
        match &self.pin {
            Some(p) => scrub(&out, &[p.as_bytes().to_vec()]),
            None => out,
        }
    }

    /// Scrubs and writes every entry (`NNN-label.bin` and `.meta.json`) and returns the written
    /// paths. Consumes the recorder: this is the only point where anything reaches disk.
    pub fn finish(self) -> crate::Result<Vec<PathBuf>> {
        std::fs::create_dir_all(&self.dir)?;
        let mut paths = vec![];
        for (i, (label, tls, bytes)) in self.entries.iter().enumerate() {
            let name = format!("{:03}-{label}", i + 1);
            let meta = format!(
                "{{\"seq\":{},\"label\":\"{label}\",\"tls\":{tls},\"scrubbed\":true}}\n",
                i + 1
            );
            for (ext, data) in [("bin", bytes.as_slice()), ("meta.json", meta.as_bytes())] {
                let p = self.dir.join(format!("{name}.{ext}"));
                std::fs::write(&p, self.clean(data))?;
                paths.push(p);
            }
        }
        Ok(paths)
    }
}
