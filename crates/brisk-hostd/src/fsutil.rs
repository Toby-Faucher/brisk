//! Small file helpers shared by the stores.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Atomic 0600 write: a fsynced temp file in the same directory, then rename.
/// Creates the directory. A leftover temp file from a crashed run is removed first; a failure
/// removes the temp file.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::create_dir_all(dir)?;
    let _ = std::fs::remove_file(&tmp); // a leftover of a crashed run
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let r = f
        .write_all(bytes)
        .and_then(|_| f.sync_all())
        .and_then(|_| std::fs::rename(&tmp, path));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// Removes the entry with the lowest key from `by` until `m` holds at most `max`.
pub(crate) fn evict_oldest<K: Ord + Clone, V, O: Ord>(
    m: &mut std::collections::BTreeMap<K, V>,
    max: usize,
    by: impl Fn(&K, &V) -> O,
) {
    while m.len() > max {
        let Some(oldest) = m.iter().min_by_key(|(k, v)| by(k, v)).map(|(k, _)| k.clone()) else {
            return;
        };
        m.remove(&oldest);
    }
}

/// True when `s` is 1 to `max` bytes of `a-z`, `0-9` and `-`.
pub(crate) fn slug_ok(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn write_atomic_replaces_and_cleans_up() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/x.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(p.parent().unwrap()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn evict_drops_lowest() {
        let mut m: BTreeMap<&str, u64> = [("a", 3), ("b", 1), ("c", 2)].into();
        evict_oldest(&mut m, 1, |_, t| *t);
        assert_eq!(m.keys().collect::<Vec<_>>(), [&"a"]);
    }

    #[test]
    fn slug() {
        assert!(slug_ok("a-1", 3));
        assert!(!slug_ok("", 3));
        assert!(!slug_ok("abcd", 3));
        assert!(!slug_ok("A", 3));
        assert!(!slug_ok("a_b", 3));
    }
}
