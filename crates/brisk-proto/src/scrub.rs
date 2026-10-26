//! Fixture scrubber: blank secrets out of captured bytes before they are written to disk.

/// Replaces every occurrence of every secret with `0x00` filler of the same length.
/// Empty secrets are ignored. Overlapping occurrences are all covered.
// shortcut: O(n * secrets) scan, fine for fixture-sized buffers; a secret made only of 0x00 survives.
pub fn scrub(bytes: &[u8], secrets: &[Vec<u8>]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for s in secrets
        .iter()
        .filter(|s| !s.is_empty() && s.len() <= bytes.len())
    {
        for (i, w) in bytes.windows(s.len()).enumerate() {
            if w == s.as_slice() {
                out[i..i + s.len()].fill(0);
            }
        }
    }
    out
}

/// Like [`scrub`], but also scrubs each secret's lowercase and uppercase hex text
/// (rikey travels as hex in HTTP query strings). Empty secrets are ignored.
pub fn scrub_with_encodings(bytes: &[u8], secrets: &[Vec<u8>]) -> Vec<u8> {
    let mut all = secrets.to_vec();
    for s in secrets.iter().filter(|s| !s.is_empty()) {
        let lower: String = s.iter().map(|b| format!("{b:02x}")).collect();
        all.push(lower.to_uppercase().into_bytes());
        all.push(lower.into_bytes());
    }
    scrub(bytes, &all)
}
