//! Resumable uploads into the games folder. Design and API: docs/plans/2026-10-28-stage2-upload.md.
//! State is in memory; the bytes are in `<games>/.uploads/<id>.part` (0600). A chunk counts only
//! when its whole body arrived, so a dropped connection leaves the offset where it was.
//! Free space is checked before every chunk (at most 32 MiB apart, tighter than every 64 MiB).

use crate::files::{disk_free, sanitize};
use crate::ingest::{self, Mode};
use axum::body::Body;
use axum::http::StatusCode;
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// The largest body of one PUT.
pub const CHUNK: u64 = 32 << 20;
/// Free space kept after the file.
pub const RESERVE: u64 = 5 << 30;
const MAX_OPEN: usize = 2;
const DAY: Duration = Duration::from_secs(24 * 3600);

pub type Fail = (StatusCode, Value);

fn fail(code: StatusCode, msg: &str) -> Fail {
    (code, json!({ "error": msg }))
}

fn fail_at(code: StatusCode, msg: &str, offset: u64) -> Fail {
    (code, json!({ "error": msg, "offset": offset }))
}

struct St {
    offset: u64,
    busy: bool,
    /// The last finish failed in a way that is not the file's fault; the slot can be taken.
    failed: bool,
    /// Cancelled while a chunk was arriving: that chunk must not report success.
    aborted: bool,
    touched: Instant,
}

struct Entry {
    name: String,
    size: u64,
    st: Mutex<St>,
}

/// Frees the "a chunk is arriving" flag however the request ends (a hang-up drops the future).
struct Busy(Arc<Entry>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.st.lock().unwrap().busy = false;
    }
}

pub struct Uploads {
    games: PathBuf,
    /// `disk_free`; tests fake a full disk.
    pub free: fn(&Path) -> Option<u64>,
    open: Mutex<HashMap<String, Arc<Entry>>>,
}

pub struct Finished {
    pub system: &'static str,
    /// A library record exists (new or already there).
    pub game: Option<crate::game::GameId>,
    pub duplicate: bool,
}

fn io_fail(e: std::io::Error) -> Fail {
    match e.raw_os_error() {
        Some(libc::ENOSPC) => fail(StatusCode::INSUFFICIENT_STORAGE, "the disk is full"),
        _ => fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("file error: {e}"),
        ),
    }
}

impl Uploads {
    pub fn new(games: PathBuf) -> Self {
        Uploads {
            games,
            free: disk_free,
            open: Default::default(),
        }
    }

    fn part_dir(&self) -> PathBuf {
        self.games.join(".uploads")
    }

    fn part(&self, id: &str) -> PathBuf {
        self.part_dir().join(format!("{id}.part"))
    }

    /// Removes partial files older than a day (run at startup).
    pub fn clean_old(&self) {
        for e in std::fs::read_dir(self.part_dir())
            .into_iter()
            .flatten()
            .flatten()
        {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|a| a > DAY);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    fn short(&self, need: u64) -> bool {
        (self.free)(&self.games).is_some_and(|f| need.saturating_add(RESERVE) > f)
    }

    fn no_space() -> Fail {
        fail(
            StatusCode::INSUFFICIENT_STORAGE,
            "not enough free space (5 GB is kept free)",
        )
    }

    fn get(&self, id: &str) -> Result<Arc<Entry>, Fail> {
        self.open
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| fail(StatusCode::NOT_FOUND, "unknown upload"))
    }

    pub fn create(&self, name: &str, size: u64) -> Result<Value, Fail> {
        if name.contains(['/', '\\', '\0']) {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "the name must not contain a path",
            ));
        }
        let name = sanitize(name).ok_or_else(|| fail(StatusCode::BAD_REQUEST, "bad file name"))?;
        if !ingest::known_ext(&name) {
            return Err(fail(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "not a game file type brisk knows",
            ));
        }
        if size == 0 {
            return Err(fail(StatusCode::BAD_REQUEST, "size must be at least 1"));
        }
        if self.short(size) {
            return Err(Self::no_space());
        }
        let mut open = self.open.lock().unwrap();
        // Uploads idle for a day are dropped, with their file.
        open.retain(|id, e| {
            let keep = e.st.lock().unwrap().touched.elapsed() < DAY;
            if !keep {
                let _ = std::fs::remove_file(self.part(id));
            }
            keep
        });
        if open.len() >= MAX_OPEN {
            // a failed finish keeps its file for a retry, but not at the price of a new upload
            open.retain(|id, e| {
                let st = e.st.lock().unwrap();
                let keep = !st.failed || st.busy;
                if !keep {
                    let _ = std::fs::remove_file(self.part(id));
                }
                keep
            });
        }
        if open.len() >= MAX_OPEN {
            return Err(fail(
                StatusCode::TOO_MANY_REQUESTS,
                "two uploads are already open",
            ));
        }
        let mut b = [0u8; 16];
        OsRng.fill_bytes(&mut b);
        let id: String = b.iter().map(|x| format!("{x:02x}")).collect();
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(self.part_dir())
            .map_err(io_fail)?;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.part(&id))
            .map_err(io_fail)?;
        open.insert(
            id.clone(),
            Arc::new(Entry {
                name,
                size,
                st: Mutex::new(St {
                    offset: 0,
                    busy: false,
                    failed: false,
                    aborted: false,
                    touched: Instant::now(),
                }),
            }),
        );
        Ok(json!({ "id": id, "chunk": CHUNK, "offset": 0 }))
    }

    pub fn status(&self, id: &str) -> Result<Value, Fail> {
        let e = self.get(id)?;
        let st = e.st.lock().unwrap();
        Ok(json!({
            "id": id, "name": e.name, "offset": st.offset, "size": e.size,
            "state": if st.busy { "busy" } else { "open" },
        }))
    }

    pub async fn put(
        &self,
        id: &str,
        x_offset: Option<&str>,
        len: Option<u64>,
        mut body: Body,
    ) -> Result<Value, Fail> {
        let e = self.get(id)?;
        let want: u64 = x_offset
            .and_then(|v| v.trim().parse().ok())
            .ok_or_else(|| {
                fail(
                    StatusCode::BAD_REQUEST,
                    "x-offset header missing or not a number",
                )
            })?;
        let offset = {
            let mut st = e.st.lock().unwrap();
            if st.busy {
                return Err(fail_at(
                    StatusCode::CONFLICT,
                    "a chunk is already arriving",
                    st.offset,
                ));
            }
            if want != st.offset {
                return Err(fail_at(StatusCode::CONFLICT, "wrong offset", st.offset));
            }
            st.busy = true;
            st.touched = Instant::now();
            st.offset
        };
        let _busy = Busy(e.clone());
        let left = e.size - offset;
        if len.is_some_and(|l| l > CHUNK || l > left) {
            return Err(fail(StatusCode::PAYLOAD_TOO_LARGE, "chunk too big"));
        }
        if self.short(left) {
            return Err(Self::no_space());
        }
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .open(self.part(id))
            .await
            .map_err(io_fail)?;
        // Bytes of an earlier chunk that never completed are cut off here.
        f.set_len(offset).await.map_err(io_fail)?;
        f.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(io_fail)?;
        let mut n = 0u64;
        while let Some(fr) = body.frame().await {
            if e.st.lock().unwrap().aborted {
                return Err(fail(StatusCode::NOT_FOUND, "unknown upload"));
            }
            let fr = fr.map_err(|_| fail(StatusCode::BAD_REQUEST, "upload interrupted"))?;
            let Ok(d) = fr.into_data() else { continue };
            n += d.len() as u64;
            if n > CHUNK || n > left {
                return Err(fail(StatusCode::PAYLOAD_TOO_LARGE, "chunk too big"));
            }
            f.write_all(&d).await.map_err(io_fail)?;
        }
        if n == 0 || len.is_some_and(|l| l != n) {
            return Err(fail(StatusCode::BAD_REQUEST, "empty or interrupted chunk"));
        }
        f.flush().await.map_err(io_fail)?;
        let mut st = e.st.lock().unwrap();
        if st.aborted {
            return Err(fail(StatusCode::NOT_FOUND, "unknown upload"));
        }
        st.offset = offset + n;
        st.touched = Instant::now();
        Ok(json!({ "offset": st.offset }))
    }

    pub fn abort(&self, id: &str) {
        if let Some(e) = self.open.lock().unwrap().remove(id) {
            e.st.lock().unwrap().aborted = true;
            let _ = std::fs::remove_file(self.part(id));
        }
    }

    /// Checks the size, identifies the file, moves it into place and registers it.
    pub async fn finish(&self, data_dir: &Path, id: &str) -> Result<Finished, Fail> {
        let e = self.get(id)?;
        {
            let mut st = e.st.lock().unwrap();
            if st.busy {
                return Err(fail_at(
                    StatusCode::CONFLICT,
                    "a chunk is still arriving",
                    st.offset,
                ));
            }
            if st.offset != e.size {
                return Err((
                    StatusCode::CONFLICT,
                    json!({ "error": "the file is not complete", "offset": st.offset, "size": e.size }),
                ));
            }
            st.busy = true;
            st.failed = false;
        }
        let _busy = Busy(e.clone());
        let (part, games, data_dir, name) = (
            self.part(id),
            self.games.clone(),
            data_dir.to_path_buf(),
            e.name.clone(),
        );
        let r = tokio::task::spawn_blocking(move || {
            place_and_register(&part, &games, &data_dir, &name)
        })
        .await
        .map_err(|_| fail(StatusCode::INTERNAL_SERVER_ERROR, "finish task failed"))?;
        // A damaged file is dropped; any other failure leaves the upload to try again.
        if !self.part(id).exists()
            || matches!(&r, Err((c, _)) if *c == StatusCode::UNPROCESSABLE_ENTITY)
        {
            self.abort(id);
        } else if r.is_err() {
            e.st.lock().unwrap().failed = true;
        }
        r
    }
}

fn place_and_register(
    part: &Path,
    games: &Path,
    data_dir: &Path,
    name: &str,
) -> Result<Finished, Fail> {
    std::fs::File::open(part)
        .and_then(|f| f.sync_all())
        .map_err(io_fail)?;
    let id = crate::discid::identify(part).map_err(|e| {
        fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            &format!("not a usable game file: {e}"),
        )
    })?;
    let dir = games.join(ingest::folder(id.system));
    let path = ingest::place(part, &dir, name, Mode::Move).map_err(io_fail)?;
    let system = ingest::folder(id.system);
    // A system with no emulator, or an unknown one, is stored but gets no card.
    let Some(rec) = ingest::record(&path, &id, "upload") else {
        return Ok(Finished {
            system,
            game: None,
            duplicate: false,
        });
    };
    let f = vec![ingest::Found { path, id }];
    ingest::update(data_dir, |recs| {
        let m = ingest::register(data_dir, recs, f, "upload", false);
        let game = crate::emu::find(recs, &rec.id).map(|r| r.id.clone());
        (
            !m.added.is_empty() || m.aliased > 0,
            Finished {
                system,
                game,
                duplicate: m.added.is_empty() && m.aliased == 0,
            },
        )
    })
    .map_err(|e| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("stored, but not added to the library: {e}"),
        )
    })
}
