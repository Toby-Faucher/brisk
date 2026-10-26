//! Feature `embed` only: pick the web build to compile in and check it. Never runs npm.
//! BRISK_EMBED_DIR (absolute, or relative to the workspace root) overrides `web/dist`.
//! Fails the build when the dir has no index.html, or holds a dangling symlink, a symlinked
//! directory, a symlink out of the tree or anything that is not a regular file or directory (a
//! FIFO would hang the compile; the disk server refuses all of these, baking them in would not).
//! Last-Modified of the embedded files: BRISK_EMBED_STAMP, else SOURCE_DATE_EPOCH (reproducible
//! builds: set it per commit, e.g. `git log -1 --format=%ct`), else the time this script ran.
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=BRISK_EMBED_DIR");
    println!("cargo:rerun-if-env-changed=BRISK_EMBED_STAMP");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    if std::env::var_os("CARGO_FEATURE_EMBED").is_none() {
        return;
    }
    let ws = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let dir = ws.join(std::env::var_os("BRISK_EMBED_DIR").unwrap_or_else(|| "web/dist".into()));
    println!("cargo:rerun-if-changed={}", dir.display());
    let root = match dir.canonicalize() {
        Ok(r) if r.join("index.html").is_file() => r,
        _ => panic!(
            "feature `embed`: {} has no index.html. Build the web UI first (cd web && npm run build) \
             or set BRISK_EMBED_DIR to a built dist directory.",
            dir.display()
        ),
    };
    check(&root, &root);
    println!("cargo:rerun-if-changed={}", root.display());
    println!("cargo:rustc-env=BRISK_EMBED_ABS={}", root.display());
    // Last-Modified (and so the ETag) of every embedded file. Without an override it changes
    // whenever this script reruns, i.e. whenever the embedded tree or BRISK_EMBED_DIR changes.
    let env_secs = |k| {
        let v = std::env::var(k).ok().filter(|v| !v.is_empty())?;
        Some(
            v.parse::<u64>()
                .unwrap_or_else(|_| panic!("{k}={v} is not a number of seconds")),
        )
    };
    let stamp = env_secs("BRISK_EMBED_STAMP")
        .or_else(|| env_secs("SOURCE_DATE_EPOCH"))
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });
    println!("cargo:rustc-env=BRISK_EMBED_STAMP={stamp}");
}

fn check(root: &Path, dir: &Path) {
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        let link = e.file_type().unwrap().is_symlink();
        let Ok(real) = p.canonicalize() else {
            panic!("feature `embed`: {} is a dangling symlink", p.display());
        };
        if !real.starts_with(root) {
            panic!(
                "feature `embed`: {} points outside {}",
                p.display(),
                root.display()
            );
        }
        if real.is_dir() {
            if link {
                panic!("feature `embed`: {} is a symlinked directory", p.display());
            }
            check(root, &p);
        } else if !real.is_file() {
            panic!(
                "feature `embed`: {} is not a regular file (FIFO, socket or device)",
                p.display()
            );
        }
    }
}
