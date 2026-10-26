//! Fake Sunshine control + media host on 127.0.0.1 ephemeral ports, shared by control.rs and
//! session.rs.
//!
//! ENet on the fake side is rusty_enet (a Rust port of ENet 1.3.18), not our own code, so the
//! handshake/ACK/retransmit layer is checked against an independent implementation. The
//! control layer above it is written here from Sunshine stream.cpp, separately from
//! brisk_proto::control: `get_session` (connect-data match, else `enet_peer_disconnect_now`),
//! `IDX_ENCRYPTED` handler (length >= 24, tag then ciphertext, IV = seq LE + 'C','C', inner
//! type u16 LE, payload after the 4-byte V2 header), `encode_control` (host seq from 0, IV
//! 'H','C') and `control_server_t::send` (channel 0, reliable). Places where this can only
//! mirror our reading of stream.cpp are listed in fixtures/control/README.md.
//!
//! With [`Media`], the fake streams like `videoBroadcastThread` / `audioBroadcastThread` to the
//! ping source addresses: 60 fps video (frame 1 and each frame after an IDR request are IDR),
//! Reed-Solomon FEC per frame as `fec::encode` (packets built the way brisk-proto/tests/rtp.rs
//! does), optional `SS_ENC_VIDEO` AES-GCM; audio every 5 ms in 4+2 FEC blocks, AES-128-CBC like
//! `encode_audio` (as brisk-proto/tests/audio.rs). Packet *contents* are synthetic, not H.264.
#![allow(dead_code)]

use aes::Aes128;
use aes::cipher::{BlockEncrypt, generic_array::GenericArray};
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce, Tag};
use brisk_client::control::AvPing;
use brisk_proto::audio::{PT_AUDIO, PT_FEC, encode_parity};
use brisk_proto::fec::rs_encode;
use rusty_enet as enet;
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const KEY: [u8; 16] = *b"control-test-key";
pub const CONNECT_DATA: u32 = 1396329308;
/// X-SS-Ping-Payload from fixtures/rtsp/010 and 012 (Sunshine sends one value for audio and video).
pub const PING_PAYLOAD: &str = "FE073FEB9401CE42";
/// Stands in for Sunshine's `ping_timeout` (10 s, config.cpp:815), shortened for tests.
pub const FAKE_PING_TIMEOUT: Duration = Duration::from_millis(1500);
/// Sunshine `packetsize` as brisk-probe announces it.
const PACKET_SIZE: usize = 1392;
const FEC_PCT: usize = 20;

#[derive(Default, Debug)]
pub struct Log {
    pub connects: Vec<u32>,
    pub rejected: u32,
    pub disconnects: u32,
    /// (ENet channel, inner type, payload after the V2 header)
    pub msgs: Vec<(u8, u16, Vec<u8>)>,
    pub unencrypted: u32,
    pub bad_tag: u32,
    /// Datagrams containing the ping payload, per socket [video, audio].
    pub av_pings: [u32; 2],
    /// Raw first ping seen per socket.
    pub first_ping: [Option<Vec<u8>>; 2],
    /// `recv_ping` timed out on a socket: session::stop, ENet peer disconnected.
    pub stopped: bool,
    /// Media mode: video frames / IDR frames / datagrams sent, datagrams dropped on purpose.
    pub frames: u32,
    pub idr_frames: u32,
    pub video_sent: u32,
    pub video_dropped: u32,
    pub audio_sent: u32,
}

pub enum Cmd {
    /// (type, payload) sealed and sent reliable on channel 0.
    Send(u16, Vec<u8>),
    /// Raw ENet packet, no encryption.
    SendRaw(Vec<u8>),
    /// Termination 0x80030023 then `free_host`'s disconnect_now.
    Terminate,
}

/// What the fake streams once the A/V pings arrive. Default: clean 60 fps, IDR on request.
#[derive(Clone, Copy)]
pub struct Media {
    /// Chance in percent to drop each video datagram, at most the parity count per block.
    pub loss_pct: u32,
    /// Send one malformed video datagram just before this frame.
    pub corrupt_frame: Option<u32>,
    pub answer_idr: bool,
    /// Frames for which parity + 1 data packets are dropped: more than FEC can recover.
    pub lose_frame: fn(u32) -> bool,
    /// `SS_ENC_VIDEO` with this key (the rikey).
    pub video_key: Option<[u8; 16]>,
    /// `SS_ENC_AUDIO` with (rikey, rikeyid); plain audio otherwise.
    pub audio_key: Option<([u8; 16], u32)>,
}

impl Default for Media {
    fn default() -> Self {
        Media {
            loss_pct: 0,
            corrupt_frame: None,
            answer_idr: true,
            lose_frame: |_| false,
            video_key: None,
            audio_key: None,
        }
    }
}

pub struct Cfg {
    pub key: [u8; 16],
    pub connect_data: u32,
    pub ping_payload: &'static str,
    /// UDP datagrams to drop on the control socket first (forces retransmission).
    pub drop: usize,
    pub media: Option<Media>,
    /// As the brisk-patched Sunshine with `a=x-brisk.tap:1`: on the first video ping connect a
    /// SEQPACKET socket here and send the frames on it (BTAP v1) instead of RTP; if the connect
    /// fails, RTP as usual.
    pub tap: Option<std::path::PathBuf>,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg {
            key: KEY,
            connect_data: CONNECT_DATA,
            ping_payload: PING_PAYLOAD,
            drop: 0,
            media: None,
            tap: None,
        }
    }
}

/// UDP socket that drops the first `drop` received datagrams (to force retransmission).
struct Lossy {
    sock: UdpSocket,
    drop: usize,
}
impl enet::Socket for Lossy {
    type Address = SocketAddr;
    type Error = std::io::Error;
    fn init(&mut self, o: enet::SocketOptions) -> Result<(), Self::Error> {
        enet::Socket::init(&mut self.sock, o)
    }
    fn send(&mut self, a: SocketAddr, b: &[u8]) -> Result<usize, Self::Error> {
        enet::Socket::send(&mut self.sock, a, b)
    }
    fn receive(
        &mut self,
        buf: &mut [u8; enet::MTU_MAX],
    ) -> Result<Option<(SocketAddr, enet::PacketReceived)>, Self::Error> {
        let r = enet::Socket::receive(&mut self.sock, buf)?;
        if r.is_some() && self.drop > 0 {
            self.drop -= 1;
            return Ok(None);
        }
        Ok(r)
    }
}

fn iv(seq: u32, dir: u8) -> [u8; 12] {
    let mut iv = [0u8; 12];
    iv[..4].copy_from_slice(&seq.to_le_bytes());
    iv[10] = dir;
    iv[11] = b'C';
    iv
}

pub struct Fake {
    pub addr: SocketAddr,
    pub video: SocketAddr,
    pub audio: SocketAddr,
    pub log: Arc<Mutex<Log>>,
    pub tx: mpsc::Sender<Cmd>,
}

pub fn fake(drop: usize) -> Fake {
    fake_with(Cfg {
        drop,
        ..Cfg::default()
    })
}

/// xorshift64: deterministic loss pattern.
fn rnd(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

/// Sunshine's video side: builds and sends frames.
struct VideoHost {
    seq: u16,
    gcm_counter: u64,
    rng: u64,
}

impl VideoHost {
    /// One single-block frame, as `videoBroadcastThread` + `fec::encode` (see tests/rtp.rs).
    /// Returns the packets in send order and the parity count.
    fn frame(
        &mut self,
        idx: u32,
        frame_type: u8,
        latency: u16,
        annexb: &[u8],
    ) -> (Vec<Vec<u8>>, usize) {
        let blocksize = PACKET_SIZE + 16;
        let per = blocksize - 32;
        let mut last_len = ((annexb.len() + 8) % (PACKET_SIZE - 16)) as u16;
        if last_len == 0 {
            last_len = (PACKET_SIZE - 16) as u16;
        }
        let mut body = vec![0x01];
        body.extend(latency.to_le_bytes());
        body.push(frame_type);
        body.extend(last_len.to_le_bytes());
        body.extend([0, 0]);
        body.extend(annexb);
        let chunks: Vec<&[u8]> = body.chunks(per).collect();
        let n = chunks.len();
        let mut shards: Vec<Vec<u8>> = chunks
            .iter()
            .enumerate()
            .map(|(x, c)| {
                let mut s = vec![0u8; blocksize];
                s[16..20].copy_from_slice(&((self.seq as u32 + x as u32) << 8).to_le_bytes());
                s[20..24].copy_from_slice(&idx.to_le_bytes());
                s[24] = 0x1 | if x == 0 { 0x4 } else { 0 } | if x == n - 1 { 0x2 } else { 0 };
                s[26] = 0x10;
                s[32..32 + c.len()].copy_from_slice(c);
                s
            })
            .collect();
        let parity = (n * FEC_PCT).div_ceil(100);
        shards.extend((0..parity).map(|_| vec![0u8; blocksize]));
        rs_encode(n, parity, &mut shards).unwrap();
        for (x, s) in shards.iter_mut().enumerate() {
            let fec = (x << 12 | n << 22 | FEC_PCT << 4) as u32;
            s[28..32].copy_from_slice(&fec.to_le_bytes());
            s[0] = 0x90;
            s[2..4].copy_from_slice(&self.seq.wrapping_add(x as u16).to_be_bytes());
            s[4..8].copy_from_slice(&(idx.wrapping_mul(1500)).to_be_bytes());
            s[20..24].copy_from_slice(&idx.to_le_bytes());
            s[27] = 0;
        }
        self.seq = self.seq.wrapping_add(shards.len() as u16);
        (shards, parity)
    }

    /// `SS_ENC_VIDEO`: iv = LE u64 counter with iv[11] = 'V'; prefix iv, frameNumber LE, tag.
    fn seal(&mut self, key: &[u8; 16], idx: u32, plain: &[u8]) -> Vec<u8> {
        let mut iv = [0u8; 12];
        iv[..8].copy_from_slice(&self.gcm_counter.to_le_bytes());
        iv[11] = b'V';
        self.gcm_counter += 1;
        let mut ct = plain.to_vec();
        let tag = Aes128Gcm::new(key.into())
            .encrypt_in_place_detached(Nonce::from_slice(&iv), &[], &mut ct)
            .unwrap();
        let mut w = iv.to_vec();
        w.extend(idx.to_le_bytes());
        w.extend(tag);
        w.extend(ct);
        w
    }
}

/// `encode_audio`: AES-128-CBC/PKCS7, IV = BE32(rikeyid + seq) then zeros.
fn cbc(key: &[u8; 16], key_id: u32, seq: u16, plain: &[u8]) -> Vec<u8> {
    let aes = Aes128::new(key.into());
    let mut data = plain.to_vec();
    let pad = 16 - plain.len() % 16;
    data.extend(std::iter::repeat_n(pad as u8, pad));
    let mut prev = [0u8; 16];
    prev[..4].copy_from_slice(&key_id.wrapping_add(seq as u32).to_be_bytes());
    for c in data.chunks_mut(16) {
        for (b, p) in c.iter_mut().zip(prev) {
            *b ^= p;
        }
        let mut blk = GenericArray::clone_from_slice(c);
        aes.encrypt_block(&mut blk);
        c.copy_from_slice(&blk);
        prev.copy_from_slice(c);
    }
    data
}

fn rtp(pt: u8, seq: u16, ts: u32, body: &[u8]) -> Vec<u8> {
    let mut v = vec![0x80, pt];
    v.extend(seq.to_be_bytes());
    v.extend(ts.to_be_bytes());
    v.extend([0; 4]);
    v.extend(body);
    v
}

/// One 4+2 audio block starting at `base` (seq % 4 == 0), as `audioBroadcastThread` sends it.
fn audio_block(base: u16, key: Option<([u8; 16], u32)>) -> Vec<Vec<u8>> {
    let ts = |i: u16| u32::from(base.wrapping_add(i)).wrapping_mul(5);
    let mut shards: Vec<Vec<u8>> = (0..4u16)
        .map(|i| {
            let seq = base.wrapping_add(i);
            let opus: Vec<u8> = (0..120u8).map(|b| b ^ seq as u8).collect();
            match key {
                Some((k, id)) => cbc(&k, id, seq, &opus),
                None => opus,
            }
        })
        .collect();
    let len = shards[0].len();
    shards.extend([vec![0; len], vec![0; len]]);
    encode_parity(&mut shards).unwrap();
    let mut out: Vec<Vec<u8>> = (0..4u16)
        .map(|i| rtp(PT_AUDIO, base.wrapping_add(i), ts(i), &shards[i as usize]))
        .collect();
    for x in 0..2u16 {
        let mut body = vec![x as u8, PT_AUDIO];
        body.extend(base.to_be_bytes());
        body.extend(ts(0).to_be_bytes());
        body.extend([0; 4]);
        body.extend(&shards[4 + x as usize]);
        out.push(rtp(PT_FEC, base.wrapping_add(4 + x), 0, &body));
    }
    out
}

pub fn fake_with(cfg: Cfg) -> Fake {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = sock.local_addr().unwrap();
    let av = [
        UdpSocket::bind("127.0.0.1:0").unwrap(),
        UdpSocket::bind("127.0.0.1:0").unwrap(),
    ];
    let (video, audio) = (av[0].local_addr().unwrap(), av[1].local_addr().unwrap());
    for s in &av {
        s.set_nonblocking(true).unwrap();
    }
    let log = Arc::new(Mutex::new(Log::default()));
    let (tx, rx) = mpsc::channel::<Cmd>();
    let l = log.clone();
    std::thread::spawn(move || {
        let Cfg {
            key,
            connect_data,
            ping_payload,
            drop,
            media,
            tap,
        } = cfg;
        let mut tap_sock: Option<socket2::Socket> = None;
        // net::host_create: 128 peers, channel limit 0 (= max), no bandwidth limits.
        let settings = enet::HostSettings {
            peer_limit: 128,
            channel_limit: 255,
            ..Default::default()
        };
        let mut host = enet::Host::new(Lossy { sock, drop }, settings).unwrap();
        let cipher = Aes128Gcm::new(&key.into());
        let mut peer: Option<enet::PeerID> = None;
        let mut seq: u32 = 0;
        // Sunshine starts videoThread/audioThread at PLAY (stream.cpp:2289-2293); the fake is
        // created right where PLAY would have completed.
        let started = Instant::now();
        let mut ping_peer: [Option<SocketAddr>; 2] = [None, None];
        let mut media_start: [Option<Instant>; 2] = [None, None];
        let mut vh = VideoHost {
            seq: 1000,
            gcm_counter: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
        };
        let mut next_frame: u32 = 1;
        let mut idr_pending = true;
        let mut audio_blocks: u32 = 0;
        let mut stopped = false;
        let mut buf = [0u8; 2048];
        while started.elapsed() < Duration::from_secs(30) {
            // recv_ping (stream.cpp:2092-2120): any datagram containing the payload is a ping and
            // its source becomes the media peer.
            for (i, s) in av.iter().enumerate() {
                while let Ok((n, from)) = s.recv_from(&mut buf) {
                    let d = &buf[..n];
                    if !d.windows(16).any(|w| w == ping_payload.as_bytes()) {
                        continue;
                    }
                    let mut log = l.lock().unwrap();
                    log.av_pings[i] += 1;
                    if ping_peer[i].is_none() && !stopped {
                        ping_peer[i] = Some(from);
                        media_start[i] = Some(Instant::now());
                        log.first_ping[i] = Some(d.to_vec());
                        if let (0, Some(path)) = (i, &tap) {
                            let t = socket2::Socket::new(
                                socket2::Domain::UNIX,
                                socket2::Type::SEQPACKET,
                                None,
                            )
                            .unwrap();
                            if t.connect(&socket2::SockAddr::unix(path).unwrap()).is_ok() {
                                tap_sock = Some(t);
                            }
                        }
                        if media.is_none() {
                            // Media-like junk so the counters have something to count.
                            for k in 0..3 - i {
                                s.send_to(&[0x80 + k as u8; 100], from).unwrap();
                            }
                        }
                    }
                }
            }
            if let (Some(m), false) = (media, stopped) {
                if let (Some(to), Some(t0)) = (ping_peer[0], media_start[0]) {
                    let due = (t0.elapsed().as_secs_f64() * 60.0) as u32 + 1;
                    while next_frame <= due {
                        let idx = next_frame;
                        next_frame += 1;
                        let idr = idr_pending && (m.answer_idr || idx == 1);
                        idr_pending &= !idr;
                        let size = if idr {
                            30_000
                        } else {
                            2_000 + (idx as usize * 397) % 2_500
                        };
                        let annexb: Vec<u8> = (0..size).map(|b| (b as u32 ^ idx) as u8).collect();
                        if let Some(t) = &tap_sock {
                            // brisk_tap_send: header + Annex B, one message, never blocking.
                            let now = brisk_client::tap::monotonic_ns();
                            let mut msg = b"BTAP".to_vec();
                            msg.extend([1, idr as u8]);
                            msg.extend(44u16.to_le_bytes());
                            msg.extend(connect_data.to_le_bytes());
                            msg.extend(0u32.to_le_bytes());
                            msg.extend((idx as u64).to_le_bytes());
                            msg.extend((now - 2_000_000).to_le_bytes()); // captured 2 ms ago
                            msg.extend(now.to_le_bytes());
                            msg.extend(0u32.to_le_bytes());
                            msg.extend(&annexb);
                            if t.send_with_flags(&msg, libc::MSG_DONTWAIT).is_ok() {
                                let mut log = l.lock().unwrap();
                                log.frames += 1;
                                log.idr_frames += u32::from(idr);
                            }
                            continue;
                        }
                        let (pkts, parity) = vh.frame(
                            idx,
                            if idr { 2 } else { 1 },
                            30 + (idx % 20) as u16,
                            &annexb,
                        );
                        let mut dropped = 0;
                        let mut log = l.lock().unwrap();
                        if m.corrupt_frame == Some(idx) {
                            av[0].send_to(&[0u8; 100], to).unwrap();
                        }
                        let lose = if (m.lose_frame)(idx) { parity + 1 } else { 0 };
                        for (k, p) in pkts.into_iter().enumerate() {
                            if k < lose {
                                log.video_dropped += 1;
                                continue;
                            }
                            if dropped < parity && rnd(&mut vh.rng) % 100 < m.loss_pct as u64 {
                                dropped += 1;
                                log.video_dropped += 1;
                                continue;
                            }
                            let w = match &m.video_key {
                                Some(k) => vh.seal(k, idx, &p),
                                None => p,
                            };
                            av[0].send_to(&w, to).unwrap();
                            log.video_sent += 1;
                        }
                        log.frames += 1;
                        log.idr_frames += u32::from(idr);
                    }
                }
                if let (Some(to), Some(t0)) = (ping_peer[1], media_start[1]) {
                    // One block (4 packets of 5 ms) every 20 ms.
                    let due = (t0.elapsed().as_millis() / 20) as u32 + 1;
                    while audio_blocks < due {
                        let base = (audio_blocks * 4) as u16;
                        audio_blocks += 1;
                        for p in audio_block(base, m.audio_key) {
                            av[1].send_to(&p, to).unwrap();
                            l.lock().unwrap().audio_sent += 1;
                        }
                    }
                }
            }
            if !stopped
                && started.elapsed() >= FAKE_PING_TIMEOUT
                && ping_peer.iter().any(|p| p.is_none())
            {
                // "Initial Ping Timeout" -> fail_guard session::stop -> broadcast loop
                // enet_peer_disconnect_now (stream.cpp:1336-1349).
                stopped = true;
                l.lock().unwrap().stopped = true;
                if let Some(id) = peer.take() {
                    host.peer_mut(id).disconnect_now(0);
                }
            }
            while let Ok(c) = rx.try_recv() {
                let Some(id) = peer else { continue };
                let mut seal = |t: u16, p: &[u8]| {
                    let mut pt = t.to_le_bytes().to_vec();
                    pt.extend_from_slice(&(p.len() as u16).to_le_bytes());
                    pt.extend_from_slice(p);
                    let tag = cipher
                        .encrypt_in_place_detached(Nonce::from_slice(&iv(seq, b'H')), b"", &mut pt)
                        .unwrap();
                    let mut w = 1u16.to_le_bytes().to_vec();
                    w.extend_from_slice(&((pt.len() + 16 + 4) as u16).to_le_bytes());
                    w.extend_from_slice(&seq.to_le_bytes());
                    w.extend_from_slice(&tag);
                    w.extend_from_slice(&pt);
                    seq += 1;
                    w
                };
                let p = host.peer_mut(id);
                match c {
                    Cmd::Send(t, payload) => {
                        let w = seal(t, &payload);
                        p.send(0, &enet::Packet::reliable(w.as_slice())).unwrap();
                    }
                    Cmd::SendRaw(w) => {
                        p.send(0, &enet::Packet::reliable(w.as_slice())).unwrap();
                    }
                    Cmd::Terminate => {
                        let w = seal(0x0109, &0x8003_0023u32.to_be_bytes());
                        p.send(0, &enet::Packet::reliable(w.as_slice())).unwrap();
                        host.flush();
                        host.peer_mut(id).disconnect_now(0);
                        peer = None;
                    }
                }
            }
            loop {
                let ev = match host.service() {
                    Ok(Some(e)) => e.no_ref(),
                    _ => break,
                };
                let mut log = l.lock().unwrap();
                match ev {
                    enet::EventNoRef::Connect { peer: id, data } => {
                        log.connects.push(data);
                        if data == connect_data && !stopped {
                            peer = Some(id);
                        } else {
                            log.rejected += 1;
                            host.peer_mut(id).disconnect_now(0);
                        }
                    }
                    enet::EventNoRef::Disconnect { .. } => {
                        log.disconnects += 1;
                        peer = None;
                    }
                    enet::EventNoRef::Receive {
                        channel_id, packet, ..
                    } => {
                        let d = packet.data();
                        if d.len() < 2 || u16::from_le_bytes([d[0], d[1]]) != 1 {
                            log.unencrypted += 1; // controlProtocolType 13 drops these
                            continue;
                        }
                        if d.len() < 8 {
                            continue;
                        }
                        let len = u16::from_le_bytes([d[2], d[3]]) as usize;
                        if len < 24 || d.len() < 4 + len {
                            continue; // "Control: Runt packet"
                        }
                        let s = u32::from_le_bytes(d[4..8].try_into().unwrap());
                        let mut pt = d[24..4 + len].to_vec();
                        let ok = cipher.decrypt_in_place_detached(
                            Nonce::from_slice(&iv(s, b'C')),
                            b"",
                            &mut pt,
                            Tag::from_slice(&d[8..24]),
                        );
                        if ok.is_err() {
                            log.bad_tag += 1;
                            continue;
                        }
                        let t = u16::from_le_bytes([pt[0], pt[1]]);
                        // IDX_REQUEST_IDR_FRAME: the encoder's next frame is an IDR.
                        idr_pending |= t == 0x0302;
                        log.msgs.push((channel_id, t, pt[4..].to_vec()));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    Fake {
        addr,
        video,
        audio,
        log,
        tx,
    }
}

pub fn types(f: &Fake) -> Vec<(u8, u16)> {
    let l = f.log.lock().unwrap();
    l.msgs.iter().map(|(c, t, _)| (*c, *t)).collect()
}

pub async fn av(f: &Fake) -> Option<AvPing> {
    let ip = f.video.ip();
    Some(
        AvPing::new(ip, f.video.port(), f.audio.port(), PING_PAYLOAD)
            .await
            .unwrap(),
    )
}
