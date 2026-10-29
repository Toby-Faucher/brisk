//! File drop: `PUT /api/files/<name>` streams the body to a temp file in `~/Downloads/brisk`, then
//! links it under a free, sanitised name. Nothing is ever buffered whole.

use axum::body::Body;
use axum::http::StatusCode;
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::io::AsyncWriteExt;

pub const MAX_UPLOAD: u64 = 8 << 30;
const MAX_NAME: usize = 200;
/// Left free on the disk after an upload.
const RESERVE: u64 = 256 << 20;
/// Free space is checked again after every this many bytes written.
const CHECK_EVERY: u64 = 64 << 20;

/// Bidi overrides/isolates and zero-width characters: they make `gnp.exe` look like `exe.png`.
fn invisible(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

/// The basename only, no control or invisible characters, no leading dots or spaces, at most 200
/// bytes (the extension is kept). None if nothing is left, or for `.` and `..`.
pub fn sanitize(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    if base == "." || base == ".." {
        return None;
    }
    let s: String = base
        .chars()
        .filter(|&c| !c.is_control() && !invisible(c))
        .collect();
    let s = s
        .trim_start_matches(|c: char| c == '.' || c.is_whitespace())
        .trim_end();
    if s.is_empty() {
        return None;
    }
    if s.len() <= MAX_NAME {
        return Some(s.to_string());
    }
    let ext = s.rfind('.').map_or("", |i| &s[i..]);
    let ext = if ext.len() <= 16 { ext } else { "" };
    let mut cut = MAX_NAME - ext.len();
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    Some(format!("{}{ext}", &s[..cut]))
}

/// `name` for 0, else `stem (i).ext`.
pub fn candidate(name: &str, i: u32) -> String {
    if i == 0 {
        return name.to_string();
    }
    match name.rfind('.').filter(|&d| d > 0) {
        Some(d) => format!("{} ({i}){}", &name[..d], &name[d..]),
        None => format!("{name} ({i})"),
    }
}

/// Free bytes for an unprivileged user on the filesystem holding `p`.
pub fn disk_free(p: &Path) -> Option<u64> {
    let s = rustix::fs::statvfs(p).ok()?;
    Some(s.f_bavail * s.f_frsize)
}

pub struct Files {
    pub dir: PathBuf,
    /// `disk_free`; tests fake a full disk.
    pub free: fn(&Path) -> Option<u64>,
    active: Mutex<HashSet<String>>,
}

/// Removes the temp file and frees the name when the upload ends in any way, a hang-up included
/// (axum drops the handler future).
struct Guard<'a> {
    files: &'a Files,
    name: String,
    tmp: PathBuf,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.tmp);
        self.files.active.lock().unwrap().remove(&self.name);
    }
}

type Fail = (StatusCode, String);

fn fail(code: StatusCode, msg: &str) -> Fail {
    (code, msg.to_string())
}

impl Files {
    pub fn new(dir: PathBuf) -> Self {
        Files {
            dir,
            free: disk_free,
            active: Default::default(),
        }
    }

    /// Removes `.*.part` left by a hostd that died mid-upload (run at startup).
    pub fn clean_partials(&self) {
        for e in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.starts_with('.') && n.ends_with(".part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Saves `body` as `raw_name`; returns the final path and size.
    pub async fn upload(
        &self,
        raw_name: &str,
        len: Option<u64>,
        mut body: Body,
    ) -> Result<(PathBuf, u64), Fail> {
        let name =
            sanitize(raw_name).ok_or_else(|| fail(StatusCode::BAD_REQUEST, "bad file name"))?;
        let too_big = || fail(StatusCode::PAYLOAD_TOO_LARGE, "file is over 8 GiB");
        if len.is_some_and(|l| l > MAX_UPLOAD) {
            return Err(too_big());
        }
        let io = |e: std::io::Error| match e.raw_os_error() {
            Some(libc::ENOSPC) => fail(StatusCode::INSUFFICIENT_STORAGE, "bawn's disk is full"),
            _ => fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("couldn't save the file: {e}"),
            ),
        };
        // 0700 only applies to directories it creates; an existing one keeps its mode.
        tokio::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .await
            .map_err(io)?;
        let no_space = || {
            fail(
                StatusCode::INSUFFICIENT_STORAGE,
                "not enough free disk space on bawn",
            )
        };
        // Checked up front with a Content-Length, then again while writing (chunked bodies,
        // several uploads at once, anything else filling the disk).
        let short =
            |need: u64| (self.free)(&self.dir).is_some_and(|f| need.saturating_add(RESERVE) > f);
        if short(len.unwrap_or(0)) {
            return Err(no_space());
        }
        if !self.active.lock().unwrap().insert(name.clone()) {
            return Err(fail(
                StatusCode::CONFLICT,
                "this file is already being sent",
            ));
        }
        let tmp = self
            .dir
            .join(format!(".{name}.{:016x}.part", OsRng.next_u64()));
        let g = Guard {
            files: self,
            name,
            tmp,
        };
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&g.tmp)
            .await
            .map_err(io)?;
        let mut size = 0u64;
        let mut checked = 0u64;
        while let Some(fr) = body.frame().await {
            let fr = fr.map_err(|_| fail(StatusCode::BAD_REQUEST, "upload interrupted"))?;
            if let Ok(d) = fr.into_data() {
                size += d.len() as u64;
                if size > MAX_UPLOAD || len.is_some_and(|l| size > l) {
                    return Err(too_big());
                }
                f.write_all(&d).await.map_err(io)?;
                if size - checked >= CHECK_EVERY {
                    checked = size;
                    if short(0) {
                        return Err(no_space());
                    }
                }
            }
        }
        if len.is_some_and(|l| size != l) {
            return Err(fail(StatusCode::BAD_REQUEST, "upload interrupted"));
        }
        f.sync_all().await.map_err(io)?;
        drop(f);
        // A hard link never replaces an existing file, so two uploads cannot take the same name.
        for i in 0..10_000 {
            let p = self.dir.join(candidate(&g.name, i));
            match tokio::fs::hard_link(&g.tmp, &p).await {
                Ok(()) => return Ok((p, size)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io(e)),
            }
        }
        Err(fail(StatusCode::CONFLICT, "too many files with this name"))
    }
}
