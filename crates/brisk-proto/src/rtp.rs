//! Sunshine video packets and frame reassembly (sans-io).
//!
//! Wire layout per packet (Sunshine stream.cpp `video_packet_raw_t`, moonlight-common-c Video.h):
//! RTP header (12 bytes, big-endian; `header` = 0x90, extension bit always set) + 4 reserved
//! bytes, then `NV_VIDEO_PACKET` (16 bytes, little-endian): streamPacketIndex u32, frameIndex u32,
//! flags u8, extraFlags u8, multiFecFlags u8, multiFecBlocks u8 (bits 4-5 block index, bits 6-7
//! last block index), fecInfo u32 (bits 22-31 data shards, 12-21 shard index, 4-11 FEC percent).
//! Every packet of a block is one Reed-Solomon shard of `packetsize + 16` bytes taken over the
//! whole packet; Sunshine rewrites the RTP header, frameIndex, multiFecBlocks and fecInfo after
//! encoding, so recovered data shards are only trusted for flags and payload (as Moonlight does).
//! The first packet of a frame carries the 8-byte `video_short_frame_header_t` before the
//! Annex B bytes: headerType 0x01, frame_processing_latency u16 LE (0.1 ms), frameType u8
//! (2 = IDR), lastPayloadLen u16 LE, 2 reserved.
//!
//! With `SS_ENC_VIDEO` each datagram is a 32-byte prefix (iv[12], frameNumber u32 LE, tag[16])
//! followed by the AES-128-GCM ciphertext (no AAD) of the plain packet; [`VideoCipher::open`]
//! turns it back into a plain packet.

use crate::fec::{rs_decode, MAX_SHARDS};
use crate::sdp::SS_ENC_VIDEO;
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce, Tag};

/// RTP fixed header + the 4-byte extension Sunshine always sends.
pub const RTP_HEADER: usize = 16;
/// `NV_VIDEO_PACKET`.
pub const NV_HEADER: usize = 16;
/// Bytes before the payload of a plain packet.
pub const HEADER: usize = RTP_HEADER + NV_HEADER;
/// `video_packet_enc_prefix_t`.
pub const ENC_PREFIX: usize = 32;
/// Plain packets above this are rejected (Moonlight packet sizes are ~1 KiB-1.4 KiB).
pub const MAX_VIDEO_PACKET: usize = 4096;
/// Frames kept open at once waiting for late shards; older ones are evicted.
pub const MAX_PENDING_FRAMES: usize = 3;

pub const FLAG_CONTAINS_PIC_DATA: u8 = 0x1;
pub const FLAG_EOF: u8 = 0x2;
pub const FLAG_SOF: u8 = 0x4;
const FLAG_EXTENSION: u8 = 0x10;
/// Sunshine sends up to 4 FEC blocks per frame (2-bit counters).
const MAX_FEC_BLOCKS: usize = 4;
const SHORT_FRAME_HEADER: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtpError {
    TooLarge,
    Truncated,
    Malformed,
    /// Encrypted bytes handed to the plain parser; decrypt with [`VideoCipher::open`] first.
    Encrypted,
    BadTag,
}

impl std::fmt::Display for RtpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RtpError::TooLarge => "video packet too large",
            RtpError::Truncated => "truncated video packet",
            RtpError::Malformed => "malformed video packet",
            RtpError::Encrypted => "encrypted video packet passed to the plain parser",
            RtpError::BadTag => "video packet failed authentication",
        })
    }
}

impl std::error::Error for RtpError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoPacket<'a> {
    pub seq: u16,
    pub ts: u32,
    pub frame_idx: u32,
    /// Index within the FEC block (fecInfo bits 12-21); data shards come first.
    pub shard_idx: u16,
    pub is_parity: bool,
    /// `NV_VIDEO_PACKET.flags` (`FLAG_*`).
    pub flags: u8,
    /// Bytes after the 32-byte header (includes the frame header on a frame's first packet).
    pub payload: &'a [u8],
    /// Sunshine frame processing latency in 0.1 ms, on a frame's first data packet only.
    pub host_latency_100us: Option<u16>,
    pub data_shards: u16,
    pub parity_shards: u16,
    pub fec_percent: u8,
    pub fec_block: u8,
    pub last_fec_block: u8,
    /// The whole packet: the Reed-Solomon shard.
    pub raw: &'a [u8],
}

fn le32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

fn is_before32(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}

/// `(latency_100us, frame_type, last_payload_len)` from a frame's first payload. Sunshine sends
/// latency 0 for repeated frames or when it has no measurement, so 0 becomes `None`.
fn frame_header(payload: &[u8]) -> Option<(Option<u16>, u8, u16)> {
    match payload {
        [0x01, l0, l1, ty, p0, p1, _, _, ..] => {
            let latency = Some(u16::from_le_bytes([*l0, *l1])).filter(|&l| l != 0);
            Some((latency, *ty, u16::from_le_bytes([*p0, *p1])))
        }
        _ => None,
    }
}

/// Parses one plain video packet. `enc_flags` are the negotiated `SS_ENC_*` bits: with
/// `SS_ENC_VIDEO` set the datagram is ciphertext and this returns [`RtpError::Encrypted`].
pub fn parse_video_packet(d: &[u8], enc_flags: u32) -> Result<VideoPacket<'_>, RtpError> {
    if enc_flags & SS_ENC_VIDEO != 0 {
        return Err(RtpError::Encrypted);
    }
    if d.len() > MAX_VIDEO_PACKET {
        return Err(RtpError::TooLarge);
    }
    if d.len() <= HEADER {
        return Err(RtpError::Truncated);
    }
    if d[0] & 0xC0 != 0x80 || d[0] & FLAG_EXTENSION == 0 {
        return Err(RtpError::Malformed);
    }
    let fec = le32(d, 28);
    let data_shards = (fec >> 22) as u16;
    let shard_idx = ((fec >> 12) & 0x3ff) as u16;
    let fec_percent = (fec >> 4) as u8;
    // moonlight RtpVideoQueue.c derives the parity count the same way.
    let parity_shards = (data_shards as u32 * fec_percent as u32).div_ceil(100) as u16;
    let total = (data_shards + parity_shards) as usize;
    let blocks = d[27];
    let (fec_block, last_fec_block) = ((blocks >> 4) & 3, blocks >> 6);
    if data_shards == 0
        || shard_idx as usize >= total
        || (parity_shards > 0 && total > MAX_SHARDS)
        || fec_block > last_fec_block
    {
        return Err(RtpError::Malformed);
    }
    let is_parity = shard_idx >= data_shards;
    let flags = d[24];
    let payload = &d[HEADER..];
    let first = !is_parity && fec_block == 0 && flags & FLAG_SOF != 0;
    Ok(VideoPacket {
        seq: u16::from_be_bytes([d[2], d[3]]),
        ts: u32::from_be_bytes([d[4], d[5], d[6], d[7]]),
        frame_idx: le32(d, 20),
        shard_idx,
        is_parity,
        flags,
        payload,
        host_latency_100us: first
            .then(|| frame_header(payload))
            .flatten()
            .and_then(|h| h.0),
        data_shards,
        parity_shards,
        fec_percent,
        fec_block,
        last_fec_block,
        raw: d,
    })
}

/// AES-128-GCM video decryption keyed with the session's `rikey` (Sunshine `launch_session.gcm_key`).
pub struct VideoCipher {
    gcm: Aes128Gcm,
}

impl VideoCipher {
    pub fn new(key: &[u8; 16]) -> Self {
        Self {
            gcm: Aes128Gcm::new(key.into()),
        }
    }

    /// Decrypts one `SS_ENC_VIDEO` datagram into the plain packet for [`parse_video_packet`].
    /// The IV is taken from the prefix (Sunshine fills it with a LE u64 counter and `iv[11] = 'V'`).
    pub fn open(&self, d: &[u8]) -> Result<Vec<u8>, RtpError> {
        if d.len() > MAX_VIDEO_PACKET + ENC_PREFIX {
            return Err(RtpError::TooLarge);
        }
        if d.len() <= ENC_PREFIX + HEADER {
            return Err(RtpError::Truncated);
        }
        let mut out = d[ENC_PREFIX..].to_vec();
        self.gcm
            .decrypt_in_place_detached(
                Nonce::from_slice(&d[..12]),
                &[],
                &mut out,
                Tag::from_slice(&d[16..32]),
            )
            .map_err(|_| RtpError::BadTag)?;
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledFrame {
    pub idx: u32,
    pub is_idr: bool,
    pub annexb: Vec<u8>,
    pub host_latency_100us: Option<u16>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssemblerStats {
    pub frames_ok: u64,
    /// Frame indices skipped when a later frame completed (lost, unrecoverable or corrupt).
    pub frames_dropped: u64,
    /// Frames that needed Reed-Solomon recovery.
    pub fec_recovered: u64,
    /// Packets inconsistent with their block, and frames whose recovered data was insane.
    pub parse_errors: u64,
}

struct Block {
    data: u16,
    parity: u16,
    base_seq: u16,
    len: usize,
    shards: Vec<Option<Vec<u8>>>,
    got: u16,
    got_data: u16,
    done: bool,
    recovered: bool,
}

struct Frame {
    idx: u32,
    last_block: u8,
    blocks: [Option<Block>; MAX_FEC_BLOCKS],
}

/// Collects shards per frame index, runs FEC per block and yields complete frames in order.
#[derive(Default)]
pub struct FrameAssembler {
    pending: Vec<Frame>,
    /// First frame index still wanted (initially the first packet's); older packets are stale.
    next: Option<u32>,
    stats: AssemblerStats,
}

impl FrameAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn stats(&self) -> AssemblerStats {
        self.stats
    }

    pub fn push(&mut self, p: VideoPacket) -> Vec<AssembledFrame> {
        let next = *self.next.get_or_insert(p.frame_idx);
        if is_before32(p.frame_idx, next) {
            return Vec::new();
        }
        let fi = match self.pending.iter().position(|f| f.idx == p.frame_idx) {
            Some(i) => i,
            None => {
                if self.pending.len() == MAX_PENDING_FRAMES {
                    // shortcut: evicts the oldest open frame even if the new packet is older still; fine for in-order hosts.
                    let oldest = (0..self.pending.len())
                        .max_by_key(|&i| p.frame_idx.wrapping_sub(self.pending[i].idx))
                        .unwrap();
                    self.pending.remove(oldest);
                }
                self.pending.push(Frame {
                    idx: p.frame_idx,
                    last_block: p.last_fec_block,
                    blocks: Default::default(),
                });
                self.pending.len() - 1
            }
        };
        let frame = &mut self.pending[fi];
        if frame.last_block != p.last_fec_block {
            self.stats.parse_errors += 1;
            return Vec::new();
        }
        let base_seq = p.seq.wrapping_sub(p.shard_idx);
        let block = frame.blocks[p.fec_block as usize].get_or_insert_with(|| Block {
            data: p.data_shards,
            parity: p.parity_shards,
            base_seq,
            len: p.raw.len(),
            shards: vec![None; (p.data_shards + p.parity_shards) as usize],
            got: 0,
            got_data: 0,
            done: false,
            recovered: false,
        });
        if block.data != p.data_shards
            || block.parity != p.parity_shards
            || block.base_seq != base_seq
            || block.len != p.raw.len()
        {
            self.stats.parse_errors += 1;
            return Vec::new();
        }
        if block.done || block.shards[p.shard_idx as usize].is_some() {
            return Vec::new(); // duplicate or no longer needed
        }
        block.shards[p.shard_idx as usize] = Some(p.raw.to_vec());
        block.got += 1;
        block.got_data += u16::from(!p.is_parity);
        if block.got < block.data {
            return Vec::new();
        }
        if block.got_data < block.data {
            if rs_decode(
                block.data as usize,
                block.parity as usize,
                &mut block.shards,
            )
            .is_err()
            {
                self.stats.parse_errors += 1;
                return Vec::new();
            }
            block.recovered = true;
        }
        block.done = true;
        block.shards.truncate(block.data as usize);
        if !frame.blocks[..=frame.last_block as usize]
            .iter()
            .all(|b| b.as_ref().is_some_and(|b| b.done))
        {
            return Vec::new();
        }

        let frame = self.pending.remove(fi);
        let idx = frame.idx;
        self.stats.frames_dropped += idx.wrapping_sub(next) as u64;
        self.pending.retain(|f| is_before32(idx, f.idx));
        self.next = Some(idx.wrapping_add(1));
        match finish(frame) {
            Some((out, recovered)) => {
                self.stats.frames_ok += 1;
                self.stats.fec_recovered += u64::from(recovered);
                vec![out]
            }
            None => {
                self.stats.frames_dropped += 1;
                self.stats.parse_errors += 1;
                Vec::new()
            }
        }
    }
}

/// Concatenates a frame's data payloads; `None` if the flags or frame header are inconsistent.
fn finish(frame: Frame) -> Option<(AssembledFrame, bool)> {
    let mut recovered = false;
    let mut payloads: Vec<&[u8]> = Vec::new();
    for b in frame.blocks.iter().flatten() {
        recovered |= b.recovered;
        let n = b.shards.len();
        for (i, s) in b.shards.iter().enumerate() {
            let s = s.as_deref()?;
            let fl = s[24];
            // moonlight RtpVideoQueue.c sanity checks on (recovered) data shards.
            if fl & !(FLAG_SOF | FLAG_EOF | FLAG_CONTAINS_PIC_DATA) != 0
                || fl & FLAG_CONTAINS_PIC_DATA == 0
                || (i == 0 && fl & FLAG_SOF == 0)
                || (i == n - 1 && fl & FLAG_EOF == 0)
            {
                return None;
            }
            payloads.push(&s[HEADER..]);
        }
    }
    let (latency, ty, last_len) = frame_header(payloads[0])?;
    // Only the frame's final packet is short; lastPayloadLen is its exact payload length.
    let last = payloads.last_mut()?;
    if last_len as usize <= last.len() && last_len != 0 {
        *last = &last[..last_len as usize];
    }
    let mut annexb: Vec<u8> = payloads.concat();
    if annexb.len() < SHORT_FRAME_HEADER {
        return None;
    }
    annexb.drain(..SHORT_FRAME_HEADER);
    Some((
        AssembledFrame {
            idx: frame.idx,
            is_idr: ty == 2,
            annexb,
            host_latency_100us: latency,
        },
        recovered,
    ))
}
