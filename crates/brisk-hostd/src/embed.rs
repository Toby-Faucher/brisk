//! Feature `embed`: the web build compiled into the binary (build.rs picks and checks the dir).
//! `Mem` is a tower-http `Backend`, so site.rs serves it through the same ServeDir path, rules
//! and headers as the disk build.
use axum::body::Bytes;
use std::collections::HashMap;
use std::future::{Ready, ready};
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use tower_http::services::fs::{Backend, File, Metadata};

static DIST: include_dir::Dir<'static> = include_dir::include_dir!("$BRISK_EMBED_ABS");

/// Relative path ("" is the root) to file bytes, or None for a directory; plus Last-Modified.
#[derive(Clone, Default)]
pub struct Mem(Arc<HashMap<PathBuf, (Option<Bytes>, SystemTime)>>);

impl Mem {
    /// The build compiled in by build.rs.
    pub fn embedded() -> Mem {
        fn add(
            d: &'static include_dir::Dir<'static>,
            t: SystemTime,
            m: &mut HashMap<PathBuf, (Option<Bytes>, SystemTime)>,
        ) {
            m.insert(d.path().to_path_buf(), (None, t));
            for e in d.entries() {
                match e {
                    include_dir::DirEntry::Dir(s) => add(s, t, m),
                    include_dir::DirEntry::File(f) => {
                        m.insert(
                            f.path().to_path_buf(),
                            (Some(Bytes::from_static(f.contents())), t),
                        );
                    }
                }
            }
        }
        // Set by build.rs (BRISK_EMBED_STAMP, SOURCE_DATE_EPOCH or build time).
        let stamp: u64 = env!("BRISK_EMBED_STAMP").parse().unwrap_or(0);
        let mut m = HashMap::new();
        add(&DIST, UNIX_EPOCH + Duration::from_secs(stamp), &mut m);
        Mem(Arc::new(m))
    }

    /// Where build.rs took the embedded build from.
    pub fn embedded_dir() -> &'static str {
        env!("BRISK_EMBED_ABS")
    }

    /// A runtime copy of `dir` (tests), keeping only what the disk server would serve: entries
    /// whose real path is inside `dir`, no symlinked directories, nothing dangling.
    pub fn from_dir(dir: &Path) -> Mem {
        fn walk(root: &Path, rel: &Path, m: &mut HashMap<PathBuf, (Option<Bytes>, SystemTime)>) {
            let Ok(rd) = std::fs::read_dir(root.join(rel)) else {
                return;
            };
            for e in rd.flatten() {
                let r = rel.join(e.file_name());
                let Ok(real) = e.path().canonicalize() else {
                    continue;
                };
                let Ok(md) = std::fs::metadata(&real) else {
                    continue;
                };
                let t = md.modified().unwrap_or(UNIX_EPOCH);
                let link = e.file_type().is_ok_and(|f| f.is_symlink());
                if !real.starts_with(root) || (md.is_dir() && link) {
                    continue;
                }
                if md.is_dir() {
                    m.insert(r.clone(), (None, t));
                    walk(root, &r, m);
                } else if !md.is_file() {
                    // FIFOs, sockets, devices: build.rs refuses them, the disk server 404s them.
                    continue;
                } else if let Ok(b) = std::fs::read(&real) {
                    m.insert(r, (Some(b.into()), t));
                }
            }
        }
        let mut m = HashMap::new();
        if let Ok(root) = dir.canonicalize() {
            m.insert(PathBuf::new(), (None, UNIX_EPOCH));
            walk(&root, Path::new(""), &mut m);
        }
        Mem(Arc::new(m))
    }

    /// Bytes of the file at `rel`, if there is one.
    pub fn file(&self, rel: &str) -> Option<Bytes> {
        self.0.get(Path::new(rel))?.0.clone()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// ServeDir hands over `./<rel>` (its base is "." joined with ours, "").
    fn get(&self, p: &Path) -> io::Result<(Option<Bytes>, Meta)> {
        let (b, t) = self
            .0
            .get(p.strip_prefix(".").unwrap_or(p))
            .ok_or(io::ErrorKind::NotFound)?;
        let m = Meta {
            dir: b.is_none(),
            len: b.as_ref().map_or(0, |b| b.len() as u64),
            modified: *t,
        };
        Ok((b.clone(), m))
    }
}

#[derive(Clone)]
pub struct Meta {
    dir: bool,
    len: u64,
    modified: SystemTime,
}

impl Metadata for Meta {
    fn is_dir(&self) -> bool {
        self.dir
    }
    fn modified(&self) -> io::Result<SystemTime> {
        Ok(self.modified)
    }
    fn len(&self) -> u64 {
        self.len
    }
}

pub struct MemFile(io::Cursor<Bytes>, Meta);

impl AsyncRead for MemFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncSeek for MemFile {
    fn start_seek(mut self: Pin<&mut Self>, pos: io::SeekFrom) -> io::Result<()> {
        Pin::new(&mut self.0).start_seek(pos)
    }
    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Pin::new(&mut self.0).poll_complete(cx)
    }
}

impl File for MemFile {
    type Metadata = Meta;
    type MetadataFuture<'a> = Ready<io::Result<Meta>>;
    fn metadata(&self) -> Self::MetadataFuture<'_> {
        ready(Ok(self.1.clone()))
    }
}

impl Backend for Mem {
    type File = MemFile;
    type Metadata = Meta;
    type OpenFuture = Ready<io::Result<MemFile>>;
    type MetadataFuture = Ready<io::Result<Meta>>;

    fn open(&self, p: PathBuf) -> Self::OpenFuture {
        ready(match self.get(&p) {
            Ok((Some(b), m)) => Ok(MemFile(io::Cursor::new(b), m)),
            Ok((None, _)) => Err(io::ErrorKind::NotFound.into()),
            Err(e) => Err(e),
        })
    }

    fn metadata(&self, p: PathBuf) -> Self::MetadataFuture {
        ready(self.get(&p).map(|(_, m)| m))
    }
}
