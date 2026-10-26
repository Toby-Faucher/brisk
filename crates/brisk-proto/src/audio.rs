//! Sunshine audio packets and Reed-Solomon recovery (sans-io, no Opus decoding).
//!
//! Sources: Sunshine stream.cpp `audioBroadcastThread` / `encode_audio`, moonlight-common-c
//! AudioStream.c and RtpAudioQueue.c.
//!
//! Data packet: RTP header (12 bytes, big-endian: 0x80, payload type 97, seq u16, timestamp u32,
//! ssrc 0) then the Opus frame. With `SS_ENC_AUDIO` the frame is AES-128-CBC/PKCS7 under the
//! session `rikey` with IV = BE32(rikeyid + seq) in the first 4 bytes, rest zero.
//! FEC packet: RTP header (payload type 127, seq = last data seq + shard + 1) then
//! `AUDIO_FEC_HEADER` (12 bytes: shard index u8, payload type u8 = 97, base seq u16, base
//! timestamp u32, ssrc u32, big-endian) then the parity shard. Blocks are 4 data + 2 parity
//! shards starting at `seq % 4 == 0`; shards are the (encrypted) payload bytes, so recovery
//! happens before decryption. Sunshine replaces nanors' parity matrix with [`PARITY`].

use crate::fec::{rs_decode_matrix, rs_encode_matrix};
use aes::Aes128;
use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};

pub const RTP_HEADER: usize = 12;
pub const FEC_HEADER: usize = 12;
/// Sunshine `MAX_AUDIO_PACKET_SIZE`: payload bytes per packet.
pub const MAX_PAYLOAD: usize = 1400;
/// Encrypted payloads are padded up to the AES block size.
pub const MAX_DATAGRAM: usize = RTP_HEADER + FEC_HEADER + MAX_PAYLOAD.div_ceil(16) * 16;
pub const PT_AUDIO: u8 = 97;
pub const PT_FEC: u8 = 127;
pub const DATA_SHARDS: usize = 4;
pub const FEC_SHARDS: usize = 2;
/// Row-major `FEC_SHARDS x DATA_SHARDS` parity matrix from Sunshine / moonlight (OpenFEC's).
pub const PARITY: [u8; 8] = [0x77, 0x40, 0x38, 0x0e, 0xc7, 0xa7, 0x0d, 0x6c];
/// FEC blocks tracked at once; older ones are given up on.
const MAX_BLOCKS: usize = 4;
const MAX_LOOKAHEAD: u16 = 1024;
/// Packets more than `MAX_LOOKAHEAD` ahead of, or behind, the oldest open block are out of
/// window; this many in a row (host restart, outage longer than the window, either direction in
/// u16 space) make the queue resync to the new position. A lone stray cannot trigger it
/// (8 packets is 40 ms at 5 ms packets). Packets at most `MAX_LOOKAHEAD` behind are plain late
/// duplicates and neither count nor resync.
// shortcut: an on-path injector can force a resync with 8 consecutive far packets; accepted on a LAN, add authentication of the stream position if that ever matters.
const RESYNC_AFTER: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioError {
    TooLarge,
    Truncated,
    Malformed,
    /// Ciphertext is not a whole number of AES blocks or the padding is wrong (wrong key/IV or
    /// tampering).
    BadPadding,
    /// Shard size differs from its block's, so Reed-Solomon cannot use it.
    BlockSize,
    /// Sequence number too far ahead of the open blocks.
    OutOfWindow,
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AudioError::TooLarge => "audio packet too large",
            AudioError::Truncated => "truncated audio packet",
            AudioError::Malformed => "malformed audio packet",
            AudioError::BadPadding => "audio packet failed decryption",
            AudioError::BlockSize => "audio shard size mismatch within a block",
            AudioError::OutOfWindow => "audio sequence number out of window",
        })
    }
}

impl std::error::Error for AudioError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusFrame {
    pub seq: u16,
    pub ts: u32,
    pub data: Vec<u8>,
}

/// An audio data packet's RTP fields plus its (still encrypted) payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioPayload {
    pub seq: u16,
    pub ts: u32,
    pub payload: Vec<u8>,
}

fn be16(d: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([d[at], d[at + 1]])
}

fn be32(d: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(d[at..at + 4].try_into().unwrap())
}

fn check_rtp(d: &[u8], min: usize) -> Result<(), AudioError> {
    if d.len() > MAX_DATAGRAM {
        return Err(AudioError::TooLarge);
    }
    if d.len() < min {
        return Err(AudioError::Truncated);
    }
    if d[0] & 0xC0 != 0x80 {
        return Err(AudioError::Malformed);
    }
    Ok(())
}

/// Parses a data packet (payload type 97) without decrypting it.
pub fn parse_audio_payload(d: &[u8]) -> Result<AudioPayload, AudioError> {
    check_rtp(d, RTP_HEADER + 1)?;
    if d[1] & 0x7f != PT_AUDIO {
        return Err(AudioError::Malformed);
    }
    Ok(AudioPayload {
        seq: be16(d, 2),
        ts: be32(d, 4),
        payload: d[RTP_HEADER..].to_vec(),
    })
}

/// AES-128-CBC audio decryption keyed with the session `rikey` / `rikeyid`.
pub struct AudioCipher {
    aes: Aes128,
    key_id: u32,
}

impl AudioCipher {
    /// `rikeyid` is the first 4 bytes of the launch `rikeyid` IV read big-endian (Sunshine `avRiKeyId`).
    pub fn new(rikey: &[u8; 16], rikeyid: u32) -> Self {
        Self {
            aes: Aes128::new(rikey.into()),
            key_id: rikeyid,
        }
    }

    /// Decrypts one payload; the IV depends on the packet's sequence number.
    pub fn open(&self, p: &AudioPayload) -> Result<OpusFrame, AudioError> {
        let ct = &p.payload;
        if ct.is_empty() || !ct.len().is_multiple_of(16) || ct.len() > MAX_PAYLOAD.div_ceil(16) * 16
        {
            return Err(AudioError::BadPadding);
        }
        let mut prev = [0u8; 16];
        prev[..4].copy_from_slice(&self.key_id.wrapping_add(p.seq as u32).to_be_bytes());
        let mut out = Vec::with_capacity(ct.len());
        for c in ct.chunks(16) {
            let mut b = GenericArray::clone_from_slice(c);
            self.aes.decrypt_block(&mut b);
            out.extend(b.iter().zip(prev).map(|(x, y)| x ^ y));
            prev.copy_from_slice(c);
        }
        let pad = *out.last().unwrap() as usize;
        if pad == 0 || pad > 16 || out[out.len() - pad..].iter().any(|&b| b as usize != pad) {
            return Err(AudioError::BadPadding);
        }
        out.truncate(out.len() - pad);
        Ok(OpusFrame {
            seq: p.seq,
            ts: p.ts,
            data: out,
        })
    }
}

/// Parses and decrypts one `SS_ENC_AUDIO` data packet.
pub fn parse_audio_packet(
    d: &[u8],
    rikey: &[u8; 16],
    rikeyid: u32,
) -> Result<OpusFrame, AudioError> {
    AudioCipher::new(rikey, rikeyid).open(&parse_audio_payload(d)?)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioFecStats {
    pub recovered: u64,
    /// Data packets never received nor recovered when their block was given up on.
    pub lost: u64,
}

struct Block {
    base: u16,
    base_ts: u32,
    len: usize,
    /// Data shards then parity shards.
    shards: [Option<Vec<u8>>; DATA_SHARDS + FEC_SHARDS],
    emitted: [bool; DATA_SHARDS],
    done: bool,
}

impl Block {
    /// Data packets neither received nor recovered.
    fn missing(&self) -> u64 {
        (0..DATA_SHARDS)
            .filter(|&i| !self.emitted[i] && self.shards[i].is_none())
            .count() as u64
    }
}

/// Groups data and FEC packets into 4+2 blocks, passes data packets through at once and
/// recovers missing ones as soon as 4 of the 6 shards are in.
pub struct AudioFec {
    blocks: Vec<Block>,
    /// Blocks starting before this are finished with.
    floor: Option<u16>,
    /// Consecutive out-of-window packets seen.
    stray: u8,
    /// Shard size of recent blocks.
    std_len: Option<usize>,
    /// RTP timestamp step per packet (`x-nv-aqos.packetDuration`).
    duration: u32,
    stats: AudioFecStats,
}

fn is_before16(a: u16, b: u16) -> bool {
    (a.wrapping_sub(b) as i16) < 0
}

impl AudioFec {
    pub fn new(packet_duration: u32) -> Self {
        Self {
            blocks: Vec::new(),
            floor: None,
            stray: 0,
            std_len: None,
            duration: packet_duration,
            stats: AudioFecStats::default(),
        }
    }

    pub fn stats(&self) -> AudioFecStats {
        self.stats
    }

    /// Feeds one datagram (data or FEC); returns the data packets that became available, in
    /// sequence order, each at most once.
    pub fn push(&mut self, d: &[u8]) -> Result<Vec<AudioPayload>, AudioError> {
        check_rtp(d, RTP_HEADER + 1)?;
        let (seq, ts) = (be16(d, 2), be32(d, 4));
        // (block base, base ts, shard slot, shard bytes)
        let (base, base_ts, slot, shard) = match d[1] & 0x7f {
            PT_AUDIO => {
                let i = (seq % DATA_SHARDS as u16) as usize;
                (
                    seq - i as u16,
                    ts.wrapping_sub(i as u32 * self.duration),
                    i,
                    &d[RTP_HEADER..],
                )
            }
            PT_FEC => {
                if d.len() <= RTP_HEADER + FEC_HEADER {
                    return Err(AudioError::Truncated);
                }
                let idx = d[RTP_HEADER] as usize;
                let base = be16(d, RTP_HEADER + 2);
                if idx >= FEC_SHARDS || !base.is_multiple_of(DATA_SHARDS as u16) {
                    return Err(AudioError::Malformed);
                }
                (
                    base,
                    be32(d, RTP_HEADER + 4),
                    DATA_SHARDS + idx,
                    &d[RTP_HEADER + FEC_HEADER..],
                )
            }
            _ => return Err(AudioError::Malformed),
        };

        let floor = *self.floor.get_or_insert(base);
        let behind = floor.wrapping_sub(base);
        if behind != 0 && behind <= MAX_LOOKAHEAD {
            return Ok(Vec::new()); // late packet of a block already finished with
        }
        if base.wrapping_sub(floor) > MAX_LOOKAHEAD {
            self.stray += 1;
            if self.stray < RESYNC_AFTER {
                return Err(AudioError::OutOfWindow);
            }
            // Resync: give up on everything open and continue from here.
            for b in self.blocks.drain(..) {
                self.stats.lost += b.missing();
            }
            self.floor = Some(base);
            // The restarted stream may use another packet size.
            self.std_len = None;
            self.stray = 0;
        } else {
            self.stray = 0;
        }
        let bi = match self.blocks.iter().position(|b| b.base == base) {
            Some(i) => i,
            None => {
                if self.blocks.len() == MAX_BLOCKS {
                    let Some(oldest) =
                        (0..MAX_BLOCKS).max_by_key(|&i| base.wrapping_sub(self.blocks[i].base))
                    else {
                        return Ok(Vec::new()); // unreachable: MAX_BLOCKS > 0
                    };
                    let b = self.blocks.remove(oldest);
                    self.stats.lost += b.missing();
                    let floor = b.base.wrapping_add(DATA_SHARDS as u16);
                    self.floor = Some(floor);
                    if is_before16(base, floor) {
                        return Ok(Vec::new());
                    }
                }
                self.blocks.push(Block {
                    base,
                    base_ts,
                    // A one-off odd-sized packet must not set the size for its block.
                    len: self.std_len.unwrap_or(shard.len()),
                    shards: Default::default(),
                    emitted: [false; DATA_SHARDS],
                    done: false,
                });
                self.blocks.len() - 1
            }
        };
        let b = &mut self.blocks[bi];
        if b.len != shard.len() {
            // Like moonlight, give up on FEC for this block but still pass data through.
            if slot < DATA_SHARDS && !b.emitted[slot] {
                b.emitted[slot] = true;
                return Ok(vec![AudioPayload {
                    seq,
                    ts,
                    payload: shard.to_vec(),
                }]);
            }
            return Err(AudioError::BlockSize);
        }
        self.std_len = Some(b.len);
        if b.shards[slot].is_some() || (slot < DATA_SHARDS && b.emitted[slot]) {
            return Ok(Vec::new()); // duplicate
        }
        let mut out = Vec::new();
        b.shards[slot] = Some(shard.to_vec());
        if slot < DATA_SHARDS {
            b.emitted[slot] = true;
            out.push(AudioPayload {
                seq,
                ts,
                payload: shard.to_vec(),
            });
        }
        let have = b.shards.iter().flatten().count();
        if !b.done && have >= DATA_SHARDS {
            b.done = true;
            let missing: Vec<usize> = (0..DATA_SHARDS).filter(|&i| !b.emitted[i]).collect();
            if !missing.is_empty()
                && rs_decode_matrix(DATA_SHARDS, FEC_SHARDS, &PARITY, &mut b.shards).is_ok()
            {
                for i in missing {
                    b.emitted[i] = true;
                    self.stats.recovered += 1;
                    out.push(AudioPayload {
                        seq: b.base.wrapping_add(i as u16),
                        ts: b.base_ts.wrapping_add(i as u32 * self.duration),
                        payload: b.shards[i].clone().unwrap(),
                    });
                }
            }
        }
        out.sort_by_key(|p| p.seq.wrapping_sub(base));
        Ok(out)
    }
}

/// Fills the two parity shards of `shards` (4 data shards of equal length first); the host-side
/// operation, used by tests to build blocks.
pub fn encode_parity(shards: &mut [Vec<u8>]) -> Result<(), crate::fec::FecError> {
    rs_encode_matrix(DATA_SHARDS, FEC_SHARDS, &PARITY, shards)
}
