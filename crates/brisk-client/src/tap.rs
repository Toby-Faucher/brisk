//! Bridge side of the Sunshine tap: the patched Sunshine connects (AF_UNIX SOCK_SEQPACKET) to
//! `$BRISK_TAP_SOCK` or `$XDG_RUNTIME_DIR/brisk/tap.sock` after the first video ping of a session
//! that announced `a=x-brisk.tap:1`, and sends one message per encoded frame
//! ([`brisk_proto::tap`] framing). The listener lives in a 0700 directory and only accepts a peer
//! running as our own uid (`SO_PEERCRED`) whose first frame carries our session's connect data.

use brisk_proto::sdp::Codec;
use brisk_proto::tap::{parse_tap, TapError, TapHeader, HEADER_LEN, MAX_PAYLOAD};
use bytes::{Bytes, BytesMut};
use socket2::{Domain, SockAddr, Socket, Type};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt};
use std::path::{Path, PathBuf};
use tokio::io::unix::AsyncFd;
use tokio::io::Interest;

use crate::control::Stats;
use crate::stream::{percentiles, FPS, IDR_DEADLINE};

/// Largest message: the biggest header `header_len` can describe plus the payload cap.
const MAX_MESSAGE: usize = u16::MAX as usize + MAX_PAYLOAD;

/// `$BRISK_TAP_SOCK`, else `$XDG_RUNTIME_DIR/brisk/tap.sock` (the patch's rule).
pub fn tap_path() -> Option<PathBuf> {
    std::env::var_os("BRISK_TAP_SOCK")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR").map(|d| Path::new(&d).join("brisk/tap.sock"))
        })
}

/// `CLOCK_MONOTONIC` in ns: the clock of the header's `capture_ns`/`tap_ns` (libstdc++
/// `steady_clock` [U]). `Instant` does not expose its value, so it cannot be compared.
pub fn monotonic_ns() -> u64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: valid out-pointer; CLOCK_MONOTONIC cannot fail on Linux.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}

fn async_fd(s: Socket) -> io::Result<AsyncFd<Socket>> {
    // SAFETY: the Socket owns its fd, which stays open as long as the AsyncFd owns the Socket.
    Ok(unsafe { AsyncFd::register(s) }?)
}

fn peer_uid(s: &Socket) -> io::Result<u32> {
    let mut c = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `c` and `len` are valid for getsockopt(SO_PEERCRED) on a live socket.
    let r = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut c as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if r != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(c.uid)
}

pub struct TapListener {
    fd: AsyncFd<Socket>,
    path: PathBuf,
    uid: u32,
}

impl TapListener {
    /// Creates the parent directory (0700) if missing, replaces a stale socket file, listens.
    pub fn bind(path: &Path) -> io::Result<TapListener> {
        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
        // Only a socket is removed: a regular file in the way is the caller's mistake.
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket()) {
            std::fs::remove_file(path)?;
        }
        let s = Socket::new(Domain::UNIX, Type::SEQPACKET.cloexec(), None)?;
        s.bind(&SockAddr::unix(path)?)?;
        s.listen(4)?;
        s.set_nonblocking(true)?;
        Ok(TapListener {
            fd: async_fd(s)?,
            path: path.to_owned(),
            // SAFETY: getuid cannot fail.
            uid: unsafe { libc::getuid() },
        })
    }

    /// Test knob: the uid a peer must have (default: ours).
    #[doc(hidden)]
    pub fn expect_uid(mut self, uid: u32) -> Self {
        self.uid = uid;
        self
    }

    /// The first connection from our uid whose first frame carries `connect_data`; its codec must
    /// be `codec`, the one the session announced (else an error). Other peers are closed
    /// (Sunshine then stops that session on EPIPE) and waiting goes on.
    // shortcut: one connection is checked at a time, so a peer that connects and never sends
    // stalls the accept; only our own uid can reach the 0700 directory.
    pub async fn accept(&self, connect_data: u32, codec: Codec) -> crate::Result<TapConn> {
        loop {
            let (s, _) = self.fd.async_io(Interest::READABLE, |l| l.accept()).await?;
            let uid = peer_uid(&s)?;
            if uid != self.uid {
                tracing::warn!("tap: closed a connection from uid {uid}");
                continue;
            }
            s.set_nonblocking(true)?;
            let c = TapConn::new(s)?;
            let h = match c.peek_header().await {
                Ok(Some(h)) => h,
                Ok(None) => continue, // closed before its first frame
                Err(e) => {
                    tracing::warn!("tap: closed a connection whose first message is bad: {e}");
                    continue;
                }
            };
            if h.connect_data != connect_data {
                tracing::warn!(
                    "tap: closed a connection for connect data {:#x}, want {connect_data:#x}",
                    h.connect_data
                );
                continue;
            }
            // The tap header's codec is Sunshine's videoFormat (0 H.264, 1 HEVC, 2 AV1).
            let name = |c: u32| {
                ["H.264", "HEVC", "AV1"]
                    .get(c as usize)
                    .copied()
                    .unwrap_or("?")
            };
            let want = match codec {
                Codec::H264 => 0,
                Codec::Hevc => 1,
            };
            if h.codec != want {
                return Err(crate::Error::Protocol(format!(
                    "tap: codec {} ({}), but the session asked for {}",
                    h.codec,
                    name(h.codec),
                    name(want)
                )));
            }
            return Ok(c);
        }
    }
}

impl Drop for TapListener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// One encoded frame as Sunshine sent it.
#[derive(Debug, Clone)]
pub struct TapFrame {
    pub idx: u64,
    pub idr: bool,
    /// CLOCK_MONOTONIC ns at capture, 0 if Sunshine had none (a duplicated frame).
    pub capture_ns: u64,
    /// CLOCK_MONOTONIC ns when Sunshine sent the frame.
    pub tap_ns: u64,
    /// Frames the tap dropped so far in this session (socket full).
    pub dropped: u32,
    pub annexb: Bytes,
}

pub struct TapConn {
    fd: AsyncFd<Socket>,
    rec: Option<Record>,
}

/// Raw messages written as fixtures, scrubbed of the session's secrets.
struct Record {
    dir: PathBuf,
    left: u32,
    n: u32,
    secrets: Vec<Vec<u8>>,
}

impl TapConn {
    fn new(s: Socket) -> io::Result<TapConn> {
        Ok(TapConn {
            fd: async_fd(s)?,
            rec: None,
        })
    }

    /// Writes the next `frames` messages, as received and scrubbed of `secrets`, to
    /// `dir/NNN.bin` (fixtures for brisk-proto's tap replay test).
    pub fn record(&mut self, dir: &Path, frames: u32, secrets: Vec<Vec<u8>>) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        self.rec = Some(Record {
            dir: dir.to_owned(),
            left: frames,
            n: 0,
            secrets,
        });
        Ok(())
    }

    /// Header of the next message without consuming it; `None` on EOF.
    async fn peek_header(&self) -> crate::Result<Option<TapHeader>> {
        // Up to the largest header_len, so any header parses; the payload is not needed.
        let mut b = vec![MaybeUninit::<u8>::uninit(); u16::MAX as usize];
        let n = self
            .fd
            .async_io(Interest::READABLE, |s| s.peek(&mut b))
            .await?;
        if n == 0 {
            return Ok(None);
        }
        // SAFETY: peek initialised the first `n` bytes.
        let b: &[u8] = unsafe { std::slice::from_raw_parts(b.as_ptr().cast(), n) };
        Ok(Some(parse_tap(b).map(|(h, _)| h)?))
    }

    /// The next frame; [`crate::Error::Closed`] when Sunshine closed the connection. Cancel-safe:
    /// a message is only taken off the socket inside one poll. The message lands in a buffer of
    /// exactly its size and `annexb` points into it (no copy).
    pub async fn recv(&mut self) -> crate::Result<TapFrame> {
        let (m, len) = self
            .fd
            .async_io(Interest::READABLE, |s| {
                // MSG_TRUNC: the full length of the next message, without taking it.
                let len = s.recv_with_flags(&mut [], libc::MSG_PEEK | libc::MSG_TRUNC)?;
                let mut m = BytesMut::with_capacity(len.min(MAX_MESSAGE));
                let n = s.recv(m.spare_capacity_mut())?; // an oversize message is cut here
                                                         // SAFETY: recv initialised the first `n` bytes.
                unsafe { m.set_len(n) };
                Ok((m, len))
            })
            .await?;
        if len == 0 {
            return Err(crate::Error::Closed("tap connection"));
        }
        if len > m.len() {
            return Err(TapError::TooLarge(len - HEADER_LEN).into());
        }
        if let Some(r) = self.rec.as_mut().filter(|r| r.left > 0) {
            r.left -= 1;
            let path = r.dir.join(format!("{:03}.bin", r.n));
            r.n += 1;
            // shortcut: a failed fixture write stops recording; the stream goes on.
            if let Err(e) = std::fs::write(path, brisk_proto::scrub::scrub(&m, &r.secrets)) {
                tracing::warn!("tap record: {e}");
                r.left = 0;
            }
        }
        let m = m.freeze();
        let (h, p) = parse_tap(&m)?;
        let start = m.len() - p.len();
        Ok(TapFrame {
            idx: h.frame_index,
            idr: h.idr(),
            capture_ns: h.capture_ns,
            tap_ns: h.tap_ns,
            dropped: h.dropped_total,
            annexb: m.slice(start..),
        })
    }
}

/// The first SPS NAL unit (type 7) of an Annex B access unit, without its start code.
fn find_sps(annexb: &[u8]) -> Option<&[u8]> {
    let mut nals = annexb
        .windows(3)
        .enumerate()
        .filter(|(_, w)| *w == [0, 0, 1])
        .map(|(i, _)| i + 3)
        .peekable();
    while let Some(start) = nals.next() {
        // the next NAL's start code (3 or 4 bytes) ends this one
        let end = nals.peek().map_or(annexb.len(), |&n| {
            n - 3 - (n >= 4 && annexb[n - 4] == 0) as usize
        });
        if annexb.get(start).is_some_and(|b| b & 0x1f == 7) {
            return Some(&annexb[start..end]);
        }
    }
    None
}

/// Sans-io counters for tap mode (`brisk-probe stream --tap`), all times CLOCK_MONOTONIC ns.
#[derive(Default)]
pub struct TapRx {
    frames: u64,
    idr_frames: u64,
    bytes: u64,
    /// (idx, dropped_total) of the first and the last frame.
    first: Option<(u64, u32)>,
    last: Option<(u64, u32)>,
    idx_gaps: u64,
    out_of_order: u64,
    recv_capture_ms: Vec<f64>,
    host_ms: Vec<f64>,
    transport_ms: Vec<f64>,
    span_ns: Option<(u64, u64)>,
    idr_reqs: Vec<(u64, Option<u64>)>,
    first_sps: Option<Vec<u8>>,
}

const MS: f64 = 1e6;

impl TapRx {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn idr_requested(&mut self, at_ns: u64) {
        self.idr_reqs.push((at_ns, None));
    }

    pub fn on_frame(&mut self, f: &TapFrame, recv_ns: u64) {
        self.frames += 1;
        self.bytes += f.annexb.len() as u64;
        let span = self.span_ns.get_or_insert((recv_ns, recv_ns));
        span.1 = recv_ns;
        if let Some((prev, _)) = self.last {
            if f.idx <= prev {
                self.out_of_order += 1;
            } else {
                self.idx_gaps += f.idx - prev - 1;
            }
        }
        self.first.get_or_insert((f.idx, f.dropped));
        self.last = Some((f.idx, f.dropped));
        if f.capture_ns != 0 {
            self.recv_capture_ms
                .push(recv_ns.saturating_sub(f.capture_ns) as f64 / MS);
            self.host_ms
                .push(f.tap_ns.saturating_sub(f.capture_ns) as f64 / MS);
        }
        self.transport_ms
            .push(recv_ns.saturating_sub(f.tap_ns) as f64 / MS);
        if f.idr {
            self.idr_frames += 1;
            for r in &mut self.idr_reqs {
                if r.1.is_none() && recv_ns >= r.0 {
                    r.1 = Some(recv_ns);
                }
            }
            if self.first_sps.is_none() {
                self.first_sps = find_sps(&f.annexb).map(<[u8]>::to_vec);
            }
        }
    }

    /// Counters and the tap gate for a run of `secs`; `control.media_rx_packets[0]` is the RTP
    /// video datagrams of the session, which must be 0 in tap mode.
    pub fn summary(&self, secs: f64, control: &Stats) -> TapSummary {
        let pct = |v: &[f64]| percentiles(v, &[50, 99]);
        let dropped = match (self.first, self.last) {
            (Some(a), Some(b)) => b.1.wrapping_sub(a.1) as u64,
            _ => 0,
        };
        let span = self.span_ns.map_or(0, |(a, b)| b - a).max(1_000_000) as f64 / 1e9;
        let mut s = TapSummary {
            secs,
            frames: self.frames,
            idr_frames: self.idr_frames,
            idx_gaps: self.idx_gaps,
            dropped,
            out_of_order: self.out_of_order,
            recv_capture_ms: pct(&self.recv_capture_ms),
            host_ms: pct(&self.host_ms),
            transport_ms: pct(&self.transport_ms),
            idr_response_ms: self
                .idr_reqs
                .iter()
                .map(|(r, got)| got.map(|g| (g - r) as f64 / MS))
                .collect(),
            rtp_video_datagrams: control.media_rx_packets[0],
            video_mbps: self.bytes as f64 * 8.0 / span / 1e6,
            first_sps: self.first_sps.clone(),
            failures: Vec::new(),
            control: control.clone(),
        };
        let want = 0.95 * FPS * secs;
        if (s.frames as f64) < want {
            s.failures
                .push(format!("frames {} < {}", s.frames, want.ceil()));
        }
        if s.idx_gaps != s.dropped || s.out_of_order > 0 {
            s.failures.push(format!(
                "idx not contiguous: {} missing, {} counted as dropped, {} out of order",
                s.idx_gaps, s.dropped, s.out_of_order
            ));
        }
        let deadline = IDR_DEADLINE.as_secs_f64() * 1000.0;
        for (n, r) in s.idr_response_ms.iter().enumerate() {
            if !r.is_some_and(|ms| ms <= deadline) {
                s.failures
                    .push(format!("no IDR within 2s of request {}", n + 1));
            }
        }
        if s.rtp_video_datagrams > 0 {
            s.failures.push(format!(
                "rtp_video_datagrams {} > 0 in tap mode",
                s.rtp_video_datagrams
            ));
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TapSummary {
    pub secs: f64,
    pub frames: u64,
    pub idr_frames: u64,
    /// Frame indices skipped between received frames.
    pub idx_gaps: u64,
    /// Growth of the header's dropped_total over the run (frames the tap dropped itself).
    pub dropped: u64,
    pub out_of_order: u64,
    /// p50, p99 in ms: bridge receive - capture.
    pub recv_capture_ms: Option<Vec<f64>>,
    /// p50, p99 in ms: tap send - capture (Sunshine's `frame_processing_latency`).
    pub host_ms: Option<Vec<f64>>,
    /// p50, p99 in ms: bridge receive - tap send.
    pub transport_ms: Option<Vec<f64>>,
    pub idr_response_ms: Vec<Option<f64>>,
    pub rtp_video_datagrams: u64,
    pub video_mbps: f64,
    pub first_sps: Option<Vec<u8>>,
    /// Empty when the gate passes.
    pub failures: Vec<String>,
    pub control: Stats,
}

impl TapSummary {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

impl std::fmt::Display for TapSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let p = |v: &Option<Vec<f64>>| {
            v.as_ref()
                .map_or("n/a".into(), |v| format!("p50 {:.2} p99 {:.2}", v[0], v[1]))
        };
        let idr: Vec<String> = self
            .idr_response_ms
            .iter()
            .map(|r| r.map_or("none".into(), |ms| format!("{ms:.1}")))
            .collect();
        let sps = self.first_sps.as_ref().map_or("none".into(), |s| {
            s.iter().map(|b| format!("{b:02x}")).collect::<String>()
        });
        let rows: [(&str, String); 15] = [
            ("duration_s", format!("{:.1}", self.secs)),
            (
                "frames",
                format!(
                    "{} (gate >= {})",
                    self.frames,
                    (0.95 * FPS * self.secs).ceil()
                ),
            ),
            ("idr_frames", self.idr_frames.to_string()),
            (
                "idx",
                format!(
                    "missing {} dropped_by_tap {} out_of_order {}",
                    self.idx_gaps, self.dropped, self.out_of_order
                ),
            ),
            ("recv_minus_capture_ms", p(&self.recv_capture_ms)),
            ("frame_processing_ms", p(&self.host_ms)),
            ("tap_transport_ms", p(&self.transport_ms)),
            ("idr_response_ms", idr.join(" ")),
            ("idr_requests", self.control.idr_sent.to_string()),
            ("rtp_video_datagrams", self.rtp_video_datagrams.to_string()),
            (
                "rtp_audio_datagrams",
                self.control.media_rx_packets[1].to_string(),
            ),
            ("video_mbps", format!("{:.2}", self.video_mbps)),
            ("first_sps", sps),
            (
                "control_pings",
                format!(
                    "sent {} acked {}",
                    self.control.pings_sent, self.control.pings_acked
                ),
            ),
            (
                "gate",
                if self.passed() {
                    "PASS".into()
                } else {
                    format!("FAIL: {}", self.failures.join("; "))
                },
            ),
        ];
        writeln!(f, "=== brisk tap summary ===")?;
        for (k, v) in rows {
            writeln!(f, "{k:<21} {v}")?;
        }
        write!(f, "=== end summary ===")
    }
}
