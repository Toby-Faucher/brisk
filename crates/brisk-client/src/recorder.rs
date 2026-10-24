//! Fixture recorder: buffers raw HTTP exchanges, scrubs every secret, then writes
//! `<dir>/<name>.bin` + `<name>.meta.json`. Nothing reaches disk before scrubbing.

use brisk_proto::scrub::{scrub, scrub_with_encodings};
use std::path::{Path, PathBuf};

/// Secrets shorter than this are not scrubbed byte-wise: they would blank coincidental matches.
pub const MIN_SECRET: usize = 8;

pub struct Recorder {
    dir: PathBuf,
    entries: Vec<(String, bool, Vec<u8>)>,
    secrets: Vec<Vec<u8>>,
    pin: Option<String>,
}

impl Recorder {
    pub fn new(dir: &Path) -> Self {
        Recorder {
            dir: dir.to_owned(),
            entries: vec![],
            secrets: vec![],
            pin: None,
        }
    }

    /// Scrubbed raw and as upper/lower hex. Ignored if shorter than [`MIN_SECRET`].
    pub fn add_secret(&mut self, s: &[u8]) {
        if s.len() >= MIN_SECRET && !self.secrets.iter().any(|x| x == s) {
            self.secrets.push(s.to_vec());
        }
    }

    /// The PIN is 4 digits, too short for [`add_secret`]; it is blanked as ASCII text in a separate pass.
    pub fn set_pin(&mut self, pin: &str) {
        self.pin = Some(pin.to_owned());
    }

    /// `label` such as "pair-req"; `tls` marks bytes that travelled inside TLS (recorded as plaintext).
    pub fn push(&mut self, label: &str, tls: bool, bytes: &[u8]) {
        self.entries.push((label.to_owned(), tls, bytes.to_vec()));
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

    /// Scrubs and writes every entry; returns the written paths.
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
