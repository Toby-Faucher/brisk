//! Shared by the targets (`#[path = "common.rs"] mod common;`).

/// Splits fuzz input into datagrams: `len u16 LE` then `len` bytes (cut short at the end), so one
/// input drives a stateful parser through a sequence. Seeds use the same framing.
#[allow(dead_code)]
pub fn chunks(mut d: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    while d.len() >= 2 {
        let n = (u16::from_le_bytes([d[0], d[1]]) as usize).min(d.len() - 2);
        out.push(&d[2..2 + n]);
        d = &d[2 + n..];
    }
    out
}

#[allow(dead_code)]
pub const KEY: [u8; 16] = *b"0123456789abcdef";
