//! Tap-to-browser video: [`Pacer`] is the drop policy (sans-io), [`Hist`] the latency counter,
//! [`forward`] the loop that runs them between a tap connection, the WebTransport endpoint and
//! the control session's [`ControlHandle`].
//!
//! Each frame goes out on its own uni stream: [`HEADER_LEN`]-byte header, Annex B, FIN. The
//! stream's send priority is the frame index, so under congestion quinn sends newer frames first
//! (research/m2-tap-design.md section 6). A stream counts as in flight until the browser has
//! acknowledged all of it.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use brisk_client::control::ControlHandle;
use brisk_client::tap::{monotonic_ns, TapConn, TapFrame};
use brisk_client::Error;
use bytes::Bytes;
use tokio::sync::{mpsc, oneshot};

use crate::wt;

/// More unacknowledged frame streams than this means the browser is behind.
pub const MAX_BEHIND: usize = 3;
/// At most one IDR request per this many ns (the tap's rule too).
pub const IDR_EVERY_NS: u64 = 100_000_000;
/// While the browser acknowledges nothing (a dead or frozen tab), the gap between IDR requests
/// doubles up to this, so Sunshine is not made to encode an IDR 10 times a second for nobody.
pub const IDR_MAX_GAP_NS: u64 = 2_000_000_000;

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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PacerStats {
    pub sent: u64,
    pub dropped: u64,
    pub reset: u64,
    pub idr_requests: u64,
}

impl std::ops::AddAssign for PacerStats {
    fn add_assign(&mut self, o: Self) {
        self.sent += o.sent;
        self.dropped += o.dropped;
        self.reset += o.reset;
        self.idr_requests += o.idr_requests;
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
    pub stats: PacerStats,
}

impl Default for Pacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Pacer {
    pub fn new() -> Self {
        Self {
            in_flight: VecDeque::with_capacity(MAX_BEHIND + 1),
            want_key: true,
            last_idr_ns: None,
            idr_gap_ns: IDR_EVERY_NS,
            acked: false,
            stats: PacerStats::default(),
        }
    }

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
    pub fn add(&mut self, ns: u64) {
        self.buckets[((ns / BUCKET_NS) as usize).min(BUCKETS - 1)] += 1;
        self.n += 1;
    }

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

/// The per-frame stream header. `frame_idx` is the tap's index truncated to 32 bits.
pub fn frame_header(idx: u64, flags: u8, capture_ns: u64, bridge_ns: u64) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0..4].copy_from_slice(&(idx as u32).to_le_bytes());
    h[4] = flags;
    h[5..13].copy_from_slice(&capture_ns.to_le_bytes());
    h[13..21].copy_from_slice(&bridge_ns.to_le_bytes());
    h
}

/// Counters over the whole run (all viewers).
#[derive(Default)]
pub struct Report {
    pub frames: u64,
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
            "browser_leg\tsent {} dropped {} reset {} idr_requests {}",
            s.sent, s.dropped, s.reset, s.idr_requests
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
}

/// Forwards tap frames to the newest viewer until the tap or the control session fails; never
/// returns `Ok`. A new viewer replaces the old one (a reloaded tab).
pub async fn forward(
    tap: &mut TapConn,
    ep: &quinn::Endpoint,
    token: &str,
    ctl: &ControlHandle,
    r: &mut Report,
) -> brisk_client::Result<()> {
    let (viewer_tx, mut viewer_rx) = mpsc::channel(1);
    // One message per spawned frame task; tasks are bounded by the pacers.
    let (done_tx, mut done_rx) = mpsc::unbounded_channel();
    let mut viewer: Option<Viewer> = None;
    loop {
        let ev = tokio::select! {
            i = ep.accept() => Ev::Incoming(i.map(Box::new)),
            Some((s, inbox)) = viewer_rx.recv() => Ev::Viewer(s, inbox),
            f = tap.recv() => Ev::Frame(f),
            Some(d) = done_rx.recv() => Ev::Done(d),
            e = viewer_event(&mut viewer) => e,
        };
        if let Some(v) = &viewer {
            r.pacer_now = v.pacer.stats;
        }
        match ev {
            Ev::Incoming(None) => return Err(Error::Closed("webtransport endpoint")),
            Ev::Incoming(Some(i)) => {
                let (tx, token) = (viewer_tx.clone(), token.to_owned());
                tokio::spawn(async move {
                    let peer = i.remote_address();
                    let r = tokio::time::timeout(HANDSHAKE_WAIT, async {
                        wt::accept((*i).await?, &token).await
                    })
                    .await;
                    match r {
                        Ok(Ok(v)) => {
                            let _ = tx.send(v).await;
                        }
                        Ok(Err(e)) => tracing::warn!("viewer {peer}: {e}"),
                        Err(_) => tracing::warn!("viewer {peer}: no session within 10 s"),
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
                viewer = Some(Viewer {
                    id: r.viewers,
                    s: Arc::new(s),
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
                if m[..] == [MSG_WANT_KEY] && v.pacer.want_key(monotonic_ns()) {
                    request_idr(ctl).await?;
                }
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

/// One frame on one uni stream; reset if `cancel` fires (or its sender is dropped) first.
async fn send_frame(
    s: Arc<wt::Session>,
    viewer: u32,
    f: TapFrame,
    done: mpsc::UnboundedSender<Done>,
    mut cancel: oneshot::Receiver<()>,
) {
    let mut d = Done {
        viewer,
        idx: f.idx,
        bridge_ns: None,
        capture_ns: f.capture_ns,
        tap_ns: f.tap_ns,
        acked: false,
    };
    let opened = tokio::select! {
        o = s.open_uni((f.idx & 0x7fff_ffff) as i32) => o.ok(),
        _ = &mut cancel => None,
    };
    if let Some(mut st) = opened {
        let now = monotonic_ns();
        d.bridge_ns = Some(now);
        let flags = if f.idr { FLAG_IDR } else { 0 };
        let h = frame_header(f.idx, flags, f.capture_ns, now);
        let written = tokio::select! {
            w = write_frame(&mut st, &h, f.annexb) => Some(w.is_some()),
            _ = &mut cancel => None,
        };
        match written {
            Some(acked) => d.acked = acked,
            None => {
                let _ = st.reset(0u32.into());
            }
        }
    }
    let _ = done.send(d);
}

/// Writes header and payload, finishes, and waits until the browser acknowledged it all.
async fn write_frame(st: &mut quinn::SendStream, h: &[u8], annexb: Bytes) -> Option<()> {
    st.write_all(h).await.ok()?;
    st.write_chunk(annexb).await.ok()?;
    st.finish().ok()?;
    st.stopped().await.ok()?;
    Some(())
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
