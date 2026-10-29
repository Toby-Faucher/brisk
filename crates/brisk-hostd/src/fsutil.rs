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

/// Creates `path` with `bytes` (0600), complete or not at all, and never replaces a file already
/// there (`AlreadyExists`). A crash leaves no partial file: the bytes are fsynced in a temp file
/// first, then linked into place.
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut f = tempfile::NamedTempFile::new_in(dir)?; // mode 0600
    f.write_all(bytes)?;
    f.as_file().sync_all()?;
    f.persist_noclobber(path).map(drop).map_err(|e| e.error)
}

/// Copies a file we could not use to `<path>.corrupt`, once: an existing copy is kept, so the first
/// bad version survives later bad saves. The caller goes on with defaults.
pub(crate) fn set_aside(path: &Path, why: &dyn std::fmt::Display) {
    let mut name = path.as_os_str().to_owned();
    name.push(".corrupt");
    let copy = std::path::PathBuf::from(name);
    if copy.exists() {
        tracing::debug!(
            "{} is unusable ({why}); {} already kept",
            path.display(),
            copy.display()
        );
        return;
    }
    match std::fs::copy(path, &copy) {
        Ok(_) => tracing::warn!(
            "{} is unusable ({why}); a copy is kept as {}",
            path.display(),
            copy.display()
        ),
        Err(e) => tracing::warn!(
            "{} is unusable ({why}); could not keep a copy: {e}",
            path.display()
        ),
    }
}

/// Reads a JSON file. Missing is the default, quietly. Unreadable or unparseable is the default too,
/// but the file is first copied aside (`set_aside`) because the next save would overwrite it.
pub(crate) fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
            set_aside(path, &e);
            T::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => T::default(),
        Err(e) => {
            set_aside(path, &e);
            T::default()
        }
    }
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
    fn write_new_never_replaces() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/s");
        write_new(&p, b"one").unwrap();
        let e = write_new(&p, b"two").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&p).unwrap(), b"one");
        // no temp file is left behind either way
        assert_eq!(std::fs::read_dir(p.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn bad_json_is_set_aside_once_and_a_missing_file_is_quiet() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.json");
        assert!(read_json_or_default::<Vec<u8>>(&p).is_empty());
        assert!(!d.path().join("x.json.corrupt").exists());
        std::fs::write(&p, "{oops").unwrap();
        assert!(read_json_or_default::<Vec<u8>>(&p).is_empty());
        std::fs::write(&p, "{second").unwrap();
        assert!(read_json_or_default::<Vec<u8>>(&p).is_empty());
        let kept = std::fs::read_to_string(d.path().join("x.json.corrupt")).unwrap();
        assert_eq!(kept, "{oops");
        std::fs::write(&p, "[1,2]").unwrap();
        assert_eq!(read_json_or_default::<Vec<u8>>(&p), [1, 2]);
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
