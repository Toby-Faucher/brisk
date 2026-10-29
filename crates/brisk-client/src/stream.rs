//! Live A/V receive for `brisk-probe stream`: the control session with A/V pings, every media
//! datagram fed through brisk_proto (video: [`VideoCipher`] when `SS_ENC_VIDEO`, then
//! [`parse_video_packet`] and [`FrameAssembler`]; audio: [`AudioFec`] then [`AudioCipher`] when
//! `SS_ENC_AUDIO`), IDR requests at 1/3 and 2/3 of the run, and the M1 gate counters.

use crate::control::{AvPing, ControlSession, Stats};
use crate::rtsp::StreamPorts;
use crate::tap::{TapConn, TapListener, TapRx, TapSummary, monotonic_ns};
use brisk_proto::audio::{AudioCipher, AudioFec, PT_AUDIO};
use brisk_proto::rtp::{AssembledFrame, FrameAssembler, VideoCipher, parse_video_packet};
use brisk_proto::sdp::{Codec, SS_ENC_AUDIO, SS_ENC_VIDEO};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The gate's frame rate (1080p60).
pub const FPS: f64 = 60.0;
/// An IDR must arrive this soon after each request.
pub const IDR_DEADLINE: Duration = Duration::from_secs(2);
/// `x-nv-aqos.packetDuration` we announce (sdp.rs).
pub const AUDIO_PACKET_MS: u32 = 5;
/// Recorded samples: at least this many video datagrams and the whole first IDR frame.
const VIDEO_SAMPLES: u32 = 120;
const VIDEO_SAMPLES_MAX: u32 = 999;
const AUDIO_SAMPLES: u32 = 100;

/// Nearest-rank percentiles (`ps` integer percents in 0..=100, exact) of `v`; `None` when `v` is empty.
pub fn percentiles(v: &[f64], ps: &[u32]) -> Option<Vec<f64>> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    Some(
        ps.iter()
            .map(|p| s[((*p as usize * s.len()).div_ceil(100)).clamp(1, s.len()) - 1])
            .collect(),
    )
}

/// Counts packets lost from gaps in a u16 sequence.
// shortcut: a late packet only refunds one gap slot and no window is kept; fine for an estimate.
#[derive(Debug, Default, Clone, Copy)]
struct SeqLoss {
    last: Option<u16>,
    received: u64,
    missing: u64,
}

impl SeqLoss {
    fn see(&mut self, seq: u16) {
        self.received += 1;
        let Some(last) = self.last else {
            self.last = Some(seq);
            return;
        };
        let d = seq.wrapping_sub(last) as i16;
        if d > 0 {
            self.missing += d as u64 - 1;
            self.last = Some(seq);
        } else if d < 0 {
            self.missing = self.missing.saturating_sub(1);
        }
    }
}

/// Writes the first datagrams of the stream for the replay tests in brisk-proto.
struct Samples {
    dir: PathBuf,
    video: u32,
    audio: u32,
}

const AUDIO_README: &str = "# Audio samples: READ BEFORE COMMITTING\n\n\
`rikey.hex` is `<rikey, 32 hex> <rikeyid, 8 hex>`: the AES key of ONE streaming \
session. The rikey is also that session's control/input AES-GCM key (and its video key if \
SS_ENC_VIDEO). `brisk-probe stream` draws a fresh random rikey for every launch and the session \
was cancelled right after recording, so the key is ephemeral and protects nothing else. If the \
cancel failed this claim does not hold: check the printed `cancelled` line before committing. \
Files here are written 0644 deliberately (they are meant to be committed). \
It is written only with `--record-samples`. It must NEVER be a real long-lived key: never copy a \
paired identity, a pairing secret or anything from ~/.config/brisk here.\n\n\
NNN.bin are raw audio datagrams as received (data packets: AES-128-CBC ciphertext; FEC packets: \
parity over the ciphertext).\n";

impl Samples {
    fn new(dir: &Path, rikey: &[u8; 16], rikeyid: u32, audio_enc: bool) -> crate::Result<Self> {
        std::fs::create_dir_all(dir.join("video"))?;
        let a = dir.join("audio");
        std::fs::create_dir_all(&a)?;
        let audio = if audio_enc {
            let hex = brisk_proto::pair::hex(rikey).to_ascii_lowercase();
            std::fs::write(a.join("rikey.hex"), format!("{hex} {rikeyid:08x}\n"))?;
            std::fs::write(a.join("README.md"), AUDIO_README)?;
            0
        } else {
            tracing::warn!(
                "record-samples: audio is not encrypted on this session; no audio samples"
            );
            AUDIO_SAMPLES
        };
        Ok(Samples {
            dir: dir.to_owned(),
            video: 0,
            audio,
        })
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Summary {
    pub secs: f64,
    pub frames: u64,
    pub idr_frames: u64,
    pub frames_dropped: u64,
    pub fec_recovered: u64,
    /// Video: undecryptable, unparseable or inconsistent packets and insane frames.
    pub video_parse_errors: u64,
    /// Audio: datagrams AudioFec rejects or payloads that fail decryption.
    pub audio_parse_errors: u64,
    pub audio_ok: u64,
    pub audio_recovered: u64,
    pub audio_lost: u64,
    pub bad_host_msgs: u32,
    /// `frame_processing_latency` in ms: min, median, p99.
    pub host_latency_ms: Option<Vec<f64>>,
    /// |inter-frame arrival delta - 1/60 s| in ms: p50, p99.
    pub jitter_ms: Option<Vec<f64>>,
    /// Assembled Annex B bytes / wire bytes, in Mbit/s over the time video arrived.
    pub video_mbps: (f64, f64),
    /// (missing, received) from sequence gaps.
    pub video_loss: (u64, u64),
    pub audio_loss: (u64, u64),
    /// Per IDR request: ms to the next assembled IDR frame, `None` if none came.
    pub idr_response_ms: Vec<Option<f64>>,
    /// Empty when the gate passes.
    pub failures: Vec<String>,
    pub control: Stats,
    /// Effective SO_RCVBUF (video, audio) in bytes; 0 if unknown.
    pub rcvbuf: (usize, usize),
}

impl Summary {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

fn fmt_opt(v: &Option<Vec<f64>>, names: &[&str]) -> String {
    match v {
        Some(v) => names
            .iter()
            .zip(v)
            .map(|(n, x)| format!("{n} {x:.2}"))
            .collect::<Vec<_>>()
            .join(" "),
        None => "n/a".into(),
    }
}

fn pct(l: (u64, u64)) -> f64 {
    let all = l.0 + l.1;
    if all == 0 {
        0.0
    } else {
        100.0 * l.0 as f64 / all as f64
    }
}

impl std::fmt::Display for Summary {
    /// Fixed labels, one per line, for copying into docs/m0-baseline.md.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let want = 0.95 * FPS * self.secs;
        let idr: Vec<String> = self
            .idr_response_ms
            .iter()
            .map(|r| r.map_or("none".into(), |ms| format!("{ms:.1}")))
            .collect();
        let rows: [(&str, String); 21] = [
            ("duration_s", format!("{:.1}", self.secs)),
            (
                "frames_assembled",
                format!("{} (gate >= {})", self.frames, want.ceil()),
            ),
            ("idr_frames", self.idr_frames.to_string()),
            ("frames_dropped", self.frames_dropped.to_string()),
            ("fec_recovered", self.fec_recovered.to_string()),
            (
                "parse_errors",
                format!(
                    "{} (video {} audio {})",
                    self.video_parse_errors + self.audio_parse_errors,
                    self.video_parse_errors,
                    self.audio_parse_errors
                ),
            ),
            ("audio_packets_ok", self.audio_ok.to_string()),
            ("audio_recovered", self.audio_recovered.to_string()),
            ("audio_lost", self.audio_lost.to_string()),
            ("bad_host_msgs", self.bad_host_msgs.to_string()),
            (
                "host_latency_ms",
                fmt_opt(&self.host_latency_ms, &["min", "median", "p99"]),
            ),
            (
                "arrival_jitter_ms",
                fmt_opt(&self.jitter_ms, &["p50", "p99"]),
            ),
            (
                "video_mbps",
                format!(
                    "effective {:.2} wire {:.2}",
                    self.video_mbps.0, self.video_mbps.1
                ),
            ),
            (
                "video_loss",
                format!(
                    "missing {} received {} ({:.3}%)",
                    self.video_loss.0,
                    self.video_loss.1,
                    pct(self.video_loss)
                ),
            ),
            (
                "audio_loss",
                format!(
                    "missing {} received {} ({:.3}%)",
                    self.audio_loss.0,
                    self.audio_loss.1,
                    pct(self.audio_loss)
                ),
            ),
            ("idr_response_ms", idr.join(" ")),
            (
                "idr_requests",
                format!(
                    "{} ({} on frame loss)",
                    self.control.idr_sent, self.control.idr_on_loss
                ),
            ),
            ("inputs_sent", self.control.inputs_sent.to_string()),
            (
                "control_pings",
                format!(
                    "sent {} acked {}",
                    self.control.pings_sent, self.control.pings_acked
                ),
            ),
            (
                "rcvbuf video/audio",
                format!("{} / {}", self.rcvbuf.0, self.rcvbuf.1),
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
        writeln!(f, "=== brisk stream summary ===")?;
        for (k, v) in rows {
            writeln!(f, "{k:<18} {v}")?;
        }
        write!(f, "=== end summary ===")
    }
}

/// Sans-io receive side: feed datagrams with their arrival time, read the counters.
pub struct StreamRx {
    vcipher: Option<VideoCipher>,
    acipher: Option<AudioCipher>,
    asm: FrameAssembler,
    afec: AudioFec,
    samples: Option<Samples>,
    video_errors: u64,
    audio_errors: u64,
    audio_ok: u64,
    idr_frames: u64,
    annexb_bytes: u64,
    wire_bytes: u64,
    video_span: Option<(Instant, Instant)>,
    frame_at: Vec<Instant>,
    latency_100us: Vec<u16>,
    idr_reqs: Vec<(Instant, Option<Instant>)>,
    vloss: SeqLoss,
    aloss: SeqLoss,
}

impl StreamRx {
    /// `enc_flags` as negotiated (`StreamPorts::enc_flags`); `record` = `--record-samples` dir.
    pub fn new(
        enc_flags: u32,
        rikey: [u8; 16],
        rikeyid: u32,
        record: Option<&Path>,
    ) -> crate::Result<Self> {
        let audio_enc = enc_flags & SS_ENC_AUDIO != 0;
        Ok(StreamRx {
            vcipher: (enc_flags & SS_ENC_VIDEO != 0).then(|| VideoCipher::new(&rikey)),
            acipher: audio_enc.then(|| AudioCipher::new(&rikey, rikeyid)),
            asm: FrameAssembler::new(),
            afec: AudioFec::new(AUDIO_PACKET_MS),
            samples: record
                .map(|d| Samples::new(d, &rikey, rikeyid, audio_enc))
                .transpose()?,
            video_errors: 0,
            audio_errors: 0,
            audio_ok: 0,
            idr_frames: 0,
            annexb_bytes: 0,
            wire_bytes: 0,
            video_span: None,
            frame_at: Vec::new(),
            latency_100us: Vec::new(),
            idr_reqs: Vec::new(),
            vloss: SeqLoss::default(),
            aloss: SeqLoss::default(),
        })
    }

    pub fn idr_requested(&mut self, at: Instant) {
        self.idr_reqs.push((at, None));
    }

    /// One datagram from the video (0) or audio (1) socket. Returns true when a video frame was
    /// found lost: the caller should request an IDR (a [`crate::control::MediaSink`]).
    pub fn on_datagram(&mut self, i: usize, d: &[u8], at: Instant) -> bool {
        if i == 0 {
            self.on_video(d, at);
            self.asm.take_dropped() > 0
        } else {
            self.on_audio(d);
            false
        }
    }

    fn on_video(&mut self, d: &[u8], at: Instant) {
        self.wire_bytes += d.len() as u64;
        let span = self.video_span.get_or_insert((at, at));
        span.1 = at;
        let opened;
        let d = match &self.vcipher {
            Some(c) => match c.open(d) {
                Ok(p) => {
                    opened = p;
                    &opened[..]
                }
                Err(_) => return self.video_errors += 1,
            },
            None => d,
        };
        let Ok(p) = parse_video_packet(d, 0) else {
            return self.video_errors += 1;
        };
        self.vloss.see(p.seq);
        if let Some(s) = &mut self.samples
            && s.video < VIDEO_SAMPLES_MAX
            && (s.video < VIDEO_SAMPLES || self.idr_frames == 0)
        {
            let base = s.dir.join(format!("video/{:03}", s.video));
            let meta = format!(
                "{{\"seq\":{},\"frame_idx\":{},\"shard_idx\":{},\"data_shards\":{},\"parity_shards\":{},\"fec_block\":{},\"last_fec_block\":{},\"is_parity\":{}}}\n",
                p.seq,
                p.frame_idx,
                p.shard_idx,
                p.data_shards,
                p.parity_shards,
                p.fec_block,
                p.last_fec_block,
                p.is_parity
            );
            // shortcut: a failed sample write is reported and recording stops; the stream goes on.
            if let Err(e) = std::fs::write(base.with_extension("bin"), d)
                .and_then(|()| std::fs::write(base.with_extension("meta.json"), meta))
            {
                tracing::warn!("record-samples: {e}");
                s.video = VIDEO_SAMPLES_MAX;
            }
            s.video += 1;
        }
        for f in self.asm.push(p) {
            self.on_frame(f, at);
        }
    }

    fn on_frame(&mut self, f: AssembledFrame, at: Instant) {
        self.frame_at.push(at);
        self.annexb_bytes += f.annexb.len() as u64;
        self.latency_100us.extend(f.host_latency_100us);
        if f.is_idr {
            self.idr_frames += 1;
            for r in &mut self.idr_reqs {
                if r.1.is_none() && at >= r.0 {
                    r.1 = Some(at);
                }
            }
        }
    }

    fn on_audio(&mut self, d: &[u8]) {
        if let Some(s) = &mut self.samples
            && s.audio < AUDIO_SAMPLES
        {
            let p = s.dir.join(format!("audio/{:03}.bin", s.audio));
            if let Err(e) = std::fs::write(p, d) {
                tracing::warn!("record-samples: {e}");
                s.audio = AUDIO_SAMPLES;
            }
            s.audio += 1;
        }
        if d.len() >= 4 && d[1] & 0x7f == PT_AUDIO {
            self.aloss.see(u16::from_be_bytes([d[2], d[3]]));
        }
        match self.afec.push(d) {
            Ok(ps) => {
                for p in ps {
                    match &self.acipher {
                        Some(c) if c.open(&p).is_err() => self.audio_errors += 1,
                        _ => self.audio_ok += 1,
                    }
                }
            }
            Err(_) => self.audio_errors += 1,
        }
    }

    /// Counters and the gate for a run of `secs`.
    pub fn summary(&self, secs: f64, control: &Stats) -> Summary {
        let a = self.asm.stats();
        let af = self.afec.stats();
        let frame = 1000.0 / FPS;
        let jitter: Vec<f64> = self
            .frame_at
            .windows(2)
            .map(|w| ((w[1] - w[0]).as_secs_f64() * 1000.0 - frame).abs())
            .collect();
        let lat: Vec<f64> = self
            .latency_100us
            .iter()
            .map(|&l| l as f64 / 10.0)
            .collect();
        let span = self
            .video_span
            .map_or(0.0, |(a, b)| (b - a).as_secs_f64())
            .max(1e-3);
        let mbps = |bytes: u64| bytes as f64 * 8.0 / span / 1e6;
        let idr_response_ms: Vec<Option<f64>> = self
            .idr_reqs
            .iter()
            .map(|(r, got)| got.map(|g| (g - *r).as_secs_f64() * 1000.0))
            .collect();
        let mut s = Summary {
            secs,
            frames: a.frames_ok,
            idr_frames: self.idr_frames,
            frames_dropped: a.frames_dropped,
            fec_recovered: a.fec_recovered,
            video_parse_errors: self.video_errors + a.parse_errors,
            audio_parse_errors: self.audio_errors,
            audio_ok: self.audio_ok,
            audio_recovered: af.recovered,
            audio_lost: af.lost,
            bad_host_msgs: control.bad_msgs,
            host_latency_ms: percentiles(&lat, &[0, 50, 99]),
            jitter_ms: percentiles(&jitter, &[50, 99]),
            video_mbps: (mbps(self.annexb_bytes), mbps(self.wire_bytes)),
            video_loss: (self.vloss.missing, self.vloss.received),
            audio_loss: (self.aloss.missing, self.aloss.received),
            idr_response_ms,
            failures: Vec::new(),
            control: control.clone(),
            rcvbuf: (0, 0),
        };
        let errors = s.video_parse_errors + s.audio_parse_errors;
        if errors > 0 {
            s.failures.push(format!("parse_errors {errors} > 0"));
        }
        let want = 0.95 * FPS * secs;
        if (s.frames as f64) < want {
            s.failures
                .push(format!("frames_assembled {} < {}", s.frames, want.ceil()));
        }
        let deadline = IDR_DEADLINE.as_secs_f64() * 1000.0;
        for (n, r) in s.idr_response_ms.iter().enumerate() {
            if !r.is_some_and(|ms| ms <= deadline) {
                s.failures
                    .push(format!("no IDR within 2s of request {}", n + 1));
            }
        }
        s
    }
}

/// Control address, `X-SS-Connect-Data` and the A/V pinger of a launched session.
pub async fn media_setup(
    host: &str,
    p: &StreamPorts,
) -> crate::Result<(std::net::SocketAddr, u32, AvPing)> {
    let cd = p.connect_data.ok_or(crate::Error::Protocol(
        "SETUP(control) reply had no X-SS-Connect-Data".into(),
    ))?;
    let addr = tokio::net::lookup_host((host, p.control))
        .await?
        .next()
        .ok_or(crate::Error::Setup("control: host did not resolve".into()))?;
    let payload = p.ping_payload.as_deref().ok_or(crate::Error::Protocol(
        "SETUP(audio) reply had no X-SS-Ping-Payload".into(),
    ))?;
    let av = AvPing::new(addr.ip(), p.video, p.audio, payload).await?;
    Ok((addr, cd, av))
}

/// The control session for `secs` with the media fed into a [`StreamRx`], IDR requests at 1/3
/// and 2/3 (and [`ControlSession::input_demo`] at 1/6 with `input_demo`), then a graceful
/// disconnect. Prints the summary block (also on error) and returns it;
/// a failed gate is an error so the caller exits non-zero. Run it inside `launch_session` so the
/// session is cancelled either way.
pub async fn stream(
    host: &str,
    p: &StreamPorts,
    connect_id: u32,
    secs: Duration,
    record: Option<&Path>,
    input_demo: bool,
) -> crate::Result<Summary> {
    match run(host, p, connect_id, secs, record, input_demo, None).await? {
        Outcome::Rtp(s) => Ok(s),
        Outcome::Tap(_) => unreachable!("tap mode without a tap listener"),
    }
}

/// How a [`stream_tap`] session delivered its video.
#[derive(Debug)]
pub enum Outcome {
    Rtp(Summary),
    Tap(TapSummary),
}

/// Waits this long for the first tap connection or RTP video datagram.
pub const TAP_WAIT: Duration = Duration::from_secs(10);
/// `--record-samples`: tap messages written to `<dir>/tap/` (no RTP samples in tap mode).
const TAP_SAMPLES: u32 = 60;

/// [`stream`] for a session announced with `StreamConfig::tap`: whichever comes first after the
/// video ping decides the mode, a tap connection on `tap` with our connect data (tap mode: frames
/// from the tap, IDR requests through a [`crate::control::ControlHandle`], the [`TapSummary`]
/// gate) or an RTP video datagram (stock Sunshine or a failed tap connect: the [`stream`] path).
pub async fn stream_tap(
    host: &str,
    p: &StreamPorts,
    connect_id: u32,
    secs: Duration,
    record: Option<&Path>,
    tap: &TapListener,
) -> crate::Result<Outcome> {
    run(host, p, connect_id, secs, record, false, Some(tap)).await
}

/// `Some` when a tap connection arrived before any RTP video datagram (the bridge runs its own
/// tap loop after this).
pub async fn race(
    c: &mut ControlSession,
    l: &TapListener,
    cd: u32,
    codec: Codec,
    rx: &mut StreamRx,
) -> crate::Result<Option<TapConn>> {
    let accept = l.accept(cd, codec);
    tokio::pin!(accept);
    let deadline = Instant::now() + TAP_WAIT;
    loop {
        let mut rtp = false;
        let mut sink = |i: usize, d: &[u8], at: Instant| {
            rtp |= i == 0;
            rx.on_datagram(i, d, at)
        };
        tokio::select! {
            conn = &mut accept => return conn.map(Some),
            r = c.run_until_with(Instant::now() + Duration::from_millis(20), &mut sink) => r?,
        }
        if rtp {
            return Ok(None);
        }
        if !c.is_open() {
            return Err(crate::Error::Session(
                "host ended the session before the first video frame".into(),
            ));
        }
        if Instant::now() >= deadline {
            return Err(crate::Error::Timeout(format!(
                "no tap connection and no RTP video within {}s",
                TAP_WAIT.as_secs()
            )));
        }
    }
}

/// Tap frames into `t` until `start + secs`, the control session running alongside; IDR
/// requests at 1/3 and 2/3 go through a handle, as the bridge will send them.
async fn tap_loop(
    c: &mut ControlSession,
    conn: &mut TapConn,
    t: &mut TapRx,
    start: Instant,
    secs: Duration,
) -> crate::Result<()> {
    let h = c.handle();
    let mut idr_at = (1..3u32).map(|k| start + secs * k / 3).peekable();
    // RTP datagrams are only counted (`Stats::media_rx_packets`): video must stay at 0.
    let mut sink = |_: usize, _: &[u8], _: Instant| false;
    let run = c.run_until_with(start + secs, &mut sink);
    tokio::pin!(run);
    loop {
        tokio::select! {
            r = &mut run => return r,
            f = conn.recv() => {
                t.on_frame(&f?, monotonic_ns());
                if idr_at.next_if(|&at| Instant::now() >= at).is_some() {
                    h.request_idr().await?;
                    t.idr_requested(monotonic_ns());
                }
            }
        }
    }
}

async fn run(
    host: &str,
    p: &StreamPorts,
    connect_id: u32,
    secs: Duration,
    record: Option<&Path>,
    input_demo: bool,
    tap: Option<&TapListener>,
) -> crate::Result<Outcome> {
    let (rikey, rikeyid) = (p.key.rikey, p.key.rikeyid);
    // shortcut: tap mode records tap frames only, also after an RTP fallback (the RTP sample
    // writer would replace fixtures/audio's key file).
    let rtp_record = if tap.is_some() { None } else { record };
    let mut rx = StreamRx::new(p.enc_flags, rikey, rikeyid, rtp_record)?;
    let mut trx = TapRx::new();
    let mut tap_mode = false;
    let start = Instant::now();
    let mut control = Stats::default();
    let mut rcvbuf = (0, 0);
    let r = async {
        let (addr, cd, av) = media_setup(host, p).await?;
        rcvbuf = av.rcvbuf();
        println!(
            "stream\tvideo {} audio {} control {addr} encryption {:#x}",
            p.video, p.audio, p.enc_flags
        );
        let mut c = ControlSession::connect(addr, cd, rikey, connect_id, Some(av)).await?;
        let r = async {
            let conn = match tap {
                // brisk-probe streams H.264 only.
                Some(l) => race(&mut c, l, cd, Codec::H264, &mut rx).await?,
                None => None,
            };
            if let Some(mut conn) = conn {
                tap_mode = true;
                println!("tap\tconnected at {:.1}s", start.elapsed().as_secs_f32());
                if let Some(d) = record {
                    conn.record(&d.join("tap"), TAP_SAMPLES, vec![rikey.to_vec()])?;
                }
                tap_loop(&mut c, &mut conn, &mut trx, start, secs).await?;
            } else {
                if tap.is_some() {
                    println!("tap\tRTP video arrived first: falling back to RTP");
                }
                if input_demo {
                    c.run_until_with(start + secs / 6, &mut |i, d, at| rx.on_datagram(i, d, at))
                        .await?;
                    if c.is_open() {
                        let n = c
                            .input_demo(&mut |i, d, at| rx.on_datagram(i, d, at))
                            .await?;
                        println!(
                            "input_demo\tsent {n} packets at {:.1}s",
                            start.elapsed().as_secs_f32()
                        );
                    }
                }
                for k in 1..=3u32 {
                    c.run_until_with(start + secs * k / 3, &mut |i, d, at| {
                        rx.on_datagram(i, d, at)
                    })
                    .await?;
                    if k < 3 && c.is_open() {
                        c.request_idr().await?;
                        rx.idr_requested(Instant::now());
                    }
                }
            }
            if !c.is_open() {
                return Err::<(), crate::Error>(crate::Error::Session(format!(
                    "host ended the session: {:?}",
                    c.stats.termination
                )));
            }
            Ok(())
        }
        .await;
        control = c.stats.clone();
        r?;
        control = c.disconnect().await?;
        Ok::<(), crate::Error>(())
    }
    .await;
    let failed = |e: crate::Error| {
        e.context(format!(
            "stream failed after {:.1}s",
            start.elapsed().as_secs_f32()
        ))
    };
    if tap_mode {
        let s = trx.summary(secs.as_secs_f64(), &control);
        println!("{s}");
        r.map_err(failed)?;
        if !s.passed() {
            return Err(crate::Error::Gate(s.failures));
        }
        return Ok(Outcome::Tap(s));
    }
    let mut s = rx.summary(secs.as_secs_f64(), &control);
    s.rcvbuf = rcvbuf;
    println!("{s}");
    r.map_err(failed)?;
    if !s.passed() {
        return Err(crate::Error::Gate(s.failures));
    }
    Ok(Outcome::Rtp(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_correct() {
        assert_eq!(percentiles(&[], &[50]), None);
        assert_eq!(percentiles(&[7.0], &[0, 50, 99]), Some(vec![7.0; 3]));
        let v: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        assert_eq!(
            percentiles(&v, &[0, 50, 99, 100]),
            Some(vec![1.0, 50.0, 99.0, 100.0])
        );
        // Nearest rank: p50 of 4 values is the 2nd, p99 the 4th.
        assert_eq!(
            percentiles(&[4.0, 1.0, 3.0, 2.0], &[50, 99]),
            Some(vec![2.0, 4.0])
        );
    }

    #[test]
    fn percentiles_are_exact_where_float_ceil_is_off() {
        // 0.07 * 100 = 7.000000000000001 in f64: float ceil picks rank 8, the exact rank is 7.
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentiles(&v, &[7]), Some(vec![7.0]));
        assert_eq!(percentiles(&v, &[29]), Some(vec![29.0]));
    }

    #[test]
    fn seq_loss_counts_gaps_and_wraps() {
        let mut l = SeqLoss::default();
        for s in [65534u16, 65535, 1, 2, 0, 5] {
            l.see(s);
        }
        // 0 missing then late (refund), 3 and 4 missing.
        assert_eq!((l.missing, l.received), (2, 6));
    }
}
