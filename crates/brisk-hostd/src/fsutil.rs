//! Small file helpers shared by the stores.

use std::io::Write;
use std::path::Path;

/// Atomic 0600 write: a fsynced temp file in the same directory, then rename.
/// Creates the directory. Each call gets its own temp file, and a failure removes it.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut f = tempfile::NamedTempFile::new_in(dir)?; // mode 0600
    f.write_all(bytes)?;
    f.as_file().sync_all()?;
    f.persist(path).map(drop).map_err(|e| e.error)
}

/// Removes the entry with the lowest key from `by` until `m` holds at most `max`.
pub(crate) fn evict_oldest<K: Ord + Clone, V, O: Ord>(
    m: &mut std::collections::BTreeMap<K, V>,
    max: usize,
    by: impl Fn(&K, &V) -> O,
) {
    while m.len() > max {
        let Some(oldest) = m
            .iter()
            .min_by_key(|(k, v)| by(k, v))
            .map(|(k, _)| k.clone())
        else {
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
