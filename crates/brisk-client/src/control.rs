//! Control-stream session over tokio UDP, after RTSP PLAY: ENet connect to the SETUP(control)
//! port with `X-SS-Connect-Data`, then what moonlight's `startControlStream` does for a Gen7Enc
//! host: the index-0 message (IDR request on Gen7Enc) and Start B, a 100 ms periodic ping
//! (`lossStatsThreadFunc` with `usePeriodicPing`), host-message decode, graceful disconnect
//! (`gracefullyDisconnectEnetPeer`, 2 s linger).
//!
//! [`AvPing`] keeps Sunshine's media threads alive: they wait `ping_timeout` (10 s) for a UDP
//! datagram containing the `X-SS-Ping-Payload` on the video and audio ports and stop the whole
//! session otherwise (stream.cpp:2092-2163). moonlight sends `SS_PING` every 500 ms
//! (VideoStream.c:55-82, AudioStream.c:38-64); the ping's source port becomes the media peer.

use brisk_proto::control::{channel, Control, ControlMsg};
use brisk_proto::enet::{EnetEvent, EnetPeer, PeerState, MAX_DATAGRAM};
use brisk_proto::input::InputEvent;
use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// moonlight CONTROL_STREAM_TIMEOUT_SEC.
pub const CONNECT_WAIT: Duration = Duration::from_secs(10);
/// moonlight CONTROL_STREAM_LINGER_TIMEOUT_SEC.
pub const LINGER: Duration = Duration::from_secs(2);
/// moonlight PERIODIC_PING_INTERVAL_MS.
pub const PING_EVERY: Duration = Duration::from_millis(100);
/// moonlight's video/audio ping interval.
pub const AV_PING_EVERY: Duration = Duration::from_millis(500);
const MEDIA_BUF: usize = 4096;
/// Media datagrams that arrive during the ENet connect are kept for the first
/// [`ControlSession::run_until_with`] (the opening IDR can land there).
const EARLY_MAX: usize = 4096;

/// Receives every video (index 0) or audio (index 1) datagram as it arrives; returns true when
/// the decoder needs an IDR (a frame was lost), which the session requests rate-limited.
pub type MediaSink<'a> = &'a mut dyn FnMut(usize, &[u8], Instant) -> bool;
/// Default [`ControlSession::idr_min_interval`].
pub const IDR_MIN_INTERVAL: Duration = Duration::from_millis(500);
/// [`ControlHandle::request_idr`] calls closer together than this are merged into one request.
pub const IDR_CMD_INTERVAL: Duration = Duration::from_millis(100);
/// Commands a [`ControlHandle`] can queue before `send` waits for the session.
const CMD_QUEUE: usize = 64;
/// A handle call waits at most this long for room in the queue (the session is not being run).
const CMD_WAIT: Duration = Duration::from_secs(1);

enum Cmd {
    Idr,
    /// One input event, already encoded by the handle (so its error goes to the caller).
    Input(Vec<ControlMsg>),
}

/// Cloneable sender into a running [`ControlSession`]; the session acts on commands while it is
/// inside [`ControlSession::run_until_with`] (or `run_until`).
#[derive(Clone)]
pub struct ControlHandle(mpsc::Sender<Cmd>);

impl ControlHandle {
    async fn send(&self, c: Cmd) -> crate::Result<()> {
        use mpsc::error::SendTimeoutError;
        match self.0.send_timeout(c, CMD_WAIT).await {
            Ok(()) => Ok(()),
            Err(SendTimeoutError::Closed(_)) => Err(crate::Error::Closed("control session")),
            Err(SendTimeoutError::Timeout(_)) => Err(crate::Error::Timeout(format!(
                "control session took no command for {}s (not running?)",
                CMD_WAIT.as_secs()
            ))),
        }
    }

    /// Asks for an IDR frame; requests within [`IDR_CMD_INTERVAL`] of the last IDR request are
    /// merged and sent when the interval is up.
    pub async fn request_idr(&self) -> crate::Result<()> {
        self.send(Cmd::Idr).await
    }

    pub async fn send_input(&self, ev: InputEvent) -> crate::Result<()> {
        self.send(Cmd::Input(ControlMsg::input(&ev)?)).await
    }
}

/// `--input-demo`: a harmless sequence that ends where it started. Mouse +5 then -5 px, Shift
/// down then up, gamepad 0 attached then detached with nothing pressed and sticks centred
/// (Sunshine allocates the pad on its `activeGamepadMask` bit and frees it when the bit clears,
/// input.cpp:1575-1580). No clicks, no other keys, no stick or trigger movement.
pub const INPUT_DEMO: [InputEvent; 6] = [
    InputEvent::MouseMoveRel { dx: 5, dy: 0 },
    InputEvent::MouseMoveRel { dx: -5, dy: 0 },
    InputEvent::Key {
        vk: 0x10, // VK_SHIFT
        down: true,
        modifiers: 0,
    },
    InputEvent::Key {
        vk: 0x10,
        down: false,
        modifiers: 0,
    },
    neutral_pad(1),
    neutral_pad(0),
];
/// Pause after each [`INPUT_DEMO`] event, so the host sees separate events.
pub const INPUT_DEMO_GAP: Duration = Duration::from_millis(50);

const fn neutral_pad(active_mask: u16) -> InputEvent {
    InputEvent::Gamepad {
        index: 0,
        active_mask,
        buttons: 0,
        lt: 0,
        rt: 0,
        lx: 0,
        ly: 0,
        rx: 0,
        ry: 0,
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stats {
    pub pings_sent: u32,
    pub pings_acked: u32,
    pub idr_sent: u32,
    /// Of `idr_sent`: requests made because the sink reported a lost frame.
    pub idr_on_loss: u32,
    pub inputs_sent: u32,
    /// Decoded host messages by packet type.
    pub host_msgs: BTreeMap<u16, u32>,
    /// Host packets that failed to open (unencrypted, bad tag, replay, runt).
    pub bad_msgs: u32,
    pub termination: Option<u32>,
    /// Last rumble seen: (controller, low, high).
    pub last_rumble: Option<(u16, u16, u16)>,
    /// The host closed the ENet connection (or it timed out).
    pub host_disconnected: bool,
    /// SS_PING datagrams sent to [video, audio].
    pub av_pings_sent: [u32; 2],
    /// Datagrams / bytes received on the [video, audio] sockets (not parsed).
    pub media_rx_packets: [u64; 2],
    pub media_rx_bytes: [u64; 2],
}

/// Video and audio UDP sockets (index 0 video, 1 audio) that ping Sunshine and count what comes
/// back. Later tasks receive media on these same sockets.
pub struct AvPing {
    socks: [UdpSocket; 2],
    targets: [SocketAddr; 2],
    payload: [u8; 16],
    seq: u32,
    next: Instant,
    bufs: [Vec<u8>; 2],
}

impl AvPing {
    /// `payload` is `X-SS-Ping-Payload` (16 characters, sent as ASCII as moonlight does).
    pub async fn new(
        host: std::net::IpAddr,
        video: u16,
        audio: u16,
        payload: &str,
    ) -> crate::Result<Self> {
        let payload: [u8; 16] = payload.as_bytes().try_into().map_err(|_| {
            crate::Error::Protocol(format!(
                "X-SS-Ping-Payload must be 16 characters, got {payload:?}"
            ))
        })?;
        let local: SocketAddr = if host.is_ipv4() {
            "0.0.0.0:0".parse().unwrap()
        } else {
            "[::]:0".parse().unwrap()
        };
        Ok(AvPing {
            socks: [
                bind_rcvbuf(local, VIDEO_RCVBUF)?,
                bind_rcvbuf(local, AUDIO_RCVBUF)?,
            ],
            targets: [SocketAddr::new(host, video), SocketAddr::new(host, audio)],
            payload,
            seq: 0,
            next: Instant::now(),
            bufs: [vec![0; MEDIA_BUF], vec![0; MEDIA_BUF]],
        })
    }

    /// Effective SO_RCVBUF of the (video, audio) sockets; the kernel doubles and may clamp it.
    pub fn rcvbuf(&self) -> (usize, usize) {
        let get = |s| socket2::SockRef::from(s).recv_buffer_size().unwrap_or(0);
        (get(&self.socks[0]), get(&self.socks[1]))
    }

    /// `SS_PING { payload[16], sequenceNumber u32 BE }`, counted from 1 (Video.h:51-54).
    pub fn ping_bytes(payload: &[u8; 16], seq: u32) -> [u8; 20] {
        let mut p = [0u8; 20];
        p[..16].copy_from_slice(payload);
        p[16..].copy_from_slice(&seq.to_be_bytes());
        p
    }

    async fn ping(&mut self, stats: &mut Stats) {
        self.seq += 1;
        let p = Self::ping_bytes(&self.payload, self.seq);
        for i in 0..2 {
            // Errors ignored like moonlight (ICMP before the host binds is expected).
            if self.socks[i].send_to(&p, self.targets[i]).await.is_ok() {
                stats.av_pings_sent[i] += 1;
            }
        }
        self.next = Instant::now() + AV_PING_EVERY;
    }
}

/// Requested SO_RCVBUF for the video and audio sockets (net.core.rmem_max may clamp them).
const VIDEO_RCVBUF: usize = 2 << 20;
const AUDIO_RCVBUF: usize = 256 << 10;

/// Bound UDP socket with a bigger receive buffer; a refused size is not an error.
fn bind_rcvbuf(local: SocketAddr, rcvbuf: usize) -> std::io::Result<UdpSocket> {
    let std_sock = std::net::UdpSocket::bind(local)?;
    let _ = socket2::SockRef::from(&std_sock).set_recv_buffer_size(rcvbuf);
    std_sock.set_nonblocking(true)?;
    UdpSocket::from_std(std_sock)
}

async fn recv_opt(s: Option<(&UdpSocket, &mut Vec<u8>)>) -> std::io::Result<usize> {
    match s {
        Some((s, b)) => s.recv_from(b).await.map(|(n, _)| n),
        None => std::future::pending().await,
    }
}

enum Got {
    Ctl(std::io::Result<usize>),
    Media(usize, std::io::Result<usize>),
    Cmd(Cmd),
    Timer,
}

pub struct ControlSession {
    sock: UdpSocket,
    peer: EnetPeer,
    ctl: Control,
    epoch: Instant,
    pings: HashSet<u16>,
    next_ping: Instant,
    /// We started the disconnect: the final Disconnected event is ours, not the host's.
    closing: bool,
    buf: Vec<u8>,
    av: Option<AvPing>,
    early: Vec<(usize, Instant, Vec<u8>)>,
    /// The sink reported a lost frame; an IDR request is owed.
    want_idr: bool,
    /// A [`ControlHandle`] asked for an IDR.
    want_idr_cmd: bool,
    cmd_tx: mpsc::Sender<Cmd>,
    cmd_rx: mpsc::Receiver<Cmd>,
    last_idr: Option<Instant>,
    /// Loss-triggered IDR requests are at most one per this (a field so tests can shrink it).
    pub idr_min_interval: Duration,
    pub stats: Stats,
}

impl ControlSession {
    /// ENet connect (10 s), then IDR request + Start B. `connect_id` must be random. `av`
    /// pings start at once (before the ENet connect) and run for the life of the session.
    pub async fn connect(
        addr: SocketAddr,
        connect_data: u32,
        rikey: [u8; 16],
        connect_id: u32,
        av: Option<AvPing>,
    ) -> crate::Result<Self> {
        let local: SocketAddr = if addr.is_ipv4() {
            "0.0.0.0:0".parse().unwrap()
        } else {
            "[::]:0".parse().unwrap()
        };
        let sock = UdpSocket::bind(local).await?;
        sock.connect(addr).await?;
        let epoch = Instant::now();
        let (peer, hello) = EnetPeer::connect(connect_data, connect_id, 0);
        let (cmd_tx, cmd_rx) = mpsc::channel(CMD_QUEUE);
        let mut s = ControlSession {
            sock,
            peer,
            ctl: Control::new(rikey),
            epoch,
            pings: HashSet::new(),
            next_ping: epoch,
            closing: false,
            buf: vec![0u8; MAX_DATAGRAM + 1],
            av,
            early: Vec::new(),
            want_idr: false,
            want_idr_cmd: false,
            cmd_tx,
            cmd_rx,
            last_idr: None,
            idr_min_interval: IDR_MIN_INTERVAL,
            stats: Stats::default(),
        };
        s.sock.send(&hello).await?;
        let deadline = epoch + CONNECT_WAIT;
        while s.peer.state() == PeerState::Connecting {
            if Instant::now() >= deadline {
                return Err(crate::Error::Timeout(format!(
                    "control: no ENet connect from {addr} within 10s"
                )));
            }
            s.step(deadline, None).await?;
        }
        if s.peer.state() != PeerState::Connected {
            return Err(crate::Error::Session(format!(
                "control: {addr} refused the ENet connection"
            )));
        }
        // Gen7Enc's index-0 slot ("Start A" on older hosts) is the IDR request, sent on the
        // generic channel (ControlStream.c:1869-1875); later IDR requests use URGENT.
        s.send_on(&ControlMsg::IdrRequest, channel::GENERIC, true)
            .await?;
        s.stats.idr_sent += 1;
        s.last_idr = Some(Instant::now());
        s.send(&ControlMsg::StartB).await?;
        Ok(s)
    }

    fn now(&self) -> u32 {
        self.epoch.elapsed().as_millis() as u32
    }

    /// A sender for IDR requests and input from other tasks.
    pub fn handle(&self) -> ControlHandle {
        ControlHandle(self.cmd_tx.clone())
    }

    pub fn is_open(&self) -> bool {
        self.peer.state() == PeerState::Connected
    }

    pub async fn send(&mut self, m: &ControlMsg) -> crate::Result<()> {
        let (ch, reliable) = m.route();
        self.send_on(m, ch, reliable).await
    }

    async fn send_on(&mut self, m: &ControlMsg, ch: u8, reliable: bool) -> crate::Result<()> {
        let w = self.ctl.encode(m)?;
        let d = self.peer.send(ch, reliable, &w, self.now())?;
        self.sock.send(&d).await?;
        Ok(())
    }

    pub async fn request_idr(&mut self) -> crate::Result<()> {
        self.send(&ControlMsg::IdrRequest).await?;
        self.stats.idr_sent += 1;
        self.last_idr = Some(Instant::now());
        self.want_idr = false; // this request also answers any loss reported so far
        self.want_idr_cmd = false;
        Ok(())
    }

    pub async fn send_input(&mut self, ev: &InputEvent) -> crate::Result<()> {
        self.send_msgs(ControlMsg::input(ev)?).await
    }

    async fn send_msgs(&mut self, msgs: Vec<ControlMsg>) -> crate::Result<()> {
        for m in msgs {
            self.send(&m).await?;
            self.stats.inputs_sent += 1;
        }
        Ok(())
    }

    /// Sends [`INPUT_DEMO`], [`INPUT_DEMO_GAP`] apart, media still going to `sink`. Returns the
    /// number of input packets sent.
    pub async fn input_demo(&mut self, sink: MediaSink<'_>) -> crate::Result<u32> {
        let before = self.stats.inputs_sent;
        for ev in &INPUT_DEMO {
            self.send_input(ev).await?;
            self.run_until_with(Instant::now() + INPUT_DEMO_GAP, &mut *sink)
                .await?;
        }
        Ok(self.stats.inputs_sent - before)
    }

    /// Ping, receive and retransmit until `until`, or until the host ends the session.
    pub async fn run_until(&mut self, until: Instant) -> crate::Result<()> {
        self.run_until_with(until, &mut |_, _, _| false).await
    }

    /// [`Self::run_until`] that hands every media datagram to `sink`, starting with any that
    /// arrived during the connect, and requests an IDR (at most one per `idr_min_interval`)
    /// when `sink` reports a lost frame.
    pub async fn run_until_with(
        &mut self,
        until: Instant,
        sink: MediaSink<'_>,
    ) -> crate::Result<()> {
        for (i, at, d) in std::mem::take(&mut self.early) {
            self.want_idr |= sink(i, &d, at);
        }
        while self.is_open() && Instant::now() < until {
            let idr_due = self.last_idr.map(|t| t + IDR_CMD_INTERVAL);
            if self.want_idr_cmd && idr_due.is_none_or(|t| Instant::now() >= t) {
                self.request_idr().await?;
            }
            if self.want_idr
                && self
                    .last_idr
                    .is_none_or(|t| t.elapsed() >= self.idr_min_interval)
            {
                self.want_idr = false;
                self.request_idr().await?;
                self.stats.idr_on_loss += 1;
            }
            if Instant::now() >= self.next_ping {
                self.send(&ControlMsg::Ping).await?;
                self.pings.insert(self.peer.outgoing_seq(channel::GENERIC));
                self.stats.pings_sent += 1;
                self.next_ping = Instant::now() + PING_EVERY;
            }
            let mut wake = until.min(self.next_ping);
            if let (true, Some(t)) = (self.want_idr_cmd, idr_due) {
                wake = wake.min(t);
            }
            self.step(wake, Some(&mut *sink)).await?;
        }
        Ok(())
    }

    /// One wait: a datagram, an ENet timer, or `until`. Media goes to `sink`, or is kept for
    /// later while connecting (`None`).
    async fn step(&mut self, until: Instant, sink: Option<MediaSink<'_>>) -> crate::Result<()> {
        let r = self.step_inner(until, sink).await;
        // Ended or ending: handles get `Closed` from now on instead of a queue nobody reads.
        if self.closing
            || !matches!(
                self.peer.state(),
                PeerState::Connecting | PeerState::Connected
            )
        {
            self.cmd_rx.close();
        }
        r
    }

    async fn step_inner(
        &mut self,
        until: Instant,
        sink: Option<MediaSink<'_>>,
    ) -> crate::Result<()> {
        let (out, next) = match self.peer.poll_timeout(self.now()) {
            Ok(v) => v,
            Err(e) => {
                self.stats.host_disconnected = true;
                return Err(crate::Error::Timeout(format!(
                    "control: {e} (no ACK from host for 10s)"
                )));
            }
        };
        for d in out {
            self.sock.send(&d).await?;
        }
        let mut wake = until;
        if let Some(ms) = next {
            wake = wake.min(Instant::now() + Duration::from_millis(ms as u64));
        }
        if let Some(av) = &mut self.av {
            if Instant::now() >= av.next {
                av.ping(&mut self.stats).await;
            }
            wake = wake.min(av.next);
        }
        let got = {
            let ControlSession {
                sock,
                buf,
                av,
                cmd_rx,
                ..
            } = self;
            let (v, a) = match av {
                Some(av) => {
                    let [vs, as_] = &av.socks;
                    let [vb, ab] = &mut av.bufs;
                    (Some((vs, vb)), Some((as_, ab)))
                }
                None => (None, None),
            };
            tokio::select! {
                r = sock.recv(buf) => Got::Ctl(r),
                r = recv_opt(v) => Got::Media(0, r),
                r = recv_opt(a) => Got::Media(1, r),
                // never None: the session holds a sender itself
                Some(c) = cmd_rx.recv() => Got::Cmd(c),
                _ = tokio::time::sleep_until(wake.into()) => Got::Timer,
            }
        };
        match got {
            Got::Ctl(Ok(n)) => {
                let buf = std::mem::take(&mut self.buf);
                let out = self.on_datagram(&buf[..n]);
                self.buf = buf;
                for d in out {
                    self.sock.send(&d).await?;
                }
            }
            // ICMP port unreachable on a connected socket: keep waiting, as ENet does.
            Got::Ctl(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
            Got::Ctl(Err(e)) => return Err(e.into()),
            Got::Media(i, Ok(n)) => {
                let at = Instant::now();
                self.stats.media_rx_packets[i] += 1;
                self.stats.media_rx_bytes[i] += n as u64;
                let d = &self.av.as_ref().unwrap().bufs[i][..n];
                match sink {
                    Some(f) => self.want_idr |= f(i, d, at),
                    None if !self.closing && self.early.len() < EARLY_MAX => {
                        self.early.push((i, at, d.to_vec()))
                    }
                    None => {}
                }
            }
            // Commands that arrive while closing are dropped.
            Got::Cmd(_) if self.closing => {}
            Got::Cmd(Cmd::Idr) => self.want_idr_cmd = true,
            Got::Cmd(Cmd::Input(msgs)) => self.send_msgs(msgs).await?,
            Got::Media(..) | Got::Timer => {}
        }
        Ok(())
    }

    /// Returns datagrams to send right away.
    fn on_datagram(&mut self, d: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let Ok(events) = self.peer.handle_datagram(d, self.now()) else {
            return out; // not ours / malformed: ENet drops it too
        };
        for e in events {
            match e {
                EnetEvent::Receive { data, .. } => match self.ctl.decode(&data) {
                    Ok(m) => {
                        *self.stats.host_msgs.entry(m.kind()).or_default() += 1;
                        match m {
                            ControlMsg::Termination(ec) => {
                                self.stats.termination = Some(ec);
                                // moonlight: the host stops ACKing now; reset without waiting.
                                out.extend(self.peer.disconnect_now(self.now()));
                            }
                            ControlMsg::Rumble {
                                controller,
                                low,
                                high,
                            } => self.stats.last_rumble = Some((controller, low, high)),
                            _ => {}
                        }
                    }
                    Err(_) => self.stats.bad_msgs += 1,
                },
                EnetEvent::Acked { channel, seq } => {
                    if channel == channel::GENERIC && self.pings.remove(&seq) {
                        self.stats.pings_acked += 1;
                    }
                }
                EnetEvent::Disconnected { .. } => self.stats.host_disconnected |= !self.closing,
                EnetEvent::Connected => {}
            }
        }
        out
    }

    /// Graceful disconnect: DISCONNECT after in-flight data is acked, wait up to 2 s for its ACK,
    /// else reset. Returns the counters.
    pub async fn disconnect(mut self) -> crate::Result<Stats> {
        self.closing = true;
        self.cmd_rx.close();
        let now = self.now();
        let mut out = self.peer.disconnect(now);
        if self.stats.termination.is_some() {
            out.extend(self.peer.disconnect_now(now));
        }
        for d in out {
            self.sock.send(&d).await?;
        }
        let deadline = Instant::now() + LINGER;
        while self.peer.state() != PeerState::Disconnected && Instant::now() < deadline {
            if self
                .step(deadline, Some(&mut |_, _, _| false))
                .await
                .is_err()
            {
                break;
            }
        }
        if let Some(d) = self.peer.disconnect_now(self.now()) {
            self.sock.send(&d).await?;
        }
        // Flush ACKs owed (e.g. for the host's own DISCONNECT).
        if let Ok((out, _)) = self.peer.poll_timeout(self.now()) {
            for d in out {
                self.sock.send(&d).await?;
            }
        }
        Ok(self.stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Datagrams kept during the connect reach the sink with their own arrival time.
    #[tokio::test]
    async fn early_datagrams_keep_their_arrival_instant() {
        let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (peer, _) = EnetPeer::connect(1, 1, 0);
        let (cmd_tx, cmd_rx) = mpsc::channel(1);
        let t0 = Instant::now();
        let mut c = ControlSession {
            sock,
            peer,
            ctl: Control::new([0; 16]),
            epoch: t0,
            pings: HashSet::new(),
            next_ping: t0,
            closing: false,
            buf: Vec::new(),
            av: None,
            early: (0..3)
                .map(|k| (0, t0 - Duration::from_millis(100 - 17 * k), vec![k as u8]))
                .collect(),
            want_idr: false,
            want_idr_cmd: false,
            cmd_tx,
            cmd_rx,
            last_idr: None,
            idr_min_interval: IDR_MIN_INTERVAL,
            stats: Stats::default(),
        };
        let mut got = Vec::new();
        c.run_until_with(t0, &mut |i, d, at| {
            got.push((i, d[0], at));
            false
        })
        .await
        .unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!((got[1].2 - got[0].2).as_millis(), 17);
        assert_eq!((got[2].2 - got[1].2).as_millis(), 17);
    }
}
