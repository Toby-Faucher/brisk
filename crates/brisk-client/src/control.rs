//! Control-stream session over tokio UDP, after RTSP PLAY: ENet connect to the SETUP(control)
//! port with `X-SS-Connect-Data`, then what moonlight's `startControlStream` does for a Gen7Enc
//! host: the index-0 message (IDR request on Gen7Enc) and Start B, a 100 ms periodic ping
//! (`lossStatsThreadFunc` with `usePeriodicPing`), host-message decode, graceful disconnect
//! (`gracefullyDisconnectEnetPeer`, 2 s linger).

use brisk_proto::control::{channel, Control, ControlMsg};
use brisk_proto::enet::{EnetEvent, EnetPeer, PeerState, MAX_DATAGRAM};
use brisk_proto::input::InputEvent;
use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

/// moonlight CONTROL_STREAM_TIMEOUT_SEC.
pub const CONNECT_WAIT: Duration = Duration::from_secs(10);
/// moonlight CONTROL_STREAM_LINGER_TIMEOUT_SEC.
pub const LINGER: Duration = Duration::from_secs(2);
/// moonlight PERIODIC_PING_INTERVAL_MS.
pub const PING_EVERY: Duration = Duration::from_millis(100);

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Stats {
    pub pings_sent: u32,
    pub pings_acked: u32,
    pub idr_sent: u32,
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
    pub stats: Stats,
}

impl ControlSession {
    /// ENet connect (10 s), then IDR request + Start B. `connect_id` must be random.
    pub async fn connect(
        addr: SocketAddr,
        connect_data: u32,
        rikey: [u8; 16],
        connect_id: u32,
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
        let mut s = ControlSession {
            sock,
            peer,
            ctl: Control::new(rikey),
            epoch,
            pings: HashSet::new(),
            next_ping: epoch,
            closing: false,
            stats: Stats::default(),
        };
        s.sock.send(&hello).await?;
        let deadline = epoch + CONNECT_WAIT;
        while s.peer.state() == PeerState::Connecting {
            if Instant::now() >= deadline {
                return Err(format!("control: no ENet connect from {addr} within 10s").into());
            }
            s.step(deadline).await?;
        }
        if s.peer.state() != PeerState::Connected {
            return Err(format!("control: {addr} refused the ENet connection").into());
        }
        // Gen7Enc's index-0 slot ("Start A" on older hosts) is the IDR request.
        s.send(&ControlMsg::IdrRequest).await?;
        s.stats.idr_sent += 1;
        s.send(&ControlMsg::StartB).await?;
        Ok(s)
    }

    fn now(&self) -> u32 {
        self.epoch.elapsed().as_millis() as u32
    }

    pub fn is_open(&self) -> bool {
        self.peer.state() == PeerState::Connected
    }

    pub async fn send(&mut self, m: &ControlMsg) -> crate::Result<()> {
        let (ch, reliable) = m.route();
        let w = self.ctl.encode(m)?;
        let d = self.peer.send(ch, reliable, &w, self.now())?;
        self.sock.send(&d).await?;
        Ok(())
    }

    pub async fn request_idr(&mut self) -> crate::Result<()> {
        self.send(&ControlMsg::IdrRequest).await?;
        self.stats.idr_sent += 1;
        Ok(())
    }

    pub async fn send_input(&mut self, ev: &InputEvent) -> crate::Result<()> {
        for m in ControlMsg::input(ev).map_err(|e| format!("input: {e:?}"))? {
            self.send(&m).await?;
            self.stats.inputs_sent += 1;
        }
        Ok(())
    }

    /// Ping, receive and retransmit until `until`, or until the host ends the session.
    pub async fn run_until(&mut self, until: Instant) -> crate::Result<()> {
        while self.is_open() && Instant::now() < until {
            if Instant::now() >= self.next_ping {
                self.send(&ControlMsg::Ping).await?;
                self.pings.insert(self.peer.outgoing_seq(channel::GENERIC));
                self.stats.pings_sent += 1;
                self.next_ping = Instant::now() + PING_EVERY;
            }
            self.step(until.min(self.next_ping)).await?;
        }
        Ok(())
    }

    /// One wait: a datagram, an ENet timer, or `until`.
    async fn step(&mut self, until: Instant) -> crate::Result<()> {
        let (out, next) = match self.peer.poll_timeout(self.now()) {
            Ok(v) => v,
            Err(e) => {
                self.stats.host_disconnected = true;
                return Err(format!("control: {e} (no ACK from host for 10s)").into());
            }
        };
        for d in out {
            self.sock.send(&d).await?;
        }
        let mut wake = until;
        if let Some(ms) = next {
            wake = wake.min(Instant::now() + Duration::from_millis(ms as u64));
        }
        let mut buf = vec![0u8; MAX_DATAGRAM + 1];
        tokio::select! {
            r = self.sock.recv(&mut buf) => match r {
                Ok(n) => {
                    for d in self.on_datagram(&buf[..n]) {
                        self.sock.send(&d).await?;
                    }
                }
                // ICMP port unreachable on a connected socket: keep waiting, as ENet does.
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
                Err(e) => return Err(e.into()),
            },
            _ = tokio::time::sleep_until(wake.into()) => {}
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
            if self.step(deadline).await.is_err() {
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
