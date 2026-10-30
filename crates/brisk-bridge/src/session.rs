//! Tap-to-browser video: [`Pacer`] is the drop policy (sans-io), [`Hist`] the latency counter,
//! [`forward`] the loop that runs them between a tap connection, the WebTransport endpoint and
//! the control session's [`ControlHandle`].
//!
//! Each frame goes out on its own uni stream: [`HEADER_LEN`]-byte header, Annex B, FIN. The
//! stream's send priority is the frame index, so under congestion quinn sends newer frames first
//! (research/m2-tap-design.md section 6). A stream counts as in flight until the browser has
//! acknowledged all of it.
//!
//! Audio keeps Sunshine's RTP path: [`AudioRx`] runs the M1 RS 4+2 recovery and AES-CBC
//! decryption, and each Opus packet goes out unchanged as one WebTransport datagram
//! ([`audio_datagram`]). Datagrams are never retransmitted; when quinn's bounded send buffer
//! ([`wt::AUDIO_SEND_BUFFER`]) is full the oldest are evicted and counted. Audio never asks for
//! an IDR.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use brisk_client::Error;
use brisk_client::control::ControlHandle;
use brisk_client::tap::{TapConn, TapFrame, monotonic_ns};
use brisk_proto::audio::{AudioCipher, AudioFec, OpusFrame};
use brisk_proto::sdp::SS_ENC_AUDIO;
use bytes::Bytes;
use tokio::sync::{mpsc, oneshot};

use crate::{input, wt};

/// More unacknowledged frame streams than this means the browser is behind.
pub const MAX_BEHIND: usize = 3;
/// At most one IDR request per this many ns (the tap's rule too).
pub const IDR_EVERY_NS: u64 = 100_000_000;
/// While the browser acknowledges nothing (a dead or frozen tab), the gap between IDR requests
/// doubles up to this, so Sunshine is not made to encode an IDR 10 times a second for nobody.
pub const IDR_MAX_GAP_NS: u64 = 2_000_000_000;
/// At most one refresh IDR ([`MSG_REFRESH_KEY`]) per this many ns, apart from the want-key limiter.
pub const REFRESH_EVERY_NS: u64 = 1_000_000_000;

/// What to do with one tap frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Decision {
    /// Open a stream for this frame.
    pub send: bool,
    /// Reset the streams of these frames (oldest first).
    pub reset: Vec<u64>,
    /// Ask Sunshine for an IDR now.
    pub request_idr: bool,
}

/// Counters for the browser-leg drop policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PacerStats {
    /// Frames let through.
    pub sent: u64,
    /// Frames dropped because the viewer fell behind.
    pub dropped: u64,
    /// Times the pacer went back to waiting for a key frame.
    pub reset: u64,
    /// IDRs requested because of drops.
    pub idr_requests: u64,
    /// IDRs requested for [`MSG_REFRESH_KEY`] (nothing dropped for them).
    pub refreshes: u64,
}

impl std::ops::AddAssign for PacerStats {
    fn add_assign(&mut self, o: Self) {
        self.sent += o.sent;
        self.dropped += o.dropped;
        self.reset += o.reset;
        self.idr_requests += o.idr_requests;
        self.refreshes += o.refreshes;
    }
}

/// The browser-leg drop policy for one viewer. It starts waiting for a key frame (the decoder
/// needs one after `configure`). Holds at most `MAX_BEHIND + 1` frame indices.
pub struct Pacer {
    in_flight: VecDeque<u64>,
    want_key: bool,
    last_idr_ns: Option<u64>,
    idr_gap_ns: u64,
    /// A frame was acknowledged since the last IDR request.
    acked: bool,
    last_refresh_ns: Option<u64>,
    /// What the pacer has counted so far.
    pub stats: PacerStats,
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Pacer {
    /// A pacer that waits for a key frame.
    pub fn new() -> Self {
        Self {
            in_flight: VecDeque::with_capacity(MAX_BEHIND + 1),
            want_key: true,
            last_idr_ns: None,
            idr_gap_ns: IDR_EVERY_NS,
            acked: false,
            last_refresh_ns: None,
            stats: PacerStats::default(),
        }
    }

    /// Decides what to do with frame `idx` arriving at `now_ns`.
    pub fn on_frame(&mut self, idx: u64, idr: bool, now_ns: u64) -> Decision {
        let mut d = Decision::default();
        if self.in_flight.len() > MAX_BEHIND {
            d.reset = self.in_flight.drain(..).collect();
            self.stats.reset += d.reset.len() as u64;
            self.want_key = true;
        }
        if self.want_key && !idr {
            self.stats.dropped += 1;
            d.request_idr = self.idr_due(now_ns);
            return d;
        }
        self.want_key = false;
        self.in_flight.push_back(idx);
        self.stats.sent += 1;
        d.send = true;
        d
    }

    /// The frame's stream was acknowledged (`acked`) or failed: it no longer counts as behind.
    pub fn on_done(&mut self, idx: u64, acked: bool) {
        self.in_flight.retain(|&i| i != idx);
        if acked {
            self.acked = true;
            self.idr_gap_ns = IDR_EVERY_NS;
        }
    }

    /// The browser needs a key frame (its decoder fell behind or saw a gap): drop until the next
    /// IDR. Returns whether to request one now.
    pub fn want_key(&mut self, now_ns: u64) -> bool {
        self.want_key = true;
        self.idr_due(now_ns)
    }

    /// The browser wants a fresh key frame but its decoder is fine (the replay buffer): request
    /// one, at most every [`REFRESH_EVERY_NS`]; frames keep flowing. Returns whether to request now.
    pub fn refresh_key(&mut self, now_ns: u64) -> bool {
        if self
            .last_refresh_ns
            .is_some_and(|t| now_ns.saturating_sub(t) < REFRESH_EVERY_NS)
        {
            return false;
        }
        self.last_refresh_ns = Some(now_ns);
        self.stats.refreshes += 1;
        true
    }

    /// Frames sent and not yet acknowledged.
    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    fn idr_due(&mut self, now_ns: u64) -> bool {
        if let Some(t) = self.last_idr_ns {
            if now_ns.saturating_sub(t) < self.idr_gap_ns {
                return false;
            }
            if !self.acked {
                self.idr_gap_ns = (self.idr_gap_ns * 2).min(IDR_MAX_GAP_NS);
            }
        }
        self.acked = false;
        self.last_idr_ns = Some(now_ns);
        self.stats.idr_requests += 1;
        true
    }
}

/// Latency histogram in 0.1 ms buckets up to 100 ms (the last bucket takes everything above):
/// constant memory however long the bridge runs.
pub struct Hist {
    buckets: Vec<u64>,
    n: u64,
}

const BUCKET_NS: u64 = 100_000;
const BUCKETS: usize = 1001;

impl Default for Hist {
    fn default() -> Self {
        Self {
            buckets: vec![0; BUCKETS],
            n: 0,
        }
    }
}

impl Hist {
    /// Counts one sample of `ns` nanoseconds.
    pub fn add(&mut self, ns: u64) {
        self.buckets[((ns / BUCKET_NS) as usize).min(BUCKETS - 1)] += 1;
        self.n += 1;
    }

    /// Number of samples counted.
    pub fn count(&self) -> u64 {
        self.n
    }

    /// Nearest-rank percentile in ms, as the upper edge of its bucket (so within +0.1 ms).
    pub fn pct(&self, p: u64) -> Option<f64> {
        Some((self.bucket(p)? + 1) as f64 * BUCKET_NS as f64 / 1e6)
    }

    fn bucket(&self, p: u64) -> Option<u64> {
        let rank = (p * self.n).div_ceil(100).max(1);
        let mut seen = 0;
        for (i, c) in self.buckets.iter().enumerate() {
            seen += c;
            if seen >= rank && self.n > 0 {
                return Some(i as u64);
            }
        }
        None
    }
}

/// Header flag: the frame is an IDR (an EncodedVideoChunk of type "key").
pub const FLAG_IDR: u8 = 1;
/// `frame_idx u32, flags u8, capture_ns u64, bridge_ns u64`, little-endian, then Annex B.
pub const HEADER_LEN: usize = 21;
/// Browser message (a one-byte uni stream): "my decoder needs a key frame".
pub const MSG_WANT_KEY: u8 = 1;
/// Browser message: "send a key frame when you can" (the page's replay buffer wants one every
/// 10 s). An IDR request behind its own 1 s limit; nothing is dropped for it.
pub const MSG_REFRESH_KEY: u8 = 2;

/// The per-frame stream header. `frame_idx` is the tap's index truncated to 32 bits.
pub fn frame_header(idx: u64, flags: u8, capture_ns: u64, bridge_ns: u64) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0..4].copy_from_slice(&(idx as u32).to_le_bytes());
    h[4] = flags;
    h[5..13].copy_from_slice(&capture_ns.to_le_bytes());
    h[13..21].copy_from_slice(&bridge_ns.to_le_bytes());
    h
}

/// Audio datagram header: `seq u16, ts_48k u32`, little-endian, then the Opus packet.
pub const AUDIO_HEADER_LEN: usize = 6;
/// Sunshine's RTP audio timestamp counts milliseconds (`timestamp += packetDuration`,
/// stream.cpp:1901); the page gets 48 kHz samples.
const TS_48K_PER_RTP: u32 = 48;

/// The datagram for one Opus frame: seq, 48 kHz timestamp, then the data.
pub fn audio_datagram(f: &OpusFrame) -> Vec<u8> {
    let mut d = Vec::with_capacity(AUDIO_HEADER_LEN + f.data.len());
    d.extend_from_slice(&f.seq.to_le_bytes());
    d.extend_from_slice(&f.ts.wrapping_mul(TS_48K_PER_RTP).to_le_bytes());
    d.extend_from_slice(&f.data);
    d
}

/// Counters for the audio path.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AudioStats {
    /// Opus packets out of FEC and decryption.
    pub packets: u64,
    /// Of those, rebuilt from parity.
    pub recovered: u64,
    /// Data packets neither received nor recovered.
    pub lost: u64,
    /// Datagrams AudioFec rejects or payloads that fail decryption.
    pub errors: u64,
    /// Queued as datagrams to a viewer.
    pub sent: u64,
    /// Sends into a full datagram buffer, each evicting the oldest queued datagram (exactly one
    /// for Sunshine's CBR packets, which are all the same size).
    pub evicted: u64,
    /// Larger than the connection's datagram limit: skipped.
    pub too_large: u64,
    /// Dropped before the forward loop because its queue was full.
    pub queue_full: u64,
    /// Packets held back behind a gap and then sent in seq order.
    pub reordered: u64,
    /// Missing seqs the reorder waited for and gave up on (FEC could not rebuild them in time).
    pub skipped: u64,
    /// Packets (usually late FEC rebuilds) behind ones already sent: dropped.
    pub late: u64,
}

impl std::fmt::Display for AudioStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "audio\tpackets {} recovered {} lost {} errors {} reordered {} skipped {} late {} sent {} evicted {} too_large {} queue_full {}",
            self.packets,
            self.recovered,
            self.lost,
            self.errors,
            self.reordered,
            self.skipped,
            self.late,
            self.sent,
            self.evicted,
            self.too_large,
            self.queue_full
        )
    }
}

/// Longest a packet is held behind a gap. A packet lost in slot 0 of a block is rebuilt when the
/// block's parity arrives, right after slot 3: 15 ms later, plus jitter.
pub const REORDER_HOLD_NS: u64 = 20_000_000;
/// A packet further behind than this is a restarted stream, not a late one.
const MAX_LATE: i16 = 16;

fn block_of(seq: u16) -> u16 {
    seq & !(brisk_proto::audio::DATA_SHARDS as u16 - 1)
}

/// Sunshine audio datagrams in, Opus packets out in strict seq order (sans-io).
///
/// AudioFec passes data packets through as they arrive and rebuilds a lost one only when the
/// block's parity comes, after the later data of that block. So on a gap the packets behind it
/// are held (only then: no added delay without loss) until the rebuild fills it, the next block
/// starts (its parity was sent before), or [`REORDER_HOLD_NS`] passes ([`Self::poll`]). At most
/// one block (3 packets) is ever held.
pub struct AudioRx {
    fec: AudioFec,
    cipher: Option<AudioCipher>,
    /// Next seq to send.
    next: Option<u16>,
    /// Held packets, in seq order, all in `next`'s block.
    held: Vec<OpusFrame>,
    held_since: Option<u64>,
    /// What the audio queue has counted so far.
    pub stats: AudioStats,
}

impl AudioRx {
    /// `enc_flags` as negotiated; the cipher is used only with `SS_ENC_AUDIO`.
    pub fn new(enc_flags: u32, rikey: [u8; 16], rikeyid: u32) -> Self {
        Self {
            fec: AudioFec::new(brisk_client::stream::AUDIO_PACKET_MS),
            cipher: (enc_flags & SS_ENC_AUDIO != 0).then(|| AudioCipher::new(&rikey, rikeyid)),
            next: None,
            held: Vec::with_capacity(brisk_proto::audio::DATA_SHARDS),
            held_since: None,
            stats: AudioStats::default(),
        }
    }

    /// One audio datagram (data or parity) at `now_ns`; returns the Opus packets to send now.
    pub fn push(&mut self, d: &[u8], now_ns: u64) -> Vec<OpusFrame> {
        let ps = match self.fec.push(d) {
            Ok(ps) => ps,
            Err(_) => {
                self.stats.errors += 1;
                Vec::new()
            }
        };
        let fs = self.fec.stats();
        (self.stats.recovered, self.stats.lost) = (fs.recovered, fs.lost);
        let mut out = Vec::with_capacity(ps.len());
        for p in ps {
            let f = match &self.cipher {
                Some(c) => match c.open(&p) {
                    Ok(f) => f,
                    Err(_) => {
                        self.stats.errors += 1;
                        continue;
                    }
                },
                None => OpusFrame {
                    seq: p.seq,
                    ts: p.ts,
                    data: p.payload,
                },
            };
            self.stats.packets += 1;
            self.take(f, now_ns, &mut out);
        }
        out
    }

    /// When [`Self::poll`] must run to give up on a gap; `None` while nothing is held.
    pub fn deadline(&self) -> Option<u64> {
        self.held_since.map(|t| t + REORDER_HOLD_NS)
    }

    /// The held packets, in order, once the deadline has passed.
    pub fn poll(&mut self, now_ns: u64) -> Vec<OpusFrame> {
        let mut out = Vec::new();
        if self.deadline().is_some_and(|t| now_ns >= t) {
            let after = self.held.last().map(|f| f.seq.wrapping_add(1));
            self.give_up(&mut out);
            self.next = after;
        }
        out
    }

    fn take(&mut self, f: OpusFrame, now_ns: u64, out: &mut Vec<OpusFrame>) {
        let Some(next) = self.next else {
            // Start at the block's first slot: AudioRx starts mid-stream (when the tap connects)
            // and parity may still rebuild the slots before `f`.
            self.next = Some(block_of(f.seq));
            return self.take(f, now_ns, out);
        };
        let d = f.seq.wrapping_sub(next) as i16;
        if d == 0 {
            self.next = Some(f.seq.wrapping_add(1));
            out.push(f);
            while self.held.first().map(|h| h.seq) == self.next {
                let h = self.held.remove(0);
                self.next = Some(h.seq.wrapping_add(1));
                self.stats.reordered += 1;
                out.push(h);
            }
            if self.held.is_empty() {
                self.held_since = None;
            }
        } else if (-MAX_LATE..0).contains(&d) {
            self.stats.late += 1;
        } else if d > 0 && block_of(f.seq) == block_of(next) {
            if let Err(i) = self
                .held
                .binary_search_by_key(&f.seq.wrapping_sub(next), |h| h.seq.wrapping_sub(next))
            {
                self.held.insert(i, f);
            }
            self.held_since.get_or_insert(now_ns);
        } else {
            // A later block (its gap's parity is gone by) or a restarted stream: send what is
            // held, then treat `f` from the start of its own block, where FEC may still help.
            self.give_up(out);
            self.next = Some(if d > 0 { block_of(f.seq) } else { f.seq });
            self.take(f, now_ns, out);
        }
    }

    /// Sends the held packets in order, counting the seqs before and between them as skipped.
    fn give_up(&mut self, out: &mut Vec<OpusFrame>) {
        if let (Some(next), Some(last)) = (self.next, self.held.last()) {
            let span = last.seq.wrapping_sub(next) as u64 + 1;
            self.stats.skipped += span - self.held.len() as u64;
        }
        self.stats.reordered += self.held.len() as u64;
        out.append(&mut self.held);
        self.held_since = None;
    }
}

/// One Opus packet to the viewer as a datagram; never waits, never fails the session.
pub fn send_audio(s: &wt::Session, f: &OpusFrame, st: &mut AudioStats) {
    match s.send_datagram(&audio_datagram(f)) {
        Ok(evicted) => {
            st.sent += 1;
            st.evicted += evicted as u64;
        }
        Err(quinn::SendDatagramError::TooLarge) => st.too_large += 1,
        // Gone or no datagram support: the viewer's close is handled by the forward loop.
        Err(_) => {}
    }
}

/// Counters over the whole run (all viewers).
#[derive(Default)]
pub struct Report {
    /// Frames written to viewers.
    pub frames: u64,
    /// Viewers that connected.
    pub viewers: u32,
    /// Browser-leg counters of the viewers that left.
    pub pacer: PacerStats,
    /// ... and of the current one (forward is cancelled at exit, so this is kept up to date).
    pub pacer_now: PacerStats,
    /// `bridge_ns - capture_ns` of every frame written to a stream.
    pub bridge_capture: Hist,
    /// `tap_ns - capture_ns` of every tap frame (= Sunshine's `frame_processing_latency`).
    pub tap_capture: Hist,
    /// `bridge_ns - tap_ns` of every frame written: what the tap socket and the bridge add.
    pub bridge_tap: Hist,
    /// Audio counters.
    pub audio: AudioStats,
    /// Input counters.
    pub input: input::InputStats,
}

impl Report {
    /// The M2 gate: `bridge_write - capture` p50 <= `frame_processing_latency` p50 + 2 ms.
    pub fn gate(&self) -> Option<bool> {
        // In whole buckets: 2 ms = 20 of them.
        Some(self.bridge_capture.bucket(50)? <= self.tap_capture.bucket(50)? + 20)
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ms = |h: &Hist, p| h.pct(p).map_or("-".into(), |v| format!("{v:.1}"));
        writeln!(f, "tap_frames\t{}", self.frames)?;
        writeln!(f, "viewers\t{}", self.viewers)?;
        let mut s = self.pacer;
        s += self.pacer_now;
        writeln!(
            f,
            "browser_leg\tsent {} dropped {} reset {} idr_requests {} refreshes {}",
            s.sent, s.dropped, s.reset, s.idr_requests, s.refreshes
        )?;
        for (name, h) in [
            ("bridge_write-capture", &self.bridge_capture),
            ("frame_processing(tap-capture)", &self.tap_capture),
            ("bridge_write-tap", &self.bridge_tap),
        ] {
            writeln!(
                f,
                "{name}\tp50 {} p99 {} ms (n {})",
                ms(h, 50),
                ms(h, 99),
                h.count()
            )?;
        }
        writeln!(f, "{}", self.audio)?;
        writeln!(f, "{}", self.input)?;
        let gate = match self.gate() {
            Some(true) => "PASS",
            Some(false) => "FAIL",
            None => "no data",
        };
        write!(
            f,
            "gate(bridge_write p50 <= frame_processing p50 + 2 ms)\t{gate}"
        )
    }
}

/// How long a new connection may take to open its WebTransport session.
const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);

struct Done {
    viewer: u32,
    idx: u64,
    /// When the frame was written (None: never opened).
    bridge_ns: Option<u64>,
    capture_ns: u64,
    tap_ns: u64,
    /// The browser acknowledged the whole stream.
    acked: bool,
}

struct Viewer {
    id: u32,
    s: Arc<wt::Session>,
    inbox: mpsc::Receiver<Bytes>,
    pacer: Pacer,
    /// Dropping a sender resets that frame's stream.
    cancels: HashMap<u64, oneshot::Sender<()>>,
}

enum Ev {
    Incoming(Option<Box<quinn::Incoming>>),
    Viewer(wt::Session, mpsc::Receiver<Bytes>),
    Frame(brisk_client::Result<TapFrame>),
    Done(Done),
    Msg(Bytes),
    Gone(quinn::ConnectionError),
    Audio(Bytes),
    /// A held audio gap timed out.
    AudioDue,
}

/// Forwards tap frames and the audio datagrams from `audio` (raw, from the RTP socket) to the
/// newest viewer until the tap or the control session fails; never returns `Ok`. A new viewer
/// replaces the old one (a reloaded tab). Each viewer's input streams feed `input` (sent to the
/// host by [`input::send_loop`], which runs beside this).
#[expect(clippy::too_many_arguments)]
pub async fn forward(
    tap: &mut TapConn,
    ep: &quinn::Endpoint,
    token: &str,
    ctl: &ControlHandle,
    audio: &mut mpsc::Receiver<Bytes>,
    arx: &mut AudioRx,
    input: &Arc<input::Shared>,
    r: &mut Report,
) -> brisk_client::Result<()> {
    let acceptor = wt::Acceptor::new(token, HANDSHAKE_WAIT);
    let (viewer_tx, mut viewer_rx) = mpsc::channel(1);
    // One message per spawned frame task; tasks are bounded by the pacers.
    let (done_tx, mut done_rx) = mpsc::unbounded_channel();
    let mut viewer: Option<Viewer> = None;
    loop {
        let audio_due = arx
            .deadline()
            .map(|t| Duration::from_nanos(t.saturating_sub(monotonic_ns())));
        let ev = tokio::select! {
            i = ep.accept() => Ev::Incoming(i.map(Box::new)),
            Some((s, inbox)) = viewer_rx.recv() => Ev::Viewer(s, inbox),
            f = tap.recv() => Ev::Frame(f),
            Some(d) = done_rx.recv() => Ev::Done(d),
            Some(a) = audio.recv() => Ev::Audio(a),
            _ = tokio::time::sleep(audio_due.unwrap_or_default()), if audio_due.is_some() => Ev::AudioDue,
            e = viewer_event(&mut viewer) => e,
        };
        if let Some(v) = &viewer {
            r.pacer_now = v.pacer.stats;
        }
        match ev {
            Ev::Incoming(None) => return Err(Error::Closed("webtransport endpoint")),
            Ev::Incoming(Some(i)) => {
                let peer = i.remote_address();
                let Some(h) = acceptor.admit(ep, *i) else {
                    tracing::warn!("viewer {peer}: refused (too many connections)");
                    continue;
                };
                let tx = viewer_tx.clone();
                tokio::spawn(async move {
                    match h.await {
                        Ok(v) => {
                            let _ = tx.send(v).await;
                        }
                        Err(e) => tracing::warn!("viewer {peer}: {e}"),
                    }
                });
            }
            Ev::Viewer(s, inbox) => {
                if let Some(old) = viewer.take() {
                    old.s.close();
                    r.pacer += std::mem::take(&mut r.pacer_now);
                }
                r.viewers += 1;
                println!("viewer\t{} connected", s.remote());
                let s = Arc::new(s);
                tokio::spawn(input::serve(s.clone(), input.clone()));
                viewer = Some(Viewer {
                    id: r.viewers,
                    s,
                    inbox,
                    pacer: Pacer::new(),
                    cancels: HashMap::new(),
                });
            }
            Ev::Frame(f) => {
                let f = f?;
                r.frames += 1;
                if f.capture_ns != 0 {
                    r.tap_capture.add(f.tap_ns.saturating_sub(f.capture_ns));
                }
                let Some(v) = viewer.as_mut() else { continue };
                let d = v.pacer.on_frame(f.idx, f.idr, monotonic_ns());
                for i in d.reset {
                    v.cancels.remove(&i);
                }
                if d.request_idr {
                    request_idr(ctl).await?;
                }
                if d.send {
                    let (cancel_tx, cancel) = oneshot::channel();
                    v.cancels.insert(f.idx, cancel_tx);
                    tokio::spawn(send_frame(v.s.clone(), v.id, f, done_tx.clone(), cancel));
                }
            }
            Ev::Done(d) => {
                if let Some(b) = d.bridge_ns {
                    r.bridge_tap.add(b.saturating_sub(d.tap_ns));
                    if d.capture_ns != 0 {
                        r.bridge_capture.add(b.saturating_sub(d.capture_ns));
                    }
                }
                if let Some(v) = viewer.as_mut().filter(|v| v.id == d.viewer) {
                    v.pacer.on_done(d.idx, d.acked);
                    v.cancels.remove(&d.idx);
                }
            }
            Ev::Msg(m) => {
                let v = viewer.as_mut().expect("a message comes from a viewer");
                let now = monotonic_ns();
                if (m[..] == [MSG_WANT_KEY] && v.pacer.want_key(now))
                    || (m[..] == [MSG_REFRESH_KEY] && v.pacer.refresh_key(now))
                {
                    request_idr(ctl).await?;
                }
            }
            Ev::Audio(_) | Ev::AudioDue => {
                let fs = match ev {
                    Ev::Audio(d) => arx.push(&d, monotonic_ns()),
                    _ => arx.poll(monotonic_ns()),
                };
                for f in fs {
                    if let Some(v) = &viewer {
                        send_audio(&v.s, &f, &mut arx.stats);
                    }
                }
                r.audio = arx.stats; // queue_full is the caller's
            }
            Ev::Gone(e) => {
                let v = viewer.take().expect("a close comes from a viewer");
                println!("viewer\t{} gone: {e}", v.s.remote());
                r.pacer += std::mem::take(&mut r.pacer_now);
            }
        }
    }
}

/// The current viewer's next message or its close; pending without a viewer.
async fn viewer_event(v: &mut Option<Viewer>) -> Ev {
    let Some(v) = v else {
        return std::future::pending().await;
    };
    tokio::select! {
        // The inbox ends only with the connection: then `closed` resolves too.
        Some(m) = v.inbox.recv() => Ev::Msg(m),
        e = v.s.closed() => Ev::Gone(e),
    }
}

/// `Closed` (the session is gone) is fatal; a `Timeout` (not drained for 1 s) only skips this
/// request, the pacer asks again 100 ms later.
async fn request_idr(ctl: &ControlHandle) -> brisk_client::Result<()> {
    match ctl.request_idr().await {
        Err(e @ Error::Closed(_)) => Err(e),
        Err(e) => {
            tracing::warn!("IDR request skipped: {e}");
            Ok(())
        }
        Ok(()) => Ok(()),
    }
}

/// What became of one frame's stream.
#[derive(Debug)]
pub struct Sent {
    /// When the frame was written (None: the stream never opened).
    pub bridge_ns: Option<u64>,
    /// The browser acknowledged the whole stream.
    pub acked: bool,
}

/// One frame on one uni stream; reset if `cancel` fires (or its sender is dropped) first.
pub async fn send_frame_stream(
    s: &wt::Session,
    f: &TapFrame,
    cancel: &mut oneshot::Receiver<()>,
) -> Sent {
    let mut sent = Sent {
        bridge_ns: None,
        acked: false,
    };
    let opened = tokio::select! {
        o = s.open_uni((f.idx & 0x7fff_ffff) as i32) => o.ok(),
        _ = &mut *cancel => None,
    };
    if let Some(mut st) = opened {
        let now = monotonic_ns();
        sent.bridge_ns = Some(now);
        let flags = if f.idr { FLAG_IDR } else { 0 };
        let h = frame_header(f.idx, flags, f.capture_ns, now);
        let written = tokio::select! {
            w = write_frame(&mut st, &h, f.annexb.clone()) => Some(w.is_some()),
            _ = &mut *cancel => None,
        };
        match written {
            Some(acked) => sent.acked = acked,
            None => {
                let _ = st.reset(0u32.into());
            }
        }
    }
    sent
}

async fn send_frame(
    s: Arc<wt::Session>,
    viewer: u32,
    f: TapFrame,
    done: mpsc::UnboundedSender<Done>,
    mut cancel: oneshot::Receiver<()>,
) {
    let sent = send_frame_stream(&s, &f, &mut cancel).await;
    let _ = done.send(Done {
        viewer,
        idx: f.idx,
        bridge_ns: sent.bridge_ns,
        capture_ns: f.capture_ns,
        tap_ns: f.tap_ns,
        acked: sent.acked,
    });
}

/// Writes header and payload, finishes, and waits until the browser acknowledged it all;
/// `None` if any step failed or the browser stopped the stream.
async fn write_frame(st: &mut quinn::SendStream, h: &[u8], annexb: Bytes) -> Option<()> {
    st.write_all(h).await.ok()?;
    st.write_chunk(annexb).await.ok()?;
    st.finish().ok()?;
    // Ok(Some(code)): the browser sent STOP_SENDING, which is not an acknowledgement.
    match st.stopped().await {
        Ok(None) => Some(()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_layout_is_little_endian_21_bytes() {
        let h = frame_header(0x1_0000_0102, FLAG_IDR, 0x0807_0605_0403_0201, 0x11);
        assert_eq!(&h[0..4], &[0x02, 0x01, 0, 0], "idx truncated to u32");
        assert_eq!(h[4], 1);
        assert_eq!(&h[5..13], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&h[13..21], &[0x11, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn hist_percentiles_are_nearest_rank_bucket_edges() {
        let mut h = Hist::default();
        assert_eq!(h.pct(50), None);
        for ms10 in 1..=100u64 {
            h.add(ms10 * BUCKET_NS - 1); // 0.0999.. ms .. 9.9999.. ms
        }
        assert_eq!(h.pct(50), Some(5.0));
        assert_eq!(h.pct(99), Some(9.9));
        assert_eq!(h.pct(0), Some(0.1));
        h.add(10_000 * BUCKET_NS); // 1 s: clamps into the last bucket
        assert_eq!(h.pct(100), Some(100.1));
        assert_eq!(h.count(), 101);
    }

    #[test]
    fn gate_compares_p50s_with_2ms_slack() {
        let mut r = Report::default();
        assert_eq!(r.gate(), None);
        r.tap_capture.add(5_000_000);
        r.bridge_capture.add(7_000_000);
        assert_eq!(r.gate(), Some(true));
        let mut r2 = Report::default();
        r2.tap_capture.add(5_000_000);
        r2.bridge_capture.add(7_100_000);
        assert_eq!(r2.gate(), Some(false));
    }
}
