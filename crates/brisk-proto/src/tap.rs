//! Brisk tap framing: one SEQPACKET message per encoded frame from the patched Sunshine
//! (`brisk_tap_send`, patches/sunshine/0001-brisk-tap.patch), a little-endian header followed by
//! the Annex B bytes (research/m2-tap-design.md section 2).
//!
//! ```text
//! off size field
//! 0   4   magic 'BTAP'
//! 4   1   version = 1
//! 5   1   flags: b0 idr, b1 after_rfi, b2 no_capture_ts, b3 gap
//! 6   2   header_len (>= 44; unknown trailing header bytes are skipped)
//! 8   4   connect_data (X-SS-Connect-Data of the session)
//! 12  4   codec (0 H.264, 1 HEVC, 2 AV1)
//! 16  8   frame_index
//! 24  8   capture_ns (CLOCK_MONOTONIC, 0 if none)
//! 32  8   tap_ns (CLOCK_MONOTONIC at send)
//! 40  4   dropped_total
//! ```

/// First 4 bytes of every message.
pub const MAGIC: [u8; 4] = *b"BTAP";
/// Header version this parser reads.
pub const VERSION: u8 = 1;
/// Size of the fixed header.
pub const HEADER_LEN: usize = 44;
/// Largest Annex B payload accepted (a 1080p IDR is about 100-500 KB \[U\]).
pub const MAX_PAYLOAD: usize = 4 << 20;

/// The frame is an IDR.
pub const FLAG_IDR: u8 = 0x01;
/// The frame follows a reference-frame invalidation.
pub const FLAG_AFTER_RFI: u8 = 0x02;
/// `capture_ns` is not a real capture time.
pub const FLAG_NO_CAPTURE_TS: u8 = 0x04;
/// Frames were dropped (socket full) before this one; it is the recovery IDR.
pub const FLAG_GAP: u8 = 0x08;

/// The fixed header of one tap message, as parsed by [`parse_tap`]; the field layout is in the
/// module docs above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TapHeader {
    /// `FLAG_*` bits.
    pub flags: u8,
    /// Header bytes; the payload starts here.
    pub header_len: u16,
    /// `X-SS-Connect-Data` of the session.
    pub connect_data: u32,
    /// 0 H.264, 1 HEVC, 2 AV1.
    pub codec: u32,
    /// Encoder frame index.
    pub frame_index: u64,
    /// Capture time, `CLOCK_MONOTONIC` ns (0 if none).
    pub capture_ns: u64,
    /// Send time, `CLOCK_MONOTONIC` ns.
    pub tap_ns: u64,
    /// Frames the tap dropped in total.
    pub dropped_total: u32,
}

impl TapHeader {
    /// The frame is an IDR ([`FLAG_IDR`]).
    pub fn idr(&self) -> bool {
        self.flags & FLAG_IDR != 0
    }
    /// The frame follows a reference-frame invalidation ([`FLAG_AFTER_RFI`]).
    pub fn after_rfi(&self) -> bool {
        self.flags & FLAG_AFTER_RFI != 0
    }
    /// `capture_ns` is not a real capture time ([`FLAG_NO_CAPTURE_TS`]).
    pub fn no_capture_ts(&self) -> bool {
        self.flags & FLAG_NO_CAPTURE_TS != 0
    }
    /// Frames were dropped before this one ([`FLAG_GAP`]).
    pub fn gap(&self) -> bool {
        self.flags & FLAG_GAP != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Why a tap message was rejected.
pub enum TapError {
    /// Fewer than [`HEADER_LEN`] bytes.
    Short(usize),
    /// First bytes are not [`MAGIC`].
    BadMagic,
    /// Header version other than [`VERSION`].
    BadVersion(u8),
    /// `header_len` below 44 or past the end of the message.
    BadHeaderLen(u16),
    /// Payload bytes over [`MAX_PAYLOAD`].
    TooLarge(usize),
}

impl std::fmt::Display for TapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TapError::Short(n) => write!(f, "tap message of {n} bytes is shorter than its header"),
            TapError::BadMagic => f.write_str("tap message without BTAP magic"),
            TapError::BadVersion(v) => write!(f, "tap version {v} (want 1)"),
            TapError::BadHeaderLen(n) => write!(f, "tap header_len {n} out of range"),
            TapError::TooLarge(n) => write!(f, "tap payload of {n} bytes over the 4 MiB cap"),
        }
    }
}

impl std::error::Error for TapError {}

fn le<const N: usize>(b: &[u8], off: usize) -> [u8; N] {
    b[off..off + N].try_into().unwrap()
}

/// Header and Annex B payload of one tap message.
pub fn parse_tap(buf: &[u8]) -> Result<(TapHeader, &[u8]), TapError> {
    if buf.len() < HEADER_LEN {
        return Err(TapError::Short(buf.len()));
    }
    if buf[..4] != MAGIC {
        return Err(TapError::BadMagic);
    }
    if buf[4] != VERSION {
        return Err(TapError::BadVersion(buf[4]));
    }
    let header_len = u16::from_le_bytes(le(buf, 6));
    if (header_len as usize) < HEADER_LEN || header_len as usize > buf.len() {
        return Err(TapError::BadHeaderLen(header_len));
    }
    let payload = &buf[header_len as usize..];
    if payload.len() > MAX_PAYLOAD {
        return Err(TapError::TooLarge(payload.len()));
    }
    let h = TapHeader {
        flags: buf[5],
        header_len,
        connect_data: u32::from_le_bytes(le(buf, 8)),
        codec: u32::from_le_bytes(le(buf, 12)),
        frame_index: u64::from_le_bytes(le(buf, 16)),
        capture_ns: u64::from_le_bytes(le(buf, 24)),
        tap_ns: u64::from_le_bytes(le(buf, 32)),
        dropped_total: u32::from_le_bytes(le(buf, 40)),
    };
    Ok((h, payload))
}
