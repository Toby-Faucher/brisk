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

/// The basename only, no control characters, no leading dots, at most 200 bytes (the extension is
/// kept). None if nothing is left.
pub fn sanitize(raw: &str) -> Option<String> {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = base.chars().filter(|c| !c.is_control()).collect();
    let s = s.trim().trim_start_matches('.').trim();
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
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs fills the zeroed struct; c is a valid NUL-terminated path.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0)
        .then(|| s.f_bavail as u64 * s.f_frsize as u64)
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
        tokio::fs::create_dir_all(&self.dir).await.map_err(io)?;
        if let (Some(l), Some(f)) = (len, (self.free)(&self.dir)) {
            if l.saturating_add(RESERVE) > f {
                return Err(fail(
                    StatusCode::INSUFFICIENT_STORAGE,
                    "not enough free disk space on bawn",
                ));
            }
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
        let mut f = tokio::fs::File::create_new(&g.tmp).await.map_err(io)?;
        let mut size = 0u64;
        while let Some(fr) = body.frame().await {
            let fr = fr.map_err(|_| fail(StatusCode::BAD_REQUEST, "upload interrupted"))?;
            if let Ok(d) = fr.into_data() {
                size += d.len() as u64;
                if size > MAX_UPLOAD || len.is_some_and(|l| size > l) {
                    return Err(too_big());
                }
                f.write_all(&d).await.map_err(io)?;
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
