//! Static pages (the built `web/dist`): tower-http's `ServeDir` does Range, ETag/Last-Modified,
//! 304s, HEAD and precompressed `.br`/`.gz` siblings; this module adds what Brisk needs on top:
//! the SPA rule, containment in the current build, and cache/security headers.
use axum::extract::Request;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use std::future::Future;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tower_http::services::fs::{Backend, Metadata as _, TokioBackend, TokioFile};

/// Where pages come from.
#[derive(Clone)]
pub enum Site {
    /// `web/dist`, a symlink `update` swaps atomically; resolved again on every request.
    Disk(PathBuf),
    /// Compiled into the binary (feature `embed`).
    #[cfg(feature = "embed")]
    Mem(Mem),
}

#[cfg(feature = "embed")]
pub use crate::embed::Mem;

impl Site {
    pub fn describe(&self) -> String {
        match self {
            Site::Disk(d) => format!("static files from disk: {}", d.display()),
            #[cfg(feature = "embed")]
            Site::Mem(m) => format!("static files embedded in the binary ({} entries)", m.len()),
        }
    }

    /// `BRISK_STATIC`: disk (the default) or embed. With the embed feature, a missing `dist` also
    /// falls back to the embedded build unless disk was asked for.
    pub fn choose(dist: PathBuf, mode: Option<&str>) -> Site {
        let embed = match mode {
            Some("embed") => true,
            None | Some("" | "disk") => false,
            Some(m) => {
                eprintln!("warning: BRISK_STATIC={m} is neither embed nor disk; using disk");
                false
            }
        };
        #[cfg(feature = "embed")]
        if embed || (matches!(mode, None | Some("")) && !dist.exists()) {
            return Site::Mem(Mem::embedded());
        }
        #[cfg(not(feature = "embed"))]
        if embed {
            eprintln!(
                "warning: BRISK_STATIC=embed but hostd was built without the embed feature; using disk"
            );
        }
        Site::Disk(dist)
    }

    /// GET/HEAD only; a hostile or unknown path is a 404, an unknown page (no extension) the root index.
    pub async fn serve(&self, req: Request) -> Response {
        let get = req.method() == Method::GET || req.method() == Method::HEAD;
        let Some(rel) = rel_path(req.uri().path()).filter(|_| get) else {
            return finish(StatusCode::NOT_FOUND.into_response(), false);
        };
        let res = match self {
            Site::Disk(dist) => {
                // Once per request, so one request never mixes two builds.
                let Ok(root) = tokio::fs::canonicalize(dist).await else {
                    return finish(StatusCode::NOT_FOUND.into_response(), false);
                };
                serve_from(root.clone(), Contained(Arc::new(root)), &rel, req).await
            }
            #[cfg(feature = "embed")]
            Site::Mem(m) => serve_from(PathBuf::new(), m.clone(), &rel, req).await,
        };
        // Extensionless paths may have fallen back to index.html: never cache those for a year.
        finish(res, rel.starts_with("_astro") && rel.extension().is_some())
    }
}

/// `uri_path` as a relative path of plain names, or None for anything that could leave the root
/// or be read two ways: `..`, `.`, encoded `/` `\` NUL, a decoded `\` or NUL, invalid UTF-8.
fn rel_path(uri_path: &str) -> Option<PathBuf> {
    let raw = uri_path.trim_start_matches('/');
    let lower = raw.to_ascii_lowercase();
    if ["%2f", "%5c", "%00"].iter().any(|e| lower.contains(e)) {
        return None;
    }
    let dec = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .ok()?;
    if dec.contains(['\\', '\0']) {
        return None;
    }
    let p = PathBuf::from(&*dec);
    p.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(p)
}

async fn serve_from<B: Backend>(root: PathBuf, b: B, rel: &Path, mut req: Request) -> Response {
    // Unknown page: the root index, so client-side routes still load. A missing file with an
    // extension is a missing asset, not a page; a path that exists but is refused stays a 404.
    let mut target = root.join(rel);
    if b.metadata(target.clone()).await.is_ok_and(|m| m.is_dir()) {
        target.push("index.html");
    }
    if rel.extension().is_none()
        && b.metadata(target)
            .await
            .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
    {
        *req.uri_mut() = Uri::from_static("/");
    }
    let res = ServeDir::with_backend(root, b)
        .append_index_html_on_directories(true)
        // `/library` and `/library/` both serve library/index.html, as before (no redirect).
        .redirect_to_trailing_slash(false)
        .precompressed_br()
        .precompressed_gzip()
        .try_call(req)
        .await;
    match res {
        Ok(r) => r.map(axum::body::Body::new),
        Err(e)
            if e.kind() == io::ErrorKind::NotFound
                || e.kind() == io::ErrorKind::PermissionDenied
                || e.raw_os_error() == Some(libc::ENOTDIR) =>
        {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(e) => {
            eprintln!("static file error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Cache and security headers for every static answer (no CSP yet: docs/ui-notes.md, Hosting).
fn finish(mut r: Response, hashed: bool) -> Response {
    let ok = r.status().is_success() || r.status() == StatusCode::NOT_MODIFIED;
    let h = r.headers_mut();
    // mime_guess says text/html (no charset) and application/javascript for .mjs.
    match h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        Some("text/html") => {
            h.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
        }
        Some("application/javascript") => {
            h.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/javascript"),
            );
        }
        _ => {}
    }
    // Astro's `_astro/*` names carry a content hash; everything else must revalidate.
    let cache = if hashed && ok {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    for (k, v) in [
        (header::CACHE_CONTROL, cache),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (
            header::HeaderName::from_static("cross-origin-resource-policy"),
            "same-origin",
        ),
    ] {
        h.insert(k, HeaderValue::from_static(v));
    }
    r
}

/// Disk backend that refuses (PermissionDenied, served as 404) anything whose real path is
/// outside the build root, e.g. a symlink inside the build pointing out of it. Opens the
/// canonical path, so what is checked is what is read.
#[derive(Clone)]
struct Contained(Arc<PathBuf>);

impl Contained {
    async fn inside(&self, p: PathBuf) -> io::Result<PathBuf> {
        let c = tokio::fs::canonicalize(p).await?;
        if c.starts_with(&*self.0) {
            Ok(c)
        } else {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
}

type Fut<T> = Pin<Box<dyn Future<Output = io::Result<T>> + Send>>;

impl Backend for Contained {
    type File = TokioFile;
    type Metadata = std::fs::Metadata;
    type OpenFuture = Fut<TokioFile>;
    type MetadataFuture = Fut<std::fs::Metadata>;

    fn open(&self, p: PathBuf) -> Self::OpenFuture {
        let me = self.clone();
        Box::pin(async move { TokioBackend.open(me.inside(p).await?).await })
    }

    fn metadata(&self, p: PathBuf) -> Self::MetadataFuture {
        let me = self.clone();
        Box::pin(async move { tokio::fs::metadata(me.inside(p).await?).await })
    }
}
