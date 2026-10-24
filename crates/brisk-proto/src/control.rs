//! Gen7Enc control-stream messages (Sunshine 7.1.431, `x-nv-general.useReliableUdp:13` =
//! `controlProtocolType` 13, `SS_ENC_CONTROL_V2`), carried as ENet packets.
//!
//! Every message is sealed: `encryptedHeaderType u16 LE = 0x0001`, `length u16 LE` (= 4 + 16 +
//! plaintext), `seq u32 LE`, GCM tag (16), ciphertext. The plaintext is `type u16 LE`,
//! `payloadLength u16 LE`, payload (Sunshine stream.cpp `control_encrypted_t`,
//! `control_header_v2`; moonlight ControlStream.c `NVCTL_ENCRYPTED_PACKET_HEADER`). AES-128-GCM
//! keyed by the launch `rikey`, no AAD, 12-byte IV = `seq` LE in bytes 0..4, zeros, then
//! `'C','C'` (client to host) or `'H','C'` (host to client) in bytes 10..11
//! (stream.cpp `encode_control` / `IDX_ENCRYPTED` handler). Each direction counts from 0.

use crate::input::{encode_input, InputError, InputEvent};
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce, Tag};

/// Packet types: Sunshine stream.cpp `packetTypes`, moonlight `packetTypesGen7Enc`.
pub mod ptype {
    pub const ENCRYPTED: u16 = 0x0001;
    pub const START_A: u16 = 0x0305;
    pub const START_B: u16 = 0x0307;
    pub const INVALIDATE_REF_FRAMES: u16 = 0x0301;
    pub const IDR_REQUEST: u16 = 0x0302;
    pub const LOSS_STATS: u16 = 0x0201;
    pub const FRAME_STATS: u16 = 0x0204;
    pub const INPUT: u16 = 0x0206;
    pub const PERIODIC_PING: u16 = 0x0200;
    pub const RUMBLE: u16 = 0x010b;
    pub const TERMINATION: u16 = 0x0109;
    pub const HDR_MODE: u16 = 0x010e;
    pub const RUMBLE_TRIGGERS: u16 = 0x5500;
}

/// ENet channels (Limelight-internal.h `CTRL_CHANNEL_*`).
pub mod channel {
    pub const GENERIC: u8 = 0x00;
    pub const URGENT: u8 = 0x01;
    pub const KEYBOARD: u8 = 0x02;
    pub const MOUSE: u8 = 0x03;
    pub const GAMEPAD_BASE: u8 = 0x10;
}

/// moonlight's `tempBuffer[256]` bound on header + payload.
pub const MAX_PLAINTEXT: usize = 255;
/// Sunshine's graceful-shutdown HRESULT (`NVST_DISCONN_SERVER_TERMINATED_CLOSED`).
pub const TERMINATION_GRACEFUL: u32 = 0x8003_0023;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlMsg {
    /// Gen7Enc has no Start A on the wire from moonlight: its index-0 slot is `IdrRequest`.
    StartA,
    StartB,
    /// moonlight's 100 ms keepalive (`usePeriodicPing`).
    Ping,
    IdrRequest,
    InvalidateRefFrames {
        first: u32,
        last: u32,
    },
    LossStats(Vec<u8>),
    FrameStats(Vec<u8>),
    /// One plaintext packet from [`encode_input`] plus the ENet channel moonlight uses for it.
    Input {
        channel: u8,
        packet: Vec<u8>,
    },
    Rumble {
        controller: u16,
        low: u16,
        high: u16,
    },
    RumbleTriggers {
        controller: u16,
        left: u16,
        right: u16,
    },
    HdrMode {
        enabled: bool,
    },
    /// Extended form: HRESULT, big-endian. The 2-byte form (u16 LE) is widened.
    Termination(u32),
    Unknown(u16, Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlError {
    TooLarge,
    Truncated,
    /// Not an `ENCRYPTED` (0x0001) envelope; Sunshine and moonlight drop these.
    Unencrypted,
    BadTag,
    /// Host sequence number not above the last accepted one.
    Replay,
    SeqExhausted,
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "control: {self:?}")
    }
}
impl std::error::Error for ControlError {}

impl ControlMsg {
    /// Packets for one input event, each on moonlight's channel (InputStream.c): keyboard 0x02,
    /// mouse move/button/scroll 0x03, gamepad 0x10 + index. All are sent reliable.
    pub fn input(ev: &InputEvent) -> Result<Vec<ControlMsg>, InputError> {
        let channel = match ev {
            InputEvent::Key { .. } => channel::KEYBOARD,
            InputEvent::Gamepad { index, .. } => channel::GAMEPAD_BASE + index,
            _ => channel::MOUSE,
        };
        Ok(encode_input(ev)?
            .into_iter()
            .map(|packet| ControlMsg::Input { channel, packet })
            .collect())
    }

    /// ENet (channel, reliable) moonlight uses for this message (ControlStream.c call sites).
    pub fn route(&self) -> (u8, bool) {
        match self {
            ControlMsg::IdrRequest | ControlMsg::InvalidateRefFrames { .. } => {
                (channel::URGENT, true)
            }
            ControlMsg::Input { channel, .. } => (*channel, true),
            ControlMsg::LossStats(_) | ControlMsg::FrameStats(_) => (channel::GENERIC, false),
            _ => (channel::GENERIC, true),
        }
    }

    /// Packet type on the wire (inside the encrypted envelope).
    pub fn kind(&self) -> u16 {
        self.plain().0
    }

    fn plain(&self) -> (u16, Vec<u8>) {
        use ptype::*;
        match self {
            ControlMsg::StartA => (START_A, vec![0, 0]),
            ControlMsg::StartB => (START_B, vec![0]),
            // BbPut16(4), BbPut32(0) into an 8-byte buffer; moonlight leaves the last 2 unset.
            ControlMsg::Ping => (PERIODIC_PING, vec![4, 0, 0, 0, 0, 0, 0, 0]),
            ControlMsg::IdrRequest => (IDR_REQUEST, vec![0, 0]),
            ControlMsg::InvalidateRefFrames { first, last } => {
                // SS_RFI_REQUEST: first, reserved, last, reserved[3] (u32 LE each).
                let mut p = vec![0u8; 24];
                p[..4].copy_from_slice(&first.to_le_bytes());
                p[8..12].copy_from_slice(&last.to_le_bytes());
                (INVALIDATE_REF_FRAMES, p)
            }
            ControlMsg::LossStats(p) => (LOSS_STATS, p.clone()),
            ControlMsg::FrameStats(p) => (FRAME_STATS, p.clone()),
            ControlMsg::Input { packet, .. } => (INPUT, packet.clone()),
            ControlMsg::Rumble {
                controller,
                low,
                high,
            } => {
                let mut p = 0xC0FFEEu32.to_le_bytes().to_vec();
                for v in [controller, low, high] {
                    p.extend_from_slice(&v.to_le_bytes());
                }
                (RUMBLE, p)
            }
            ControlMsg::RumbleTriggers {
                controller,
                left,
                right,
            } => {
                let mut p = Vec::new();
                for v in [controller, left, right] {
                    p.extend_from_slice(&v.to_le_bytes());
                }
                (RUMBLE_TRIGGERS, p)
            }
            ControlMsg::HdrMode { enabled } => (HDR_MODE, vec![*enabled as u8]),
            ControlMsg::Termination(ec) => (TERMINATION, ec.to_be_bytes().to_vec()),
            ControlMsg::Unknown(t, p) => (*t, p.clone()),
        }
    }

    fn parse(t: u16, p: &[u8]) -> ControlMsg {
        let le = |i: usize| u16::from_le_bytes([p[i], p[i + 1]]);
        use ptype::*;
        match (t, p.len()) {
            (START_A, _) => ControlMsg::StartA,
            (START_B, _) => ControlMsg::StartB,
            (PERIODIC_PING, _) => ControlMsg::Ping,
            (IDR_REQUEST, _) => ControlMsg::IdrRequest,
            (INVALIDATE_REF_FRAMES, n) if n >= 12 => ControlMsg::InvalidateRefFrames {
                first: u32::from_le_bytes(p[..4].try_into().unwrap()),
                last: u32::from_le_bytes(p[8..12].try_into().unwrap()),
            },
            (LOSS_STATS, _) => ControlMsg::LossStats(p.to_vec()),
            (FRAME_STATS, _) => ControlMsg::FrameStats(p.to_vec()),
            (INPUT, _) => ControlMsg::Input {
                channel: channel::GENERIC,
                packet: p.to_vec(),
            },
            // moonlight skips 4 bytes (Sunshine's 0xC0FFEE), then three u16 LE.
            (RUMBLE, n) if n >= 10 => ControlMsg::Rumble {
                controller: le(4),
                low: le(6),
                high: le(8),
            },
            (RUMBLE_TRIGGERS, n) if n >= 6 => ControlMsg::RumbleTriggers {
                controller: le(0),
                left: le(2),
                right: le(4),
            },
            (HDR_MODE, n) if n >= 1 => ControlMsg::HdrMode { enabled: p[0] != 0 },
            (TERMINATION, n) if n >= 4 => {
                ControlMsg::Termination(u32::from_be_bytes(p[..4].try_into().unwrap()))
            }
            (TERMINATION, n) if n >= 2 => ControlMsg::Termination(le(0) as u32),
            _ => ControlMsg::Unknown(t, p.to_vec()),
        }
    }
}

/// Per-session AES-GCM sealer/opener with one sequence counter per direction.
pub struct Control {
    cipher: Aes128Gcm,
    tx: u64,
    rx: Option<u32>,
    tx_dir: u8,
    rx_dir: u8,
}

fn iv(seq: u32, dir: u8) -> [u8; 12] {
    let mut iv = [0u8; 12];
    iv[..4].copy_from_slice(&seq.to_le_bytes());
    iv[10] = dir;
    iv[11] = b'C';
    iv
}

impl Control {
    /// Client side. (Gen7Enc V2 IVs do not use `rikeyid`; only the legacy 16-byte IV did.)
    pub fn new(rikey: [u8; 16]) -> Self {
        Self::with_dirs(rikey, b'C', b'H')
    }
    /// Host side, for test fakes.
    pub fn host(rikey: [u8; 16]) -> Self {
        Self::with_dirs(rikey, b'H', b'C')
    }
    fn with_dirs(rikey: [u8; 16], tx_dir: u8, rx_dir: u8) -> Self {
        Control {
            cipher: Aes128Gcm::new(&rikey.into()),
            tx: 0,
            rx: None,
            tx_dir,
            rx_dir,
        }
    }

    pub fn encode(&mut self, msg: &ControlMsg) -> Result<Vec<u8>, ControlError> {
        let (t, p) = msg.plain();
        if 4 + p.len() > MAX_PLAINTEXT {
            return Err(ControlError::TooLarge);
        }
        let seq = u32::try_from(self.tx).map_err(|_| ControlError::SeqExhausted)?;
        let mut pt = t.to_le_bytes().to_vec();
        pt.extend_from_slice(&(p.len() as u16).to_le_bytes());
        pt.extend_from_slice(&p);
        let tag = self
            .cipher
            .encrypt_in_place_detached(Nonce::from_slice(&iv(seq, self.tx_dir)), b"", &mut pt)
            .map_err(|_| ControlError::TooLarge)?;
        self.tx += 1;
        let mut out = ptype::ENCRYPTED.to_le_bytes().to_vec();
        out.extend_from_slice(&((4 + 16 + pt.len()) as u16).to_le_bytes());
        out.extend_from_slice(&seq.to_le_bytes());
        out.extend_from_slice(&tag);
        out.extend_from_slice(&pt);
        Ok(out)
    }

    /// Checks follow moonlight `decryptControlMessageToV1`: envelope type, `length` within the
    /// packet, runt (< seq + tag + V2 header). Bytes beyond `length` are ignored, as there.
    pub fn decode(&mut self, d: &[u8]) -> Result<ControlMsg, ControlError> {
        if d.len() > 4 + u16::MAX as usize {
            return Err(ControlError::TooLarge);
        }
        if d.len() < 2 {
            return Err(ControlError::Truncated);
        }
        if u16::from_le_bytes([d[0], d[1]]) != ptype::ENCRYPTED {
            return Err(ControlError::Unencrypted);
        }
        if d.len() < 8 {
            return Err(ControlError::Truncated);
        }
        let len = u16::from_le_bytes([d[2], d[3]]) as usize;
        if len < 4 + 16 + 4 || d.len() < 4 + len {
            return Err(ControlError::Truncated);
        }
        let seq = u32::from_le_bytes(d[4..8].try_into().unwrap());
        if self.rx.is_some_and(|r| seq <= r) {
            return Err(ControlError::Replay);
        }
        let tag = Tag::from_slice(&d[8..24]);
        let mut pt = d[24..4 + len].to_vec();
        self.cipher
            .decrypt_in_place_detached(Nonce::from_slice(&iv(seq, self.rx_dir)), b"", &mut pt, tag)
            .map_err(|_| ControlError::BadTag)?;
        self.rx = Some(seq);
        let t = u16::from_le_bytes([pt[0], pt[1]]);
        Ok(ControlMsg::parse(t, &pt[4..]))
    }
}
