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

use crate::input::{InputError, InputEvent, encode_input};
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce, Tag};
use std::borrow::Cow;

/// Packet types: Sunshine stream.cpp `packetTypes`, moonlight `packetTypesGen7Enc`.
pub mod ptype {
    /// Envelope of every sealed message.
    pub const ENCRYPTED: u16 = 0x0001;
    /// Start A.
    pub const START_A: u16 = 0x0305;
    /// Start B.
    pub const START_B: u16 = 0x0307;
    /// Reference-frame invalidation request.
    pub const INVALIDATE_REF_FRAMES: u16 = 0x0301;
    /// IDR frame request.
    pub const IDR_REQUEST: u16 = 0x0302;
    /// Client loss statistics.
    pub const LOSS_STATS: u16 = 0x0201;
    /// Client frame statistics.
    pub const FRAME_STATS: u16 = 0x0204;
    /// Input packet.
    pub const INPUT: u16 = 0x0206;
    /// Keepalive ping.
    pub const PERIODIC_PING: u16 = 0x0200;
    /// Gamepad rumble.
    pub const RUMBLE: u16 = 0x010b;
    /// Host ends the session.
    pub const TERMINATION: u16 = 0x0109;
    /// Host HDR mode change.
    pub const HDR_MODE: u16 = 0x010e;
    /// Trigger rumble.
    pub const RUMBLE_TRIGGERS: u16 = 0x5500;
}

/// ENet channels (Limelight-internal.h `CTRL_CHANNEL_*`).
pub mod channel {
    /// Generic channel.
    pub const GENERIC: u8 = 0x00;
    /// Urgent channel.
    pub const URGENT: u8 = 0x01;
    /// Keyboard input.
    pub const KEYBOARD: u8 = 0x02;
    /// Mouse input.
    pub const MOUSE: u8 = 0x03;
    /// First gamepad channel; pad `n` uses `GAMEPAD_BASE + n`.
    pub const GAMEPAD_BASE: u8 = 0x10;
}

/// moonlight's `tempBuffer[256]` bound on header + payload.
pub const MAX_PLAINTEXT: usize = 255;
/// Sunshine's graceful-shutdown HRESULT (`NVST_DISCONN_SERVER_TERMINATED_CLOSED`).
pub const TERMINATION_GRACEFUL: u32 = 0x8003_0023;

#[derive(Debug, Clone, PartialEq, Eq)]
/// One control-stream message, either direction.
pub enum ControlMsg {
    /// Gen7Enc has no Start A on the wire from moonlight: its index-0 slot is `IdrRequest`.
    StartA,
    /// Start B: sent after Start A to begin streaming.
    StartB,
    /// moonlight's 100 ms keepalive (`usePeriodicPing`).
    Ping,
    /// Ask the host for an IDR frame.
    IdrRequest,
    /// Ask the host to stop referencing frames `first..=last`.
    InvalidateRefFrames {
        /// First frame index to invalidate.
        first: u32,
        /// Last frame index to invalidate.
        last: u32,
    },
    /// Loss statistics payload, sent as is.
    LossStats(Vec<u8>),
    /// Frame statistics payload, sent as is.
    FrameStats(Vec<u8>),
    /// One plaintext packet from [`encode_input`] plus the ENet channel moonlight uses for it.
    Input {
        /// ENet channel for the packet.
        channel: u8,
        /// The plaintext input packet.
        packet: Vec<u8>,
    },
    /// Gamepad rumble from the host.
    Rumble {
        /// Controller index.
        controller: u16,
        /// Low-frequency motor strength.
        low: u16,
        /// High-frequency motor strength.
        high: u16,
    },
    /// Trigger rumble from the host.
    RumbleTriggers {
        /// Controller index.
        controller: u16,
        /// Left trigger motor strength.
        left: u16,
        /// Right trigger motor strength.
        right: u16,
    },
    /// Host switched HDR on or off.
    HdrMode {
        /// HDR is on.
        enabled: bool,
    },
    /// Extended form: HRESULT, big-endian. The 2-byte form (u16 LE) is widened.
    Termination(u32),
    /// Any other type, with its raw payload.
    Unknown(u16, Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Why a control message could not be sealed or opened.
pub enum ControlError {
    /// Plaintext or envelope over [`MAX_PLAINTEXT`].
    TooLarge,
    /// Shorter than its headers or its length field.
    Truncated,
    /// Not an `ENCRYPTED` (0x0001) envelope; Sunshine and moonlight drop these.
    Unencrypted,
    /// AES-GCM tag did not verify: tampered or wrong key.
    BadTag,
    /// Host sequence number not above the last accepted one.
    Replay,
    /// 2^32 messages sent under one key; sending more would reuse an IV.
    SeqExhausted,
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ControlError::TooLarge => "control message too large",
            ControlError::Truncated => "truncated control message",
            ControlError::Unencrypted => "unencrypted control message",
            ControlError::BadTag => "control message failed authentication",
            ControlError::Replay => "replayed control sequence number",
            ControlError::SeqExhausted => "control sequence number exhausted",
        })
    }
}
impl std::error::Error for ControlError {}

impl ControlMsg {
    /// Packets for one input event, each on moonlight's channel (InputStream.c): keyboard 0x02,
    /// mouse move/button/scroll 0x03, gamepad 0x10 + index. All are sent reliable.
    pub fn input(ev: &InputEvent) -> Result<Vec<ControlMsg>, InputError> {
        // Encode first: it rejects a gamepad index >= 16, so the channel sum below cannot overflow.
        let packets = encode_input(ev)?;
        let channel = match ev {
            InputEvent::Key { .. } => channel::KEYBOARD,
            InputEvent::Gamepad { index, .. } => channel::GAMEPAD_BASE + index,
            _ => channel::MOUSE,
        };
        Ok(packets
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

    /// Type and payload; borrowed where the message already holds its payload bytes.
    fn plain(&self) -> (u16, Cow<'_, [u8]>) {
        use ptype::*;
        match self {
            ControlMsg::StartA => (START_A, Cow::Borrowed(&[0, 0])),
            ControlMsg::StartB => (START_B, Cow::Borrowed(&[0])),
            // BbPut16(4), BbPut32(0) into an 8-byte buffer; moonlight leaves the last 2 unset.
            ControlMsg::Ping => (PERIODIC_PING, Cow::Borrowed(&[4, 0, 0, 0, 0, 0, 0, 0])),
            ControlMsg::IdrRequest => (IDR_REQUEST, Cow::Borrowed(&[0, 0])),
            ControlMsg::InvalidateRefFrames { first, last } => {
                // SS_RFI_REQUEST: first, reserved, last, reserved[3] (u32 LE each).
                let mut p = vec![0u8; 24];
                p[..4].copy_from_slice(&first.to_le_bytes());
                p[8..12].copy_from_slice(&last.to_le_bytes());
                (INVALIDATE_REF_FRAMES, p.into())
            }
            ControlMsg::LossStats(p) => (LOSS_STATS, p.into()),
            ControlMsg::FrameStats(p) => (FRAME_STATS, p.into()),
            ControlMsg::Input { packet, .. } => (INPUT, packet.into()),
            ControlMsg::Rumble {
                controller,
                low,
                high,
            } => {
                let mut p = 0xC0FFEEu32.to_le_bytes().to_vec();
                for v in [controller, low, high] {
                    p.extend_from_slice(&v.to_le_bytes());
                }
                (RUMBLE, p.into())
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
                (RUMBLE_TRIGGERS, p.into())
            }
            ControlMsg::HdrMode { enabled } => (HDR_MODE, vec![*enabled as u8].into()),
            ControlMsg::Termination(ec) => (TERMINATION, ec.to_be_bytes().to_vec().into()),
            ControlMsg::Unknown(t, p) => (*t, p.into()),
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
                first: u32::from_le_bytes(p[..4].try_into().expect("length checked: 4 bytes")),
                last: u32::from_le_bytes(p[8..12].try_into().expect("length checked: 4 bytes")),
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
            (TERMINATION, n) if n >= 4 => ControlMsg::Termination(u32::from_be_bytes(
                p[..4].try_into().expect("length checked: 4 bytes"),
            )),
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
    #[doc(hidden)]
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

    /// Seals `msg` as the next message in our direction.
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
        let seq = u32::from_le_bytes(d[4..8].try_into().expect("length checked: 4 bytes"));
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
