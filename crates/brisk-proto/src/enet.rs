//! Minimal sans-io ENet 1.3 client peer: only what Sunshine's control stream uses.
//!
//! Wire format and behaviour follow the ENet fork in moonlight-common-c (`enet/protocol.c`,
//! `peer.c`, `host.c`, `include/enet/protocol.h`, ENet 1.3.17), which Sunshine also builds
//! (`cmake/dependencies/common.cmake`). That fork has no checksum and no compressor configured, so
//! neither is implemented. All multi-byte fields are big-endian except `connectID`, which ENet
//! copies opaquely. Constants mirror a moonlight client host: one peer, MTU 900
//! (`ENET_HOST_DEFAULT_MTU`), 0x30 channels (`CTRL_CHANNEL_COUNT`), unlimited bandwidth
//! (window 65536), default throttle (5000/2/2), ping interval 500 ms, peer timeout 10 s
//! (`enet_peer_timeout(peer, 2, 10000, 10000)` in ControlStream.c).
//!
//! Not implemented: fragments (every Sunshine control message fits one datagram), compression,
//! checksums, bandwidth/throttle adaptation. Received fragments are dropped without ACK.
//!
//! Datagram: `peerID u16` (low 12 bits id, bits 12-13 session, bit 15 sent-time present),
//! optional `sentTime u16`, then commands. Command header: `command u8` (low nibble number,
//! 0x80 acknowledge, 0x40 unsequenced), `channelID u8`, `reliableSequenceNumber u16`.

use std::collections::HashMap;

pub const MTU: u32 = 900;
pub const CHANNEL_COUNT: u8 = 0x30;
pub const MAX_DATAGRAM: usize = 4096; // ENET_PROTOCOL_MAXIMUM_MTU: ENet never sends more
const PEER_ID_NONE: u16 = 0xFFF;
const SYSTEM: u8 = 0xFF; // channel of CONNECT / VERIFY_CONNECT / PING / DISCONNECT
const WINDOW: u32 = 65536;
const THROTTLE: (u32, u32, u32) = (5000, 2, 2);
const PING_INTERVAL: u32 = 500;
const TIMEOUT: u32 = 10_000;
const TIMEOUT_LIMIT: u32 = 2;
/// Unacked reliable commands we keep in flight before `send` refuses.
pub const MAX_IN_FLIGHT: usize = 256;
/// Out-of-order reliable commands buffered per channel; further ones are dropped un-ACKed.
const REORDER: u16 = 64;

const ACK: u8 = 1;
const CONNECT: u8 = 2;
const VERIFY_CONNECT: u8 = 3;
const DISCONNECT: u8 = 4;
const PING: u8 = 5;
const SEND_RELIABLE: u8 = 6;
const SEND_UNRELIABLE: u8 = 7;
const SEND_UNSEQUENCED: u8 = 9;
const F_ACK: u8 = 0x80;
const F_UNSEQ: u8 = 0x40;
const H_SENT_TIME: u16 = 0x8000;
const H_COMPRESSED: u16 = 0x4000;
/// `commandSizes` in protocol.c, indexed by command number.
const SIZES: [usize; 13] = [0, 8, 48, 44, 8, 4, 6, 8, 24, 8, 12, 16, 24];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnetEvent {
    Connected,
    Receive {
        channel: u8,
        data: Vec<u8>,
    },
    /// A reliable command we sent was acknowledged.
    Acked {
        channel: u8,
        seq: u16,
    },
    Disconnected {
        data: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnetError {
    Malformed,
    TooLarge,
    NotConnected,
    QueueFull,
    BadChannel,
    /// VERIFY_CONNECT did not match our CONNECT (connectID, throttle or channel count).
    Refused,
    /// A reliable command went unacknowledged for 10 s.
    Timeout,
}

impl std::fmt::Display for EnetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "enet: {self:?}")
    }
}
impl std::error::Error for EnetError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    Connecting,
    Connected,
    /// `enet_peer_disconnect_later`: DISCONNECT goes out once everything in flight is acked.
    DisconnectLater,
    Disconnecting,
    Disconnected,
}

struct Sent {
    channel: u8,
    seq: u16,
    number: u8,
    cmd: Vec<u8>,
    sent_at: u32,
    first_sent: u32,
    rto: u32,
    attempts: u32,
}

pub struct EnetPeer {
    state: PeerState,
    connect_id: u32,
    outgoing_peer_id: u16,
    incoming_session: u8,
    outgoing_session: u8,
    mtu: u32,
    channels: u8,
    system_seq: u16,
    out_rel: [u16; CHANNEL_COUNT as usize],
    out_unrel: [u16; CHANNEL_COUNT as usize],
    in_rel: [u16; CHANNEL_COUNT as usize],
    in_unrel: [u16; CHANNEL_COUNT as usize],
    reorder: HashMap<(u8, u16), Vec<u8>>,
    sent: Vec<Sent>,
    acks: Vec<[u8; 8]>,
    rtt: u32,
    rtt_var: u32,
    rtt_sampled: bool,
    last_receive: u32,
}

fn be16(d: &[u8], i: usize) -> u16 {
    u16::from_be_bytes([d[i], d[i + 1]])
}
fn be32(d: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]])
}
/// `ENET_TIME_LESS`: modular u32 milliseconds.
fn time_less(a: u32, b: u32) -> bool {
    a.wrapping_sub(b) >= 86_400_000
}

impl EnetPeer {
    /// `enet_host_connect(client, addr, CTRL_CHANNEL_COUNT, connect_data)`. `connect_id` must be
    /// random (ENet uses `enet_host_random`); `connect_data` is RTSP `X-SS-Connect-Data`.
    pub fn connect(connect_data: u32, connect_id: u32, now: u32) -> (Self, Vec<u8>) {
        let mut p = EnetPeer {
            state: PeerState::Connecting,
            connect_id,
            outgoing_peer_id: PEER_ID_NONE,
            incoming_session: 0xFF,
            outgoing_session: 0xFF,
            mtu: MTU,
            channels: CHANNEL_COUNT,
            system_seq: 0,
            out_rel: [0; CHANNEL_COUNT as usize],
            out_unrel: [0; CHANNEL_COUNT as usize],
            in_rel: [0; CHANNEL_COUNT as usize],
            in_unrel: [0; CHANNEL_COUNT as usize],
            reorder: HashMap::new(),
            sent: Vec::new(),
            acks: Vec::new(),
            rtt: 500, // ENET_PEER_DEFAULT_ROUND_TRIP_TIME
            rtt_var: 0,
            rtt_sampled: false,
            last_receive: now,
        };
        let mut c = p.system_header(CONNECT | F_ACK);
        c.extend_from_slice(&0u16.to_be_bytes()); // outgoingPeerID = our incoming id (peer 0)
        c.extend_from_slice(&[0xFF, 0xFF]); // incoming/outgoing session id
        for v in [
            MTU,
            WINDOW,
            CHANNEL_COUNT as u32,
            0,
            0,
            THROTTLE.0,
            THROTTLE.1,
            THROTTLE.2,
        ] {
            c.extend_from_slice(&v.to_be_bytes());
        }
        c.extend_from_slice(&connect_id.to_ne_bytes());
        c.extend_from_slice(&connect_data.to_be_bytes());
        let d = p.queue_reliable(SYSTEM, c, now);
        (p, d)
    }

    pub fn state(&self) -> PeerState {
        self.state
    }
    pub fn rtt(&self) -> u32 {
        self.rtt
    }
    /// Reliable commands sent and not yet acknowledged.
    pub fn in_flight(&self) -> usize {
        self.sent.len()
    }

    fn system_header(&mut self, command: u8) -> Vec<u8> {
        self.system_seq = self.system_seq.wrapping_add(1);
        let mut c = vec![command, SYSTEM];
        c.extend_from_slice(&self.system_seq.to_be_bytes());
        c
    }

    fn rto_base(&self) -> u32 {
        (self.rtt + self.rtt.min(4 * self.rtt_var.max(1))).min(TIMEOUT / 5)
    }

    fn queue_reliable(&mut self, channel: u8, cmd: Vec<u8>, now: u32) -> Vec<u8> {
        let d = self.datagram(now, &[&cmd], true);
        self.sent.push(Sent {
            channel,
            seq: be16(&cmd, 2),
            number: cmd[0] & 0x0F,
            cmd,
            sent_at: now,
            first_sent: now,
            rto: self.rto_base(),
            attempts: 1,
        });
        d
    }

    /// Header plus as many pending ACKs as fit, then `cmds`.
    fn datagram(&mut self, now: u32, cmds: &[&[u8]], sent_time: bool) -> Vec<u8> {
        let mut peer_id = self.outgoing_peer_id;
        if self.outgoing_peer_id < PEER_ID_NONE {
            peer_id |= (self.outgoing_session as u16 & 3) << 12;
        }
        if sent_time {
            peer_id |= H_SENT_TIME;
        }
        let mut d = peer_id.to_be_bytes().to_vec();
        if sent_time {
            d.extend_from_slice(&(now as u16).to_be_bytes());
        }
        let body: usize = cmds.iter().map(|c| c.len()).sum();
        let room = (self.mtu as usize).saturating_sub(d.len() + body) / 8;
        for a in self.acks.drain(..room.min(self.acks.len())) {
            d.extend_from_slice(&a);
        }
        for c in cmds {
            d.extend_from_slice(c);
        }
        d
    }

    /// One datagram carrying `payload` on `channel`: SEND_RELIABLE, or SEND_UNRELIABLE (sequenced,
    /// ENet's default for packets without flags).
    pub fn send(
        &mut self,
        channel: u8,
        reliable: bool,
        payload: &[u8],
        now: u32,
    ) -> Result<Vec<u8>, EnetError> {
        if self.state != PeerState::Connected {
            return Err(EnetError::NotConnected);
        }
        if channel >= self.channels {
            return Err(EnetError::BadChannel);
        }
        let hdr = if reliable { 6 } else { 8 };
        if 4 + hdr + payload.len() > self.mtu as usize {
            return Err(EnetError::TooLarge);
        }
        let ch = channel as usize;
        let mut c = Vec::with_capacity(hdr + payload.len());
        if reliable {
            if self.sent.len() >= MAX_IN_FLIGHT {
                return Err(EnetError::QueueFull);
            }
            self.out_rel[ch] = self.out_rel[ch].wrapping_add(1);
            self.out_unrel[ch] = 0;
            c.extend_from_slice(&[SEND_RELIABLE | F_ACK, channel]);
            c.extend_from_slice(&self.out_rel[ch].to_be_bytes());
        } else {
            self.out_unrel[ch] = self.out_unrel[ch].wrapping_add(1);
            c.extend_from_slice(&[SEND_UNRELIABLE, channel]);
            c.extend_from_slice(&self.out_rel[ch].to_be_bytes());
            c.extend_from_slice(&self.out_unrel[ch].to_be_bytes());
        }
        c.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        c.extend_from_slice(payload);
        Ok(if reliable {
            self.queue_reliable(channel, c, now)
        } else {
            self.datagram(now, &[&c], false)
        })
    }

    /// `enet_peer_disconnect_later`: DISCONNECT once in-flight reliables are acked; returns it
    /// now if nothing is in flight. Otherwise behaves as [`Self::disconnect_now`].
    pub fn disconnect(&mut self, now: u32) -> Vec<Vec<u8>> {
        match self.state {
            PeerState::Connected | PeerState::DisconnectLater => {
                self.state = PeerState::DisconnectLater;
                self.poll_timeout(now).map(|(d, _)| d).unwrap_or_default()
            }
            _ => self.disconnect_now(now).into_iter().collect(),
        }
    }

    /// `enet_peer_disconnect_now`: unsequenced DISCONNECT, no ACK awaited, peer reset. What
    /// moonlight does after a termination message (the host no longer ACKs).
    pub fn disconnect_now(&mut self, now: u32) -> Option<Vec<u8>> {
        if self.state == PeerState::Disconnected {
            return None;
        }
        let mut c = self.system_header(DISCONNECT | F_UNSEQ);
        c.extend_from_slice(&0u32.to_be_bytes());
        self.state = PeerState::Disconnected;
        self.sent.clear();
        Some(self.datagram(now, &[&c], false))
    }

    /// Sequence number of the last reliable command sent on `channel` (0 before any).
    pub fn outgoing_seq(&self, channel: u8) -> u16 {
        self.out_rel.get(channel as usize).copied().unwrap_or(0)
    }

    /// Retransmissions, pings, pending ACKs and a deferred DISCONNECT due at `now`, plus the
    /// delay in ms until the next call is needed. `Err(Timeout)` when the peer is gone.
    pub fn poll_timeout(&mut self, now: u32) -> Result<(Vec<Vec<u8>>, Option<u32>), EnetError> {
        let mut out = Vec::new();
        if self.state == PeerState::Disconnected {
            if !self.acks.is_empty() {
                out.push(self.datagram(now, &[], false));
            }
            return Ok((out, None));
        }
        for i in 0..self.sent.len() {
            let s = &self.sent[i];
            if now.wrapping_sub(s.sent_at) < s.rto {
                continue;
            }
            if now.wrapping_sub(s.first_sent) >= TIMEOUT {
                self.state = PeerState::Disconnected;
                self.sent.clear();
                return Err(EnetError::Timeout);
            }
            // enet_protocol_check_timeouts: back off by attempts, capped at the timeout limit.
            let rto = self.rto_base() * self.sent[i].attempts.clamp(1, TIMEOUT_LIMIT);
            let cmd = self.sent[i].cmd.clone();
            out.push(self.datagram(now, &[&cmd], true));
            let s = &mut self.sent[i];
            s.rto = rto;
            s.attempts += 1;
            s.sent_at = now;
        }
        if self.state == PeerState::DisconnectLater && self.sent.is_empty() {
            let mut c = self.system_header(DISCONNECT | F_ACK);
            c.extend_from_slice(&0u32.to_be_bytes());
            self.state = PeerState::Disconnecting;
            out.push(self.queue_reliable(SYSTEM, c, now));
        }
        if self.state == PeerState::Connected
            && self.sent.is_empty()
            && now.wrapping_sub(self.last_receive) >= PING_INTERVAL
        {
            let c = self.system_header(PING | F_ACK);
            out.push(self.queue_reliable(SYSTEM, c, now));
        }
        if !self.acks.is_empty() {
            out.push(self.datagram(now, &[], false));
        }
        let mut next = self
            .sent
            .iter()
            .map(|s| s.rto.saturating_sub(now.wrapping_sub(s.sent_at)))
            .min();
        if self.state == PeerState::Connected && self.sent.is_empty() {
            next = Some(PING_INTERVAL.saturating_sub(now.wrapping_sub(self.last_receive)));
        }
        Ok((out, next))
    }

    /// Parse one datagram from the host. Commands after the first malformed one are ignored
    /// (as `enet_protocol_handle_incoming_commands` does); ACKs for accepted reliable commands
    /// are queued and go out with the next `send`/`poll_timeout`.
    pub fn handle_datagram(&mut self, d: &[u8], now: u32) -> Result<Vec<EnetEvent>, EnetError> {
        if d.len() > MAX_DATAGRAM {
            return Err(EnetError::TooLarge);
        }
        if d.len() < 2 {
            return Err(EnetError::Malformed);
        }
        let raw = be16(d, 0);
        let session = ((raw >> 12) & 3) as u8;
        let peer_id = raw & 0x0FFF;
        if raw & H_COMPRESSED != 0 || peer_id != 0 {
            return Err(EnetError::Malformed);
        }
        if self.outgoing_peer_id < PEER_ID_NONE && session != self.incoming_session {
            return Err(EnetError::Malformed);
        }
        let sent_time = (raw & H_SENT_TIME != 0).then(|| d.get(2..4).map(|b| be16(b, 0)));
        let sent_time = match sent_time {
            Some(None) => return Err(EnetError::Malformed),
            Some(Some(t)) => Some(t),
            None => None,
        };
        let mut i = if sent_time.is_some() { 4 } else { 2 };
        let mut ev = Vec::new();
        while i + 4 <= d.len() {
            let number = d[i] & 0x0F;
            let size = *SIZES.get(number as usize).unwrap_or(&0);
            if size == 0 || i + size > d.len() {
                break;
            }
            let c = &d[i..i + size];
            i += size;
            let data_len = match number {
                SEND_RELIABLE => be16(c, 4) as usize,
                SEND_UNRELIABLE | SEND_UNSEQUENCED | 8 | 12 => be16(c, 6) as usize,
                _ => 0,
            };
            if i + data_len > d.len() {
                break;
            }
            let data = &d[i..i + data_len];
            i += data_len;
            match self.command(number, c, data, now, &mut ev) {
                Ok(true) => {}
                Ok(false) => continue, // accepted but must not be ACKed
                Err(_) => break,
            }
            if c[0] & F_ACK != 0 {
                // No sent-time header means ENet stops here without ACKing (protocol.c).
                let Some(t) = sent_time else { break };
                let mut a = [ACK, c[1], c[2], c[3], c[2], c[3], 0, 0];
                a[6..].copy_from_slice(&t.to_be_bytes());
                if self.acks.len() < MAX_IN_FLIGHT {
                    self.acks.push(a);
                }
            }
        }
        Ok(ev)
    }

    /// Ok(true): ACK it if flagged. Ok(false): drop silently. Err: stop parsing.
    fn command(
        &mut self,
        number: u8,
        c: &[u8],
        data: &[u8],
        now: u32,
        ev: &mut Vec<EnetEvent>,
    ) -> Result<bool, EnetError> {
        let channel = c[1];
        let seq = be16(c, 2);
        let live = matches!(
            self.state,
            PeerState::Connected | PeerState::DisconnectLater | PeerState::Disconnecting
        );
        match number {
            ACK => {
                if self.state == PeerState::Disconnected {
                    return Ok(true);
                }
                self.on_ack(be16(c, 6), now);
                let acked = be16(c, 4);
                if let Some(k) = self
                    .sent
                    .iter()
                    .position(|s| s.channel == channel && s.seq == acked)
                {
                    let s = self.sent.remove(k);
                    ev.push(EnetEvent::Acked {
                        channel,
                        seq: acked,
                    });
                    if s.number == DISCONNECT && self.state == PeerState::Disconnecting {
                        self.state = PeerState::Disconnected;
                        self.sent.clear();
                        ev.push(EnetEvent::Disconnected { data: 0 });
                    }
                }
                Ok(true)
            }
            VERIFY_CONNECT => {
                if self.state != PeerState::Connecting {
                    return Ok(true);
                }
                let channels = be32(c, 16);
                if !(1..=255).contains(&channels)
                    || (be32(c, 28), be32(c, 32), be32(c, 36)) != THROTTLE
                    || c[40..44] != self.connect_id.to_ne_bytes()
                {
                    self.state = PeerState::Disconnected;
                    self.sent.clear();
                    return Err(EnetError::Refused);
                }
                self.sent.retain(|s| !(s.channel == SYSTEM && s.seq == 1));
                self.channels = self.channels.min(channels as u8);
                self.outgoing_peer_id = be16(c, 4) & 0x0FFF;
                self.incoming_session = c[6];
                self.outgoing_session = c[7];
                self.mtu = self.mtu.min(be32(c, 8).clamp(576, 4096));
                self.state = PeerState::Connected;
                self.last_receive = now;
                ev.push(EnetEvent::Connected);
                Ok(true)
            }
            DISCONNECT => {
                if self.state == PeerState::Disconnected {
                    return Ok(true);
                }
                self.state = PeerState::Disconnected;
                self.sent.clear();
                ev.push(EnetEvent::Disconnected { data: be32(c, 4) });
                Ok(true)
            }
            PING => Ok(live),
            SEND_RELIABLE if live && channel < self.channels => {
                Ok(self.reliable(channel, seq, data, ev))
            }
            SEND_UNRELIABLE if live && channel < self.channels => {
                let ch = channel as usize;
                let useq = be16(c, 4);
                let d = useq.wrapping_sub(self.in_unrel[ch]);
                // shortcut: a sequenced-unreliable command whose reliable seq is ahead of ours is
                // dropped, not queued as ENet does; Sunshine never sends unreliable. Upgrade if a
                // host starts using unreliable sends.
                if seq == self.in_rel[ch] && (1..0x8000).contains(&d) {
                    self.in_unrel[ch] = useq;
                    ev.push(EnetEvent::Receive {
                        channel,
                        data: data.to_vec(),
                    });
                }
                Ok(true)
            }
            SEND_UNSEQUENCED if live && channel < self.channels => {
                ev.push(EnetEvent::Receive {
                    channel,
                    data: data.to_vec(),
                });
                Ok(true)
            }
            10 | 11 => Ok(live), // bandwidth limit / throttle configure: ACK, otherwise ignore
            8 | 12 => Ok(false), // shortcut: fragments unsupported; Sunshine's messages fit one datagram
            _ => Err(EnetError::Malformed),
        }
    }

    /// In-order delivery per channel. Returns whether to ACK (not for commands we drop).
    fn reliable(&mut self, channel: u8, seq: u16, data: &[u8], ev: &mut Vec<EnetEvent>) -> bool {
        let ch = channel as usize;
        let d = seq.wrapping_sub(self.in_rel[ch]);
        if d == 0 || d >= 0x8000 {
            return true; // duplicate of something delivered: re-ACK
        }
        if d > 1 {
            if d > REORDER || self.reorder.len() >= REORDER as usize * 4 {
                return false;
            }
            self.reorder.insert((channel, seq), data.to_vec());
            return true;
        }
        let mut next = Some(data.to_vec());
        while let Some(data) = next {
            self.in_rel[ch] = self.in_rel[ch].wrapping_add(1);
            self.in_unrel[ch] = 0;
            ev.push(EnetEvent::Receive { channel, data });
            next = self
                .reorder
                .remove(&(channel, self.in_rel[ch].wrapping_add(1)));
        }
        true
    }

    /// `enet_protocol_handle_acknowledge` RTT update from the echoed 16-bit sent time.
    fn on_ack(&mut self, echoed: u16, now: u32) {
        let mut t = echoed as u32 | (now & 0xFFFF_0000);
        if (t & 0x8000) > (now & 0x8000) {
            t = t.wrapping_sub(0x10000);
        }
        if time_less(now, t) {
            return;
        }
        let rtt = now.wrapping_sub(t).max(1);
        self.last_receive = now.max(1);
        if !std::mem::replace(&mut self.rtt_sampled, true) {
            self.rtt = rtt;
            self.rtt_var = rtt.div_ceil(2);
            return;
        }
        self.rtt_var -= self.rtt_var.div_ceil(4);
        if rtt >= self.rtt {
            let diff = rtt - self.rtt;
            self.rtt_var += diff.div_ceil(4);
            self.rtt += diff.div_ceil(8);
        } else {
            let diff = self.rtt - rtt;
            self.rtt_var += diff.div_ceil(4);
            self.rtt -= diff.div_ceil(8);
        }
    }
}
